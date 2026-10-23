use compute_api::dsg_admission::{
    DsgAdmissionDisposition as D, DsgAdmissionRequest, DsgAdmissionResponse,
};
use compute_api::dsg_control::{DsgInstanceIdentity, DsgUint64};
use compute_api::dsg_state::DsgControlResponse;

#[test]
fn admission_context_matches_native_pg_vectors() {
    #[derive(serde::Deserialize)]
    struct Vector {
        request: DsgAdmissionRequest,
        context_sha256: String,
    }
    // 文件由准确锁定PG的jsonb_out+sha256产生，含两种bool、ASCII转义与uint64高位；
    // Go消费者使用同字节文件，不能用两份手写canonical互相印证。
    let vectors: Vec<Vector> =
        serde_json::from_str(include_str!("../testdata/dsg_admission_context_v1.json")).unwrap();
    assert_eq!(vectors.len(), 2);
    for vector in vectors {
        assert_eq!(
            vector.request.context_sha256().unwrap(),
            vector.context_sha256
        );
    }
}

fn request() -> DsgAdmissionRequest {
    DsgAdmissionRequest {
        protocol_version: 1,
        instance: DsgInstanceIdentity {
            vm_uid: "vm-1".into(),
            compute_instance_id: "compute-1".into(),
            pg_boot_id: DsgUint64::new(u64::MAX),
        },
        issuer_epoch: DsgUint64::new(2),
        revision: DsgUint64::new(1),
        expected_capacity_generation: DsgUint64::new(2),
        open: true,
        budget_commitment_id: "budget-1".into(),
    }
}

#[test]
fn admission_request_is_strict_lossless_and_closure_does_not_claim_no_effect() {
    let req = request();
    req.validate().unwrap();
    let raw = serde_json::to_string(&req).unwrap();
    assert!(raw.contains("\"18446744073709551615\""));
    assert_eq!(
        serde_json::from_str::<DsgAdmissionRequest>(&raw).unwrap(),
        req
    );
    for invalid in [
        raw.replace("\"open\":true", "\"open\":true,\"open\":false"),
        raw.replace("\"open\":true", "\"open\":\"true\""),
        raw.replace("\"revision\":\"1\"", "\"revision\":1"),
        raw.replace("\"revision\":\"1\"", "\"revision\":\"01\""),
        raw.replace("\"open\":true", "\"open\":true,\"extra\":1"),
    ] {
        assert!(serde_json::from_str::<DsgAdmissionRequest>(&invalid).is_err());
    }
    let mut close = req.clone();
    close.open = false;
    close.issuer_epoch = DsgUint64::new(3);
    assert!(close.closes_over(&req).is_ok());
    close.issuer_epoch = req.issuer_epoch;
    assert!(close.closes_over(&req).is_err());
    close.revision = DsgUint64::new(2);
    assert!(close.closes_over(&req).is_ok());
    assert_ne!(
        req.context_sha256().unwrap(),
        close.context_sha256().unwrap()
    );
}

#[test]
fn admission_response_binds_full_context_action_and_current_fact() {
    let req = request();
    let mut observed: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    observed.observed.instance = req.instance.clone();
    observed.observed.operation = None;
    observed.observed.issuer_floor_epoch = req.issuer_epoch;
    observed.observed.capacity_generation = req.expected_capacity_generation;
    observed.observed.admission_open = true;
    observed.observed.admission_revision = req.revision;
    let reply = DsgAdmissionResponse {
        protocol_version: 1,
        vm_shrink_permit: false,
        request: req.clone(),
        disposition: D::Applied,
        context_sha256: req.context_sha256().unwrap(),
        observed: observed.observed,
    };
    reply.validate_for(&req, &req.instance, false).unwrap();
    assert!(reply.validate_for(&req, &req.instance, true).is_err());
    let mut replay = reply.clone();
    replay.disposition = D::Replayed;
    replay.observed.issuer_floor_epoch = DsgUint64::new(3);
    replay.observed.capacity_generation = DsgUint64::new(3);
    replay.validate_for(&req, &req.instance, true).unwrap();
    assert!(replay.validate_for(&req, &req.instance, false).is_err());
    let mut bad = reply.clone();
    bad.context_sha256 = "0".repeat(64);
    assert!(bad.validate_for(&req, &req.instance, false).is_err());
    let mut bad = reply.clone();
    bad.vm_shrink_permit = true;
    assert!(bad.validate_for(&req, &req.instance, false).is_err());
    let mut bad = reply.clone();
    bad.disposition = D::NotFound;
    assert!(bad.validate_for(&req, &req.instance, true).is_err());
}
