use compute_api::dsg_admission::{
    DsgAdmissionContinuationRequest as Request, DsgAdmissionContinuationResponse as Response,
    DsgAdmissionDisposition as D, DsgAdmissionResponse,
};
use compute_api::dsg_control::DsgUint64;
use compute_api::dsg_state::DsgControlResponse;

#[derive(serde::Deserialize)]
struct Vector {
    name: String,
    valid: bool,
    request: serde_json::Value,
    context_sha256: Option<String>,
}

fn vectors() -> Vec<Vector> {
    serde_json::from_str(include_str!(
        "../testdata/dsg_admission_continuation_v1.json"
    ))
    .unwrap()
}

#[test]
fn continuation_vectors_and_independent_digests() {
    let vectors = vectors();
    assert_eq!(vectors.len(), 26);
    for v in vectors {
        let req = serde_json::from_value::<Request>(v.request);
        let valid = req.as_ref().is_ok_and(|r| r.validate().is_ok());
        assert_eq!(valid, v.valid, "{}", v.name);
        if v.valid {
            assert_eq!(
                req.unwrap().context_sha256().unwrap(),
                v.context_sha256.unwrap(),
                "{}",
                v.name
            );
        }
    }
}

fn fixture() -> (Request, Response) {
    let req: Request = serde_json::from_value(vectors().remove(0).request).unwrap();
    let mut base: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    let s = &mut base.observed;
    s.instance = req.close.instance.clone();
    s.operation = None;
    s.issuer_floor_epoch = req.close.issuer_epoch;
    s.capacity_generation = req.close.expected_capacity_generation;
    s.admission_revision = req.close.revision;
    s.admission_open = false;
    let proof = Response {
        protocol_version: 1,
        vm_shrink_permit: false,
        request: req.clone(),
        context_sha256: req.context_sha256().unwrap(),
        prior_may_have_applied: true,
        closed_by: DsgAdmissionResponse {
            protocol_version: 1,
            vm_shrink_permit: false,
            request: req.close.clone(),
            context_sha256: req.close.context_sha256().unwrap(),
            disposition: D::Applied,
            observed: base.observed,
        },
    };
    (req, proof)
}

#[test]
fn continuation_requires_native_close_and_exact_selection() {
    let (req, proof) = fixture();
    proof
        .validate_for(&req, &req.close.instance, false)
        .unwrap();
    assert!(proof.validate_for(&req, &req.close.instance, true).is_err());
    let mut query = proof.clone();
    query.closed_by.disposition = D::Replayed;
    query.closed_by.observed.issuer_floor_epoch = DsgUint64::new(req.close.issuer_epoch.get() + 1);
    query.validate_for(&req, &req.close.instance, true).unwrap();
    assert!(
        query
            .validate_for(&req, &req.close.instance, false)
            .is_err()
    );
    // 逐项破坏闭合证据；NotFound/Expired即使搭配closed布尔也不能授予成功。
    let mutations: [fn(&mut Response); 12] = [
        |p| p.protocol_version = 2,
        |p| p.vm_shrink_permit = true,
        |p| p.prior_may_have_applied = false,
        |p| p.context_sha256 = "0".repeat(64),
        |p| p.request.previous_close.budget_commitment_id = "other".into(),
        |p| p.closed_by.context_sha256 = "0".repeat(64),
        |p| p.closed_by.observed.admission_open = true,
        |p| p.closed_by.request.budget_commitment_id = "other".into(),
        |p| p.closed_by.observed.instance.pg_boot_id = DsgUint64::new(u64::MAX - 1),
        |p| p.closed_by.observed.capacity_generation = DsgUint64::new(3),
        |p| {
            p.closed_by.disposition = D::NotFound;
            p.closed_by.observed.admission_revision =
                DsgUint64::new(p.closed_by.request.revision.get() - 1);
        },
        |p| {
            p.closed_by.disposition = D::ResultExpired;
            p.closed_by.observed.admission_revision =
                DsgUint64::new(p.closed_by.request.revision.get() + 1);
        },
    ];
    for (i, mutate) in mutations.into_iter().enumerate() {
        let mut bad = proof.clone();
        mutate(&mut bad);
        assert!(
            bad.validate_for(&req, &req.close.instance, false).is_err(),
            "mutation {i}"
        );
        assert!(
            bad.validate_for(&req, &req.close.instance, true).is_err(),
            "mutation {i}"
        );
    }
}

#[test]
fn continuation_json_rejects_missing_null_duplicate_case_and_unknown() {
    let (req, proof) = fixture();
    for (raw, response) in [
        (serde_json::to_string(&req).unwrap(), false),
        (serde_json::to_string(&proof).unwrap(), true),
    ] {
        for invalid in [
            raw.replacen("\"protocol_version\":1", "\"protocol_version\":null", 1),
            raw.replacen(
                "\"protocol_version\":1",
                "\"protocol_version\":1,\"protocol_version\":1",
                1,
            ),
            raw.replacen("\"protocol_version\":1,", "", 1),
            raw.replacen("\"protocol_version\":1", "\"Protocol_Version\":1", 1),
            raw.replacen(
                "\"protocol_version\":1",
                "\"protocol_version\":1,\"extra\":false",
                1,
            ),
            format!("{raw} {{}}"),
        ] {
            if response {
                assert!(serde_json::from_str::<Response>(&invalid).is_err());
            } else {
                assert!(serde_json::from_str::<Request>(&invalid).is_err());
            }
        }
    }
}
