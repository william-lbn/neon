use compute_api::dsg_admission_lease::*;
use compute_api::dsg_control::DsgUint64;
use compute_api::dsg_state::DsgControlResponse;

#[test]
fn renewal_proof_requires_exact_native_context() {
    let base: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    let mut s = base.observed;
    s.operation = None;
    s.issuer_floor_epoch = DsgUint64::new(2);
    s.admission_revision = DsgUint64::new(1);
    s.admission_open = true;
    let request = DsgAdmissionLeaseRequest {
        protocol_version: 1,
        instance: s.instance.clone(),
        issuer_epoch: DsgUint64::new(2),
        expected_capacity_generation: s.capacity_generation,
        expected_admission_revision: s.admission_revision,
        expected_deadline_ns: DsgUint64::new(100),
        action: DsgAdmissionLeaseAction::Renew,
    };
    let mut fact = DsgAdmissionLeaseResponse {
        protocol_version: 1,
        vm_shrink_permit: false,
        request: request.clone(),
        renewed: true,
        armed: true,
        expired: false,
        deadline_ns: DsgUint64::new(200),
        lease_epoch: DsgUint64::new(2),
        timeout_ms: 30000,
        observed: s,
    };
    fact.validate_for(&request, &request.instance).unwrap();
    let value = serde_json::to_value(&fact).unwrap();
    for (field, v) in [
        ("deadline_ns", serde_json::json!("100")),
        ("expired", serde_json::json!(true)),
        ("armed", serde_json::json!(false)),
        ("vm_shrink_permit", serde_json::json!(true)),
        ("lease_epoch", serde_json::json!("3")),
        ("timeout_ms", serde_json::json!(0)),
    ] {
        let mut bad = value.clone();
        bad[field] = v;
        let parsed: DsgAdmissionLeaseResponse = serde_json::from_value(bad).unwrap();
        assert!(
            parsed.validate_for(&request, &request.instance).is_err(),
            "{field}"
        );
    }
    fact.request.action = DsgAdmissionLeaseAction::Rearm;
    assert!(fact.validate_for(&fact.request, &request.instance).is_err());
    fact.observed.admission_open = false;
    fact.validate_for(&fact.request, &request.instance).unwrap();
    let raw = serde_json::to_string(&request).unwrap();
    for bad in [
        raw.replace(
            "\"action\":\"renew\"",
            "\"action\":\"renew\",\"action\":\"renew\"",
        ),
        raw.replace(
            "\"expected_deadline_ns\":\"100\"",
            "\"expected_deadline_ns\":100",
        ),
        raw.replace("\"expected_deadline_ns\":\"100\",", ""),
    ] {
        assert!(serde_json::from_str::<DsgAdmissionLeaseRequest>(&bad).is_err());
    }
}

#[test]
fn rearm_history_preserves_expiry_and_unknown_without_current_service_authority() {
    let base: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    let mut s = base.observed;
    s.operation = None;
    s.issuer_floor_epoch = DsgUint64::new(3);
    s.admission_revision = DsgUint64::new(1);
    s.admission_open = false;
    let request = DsgAdmissionLeaseRequest {
        protocol_version: 1,
        instance: s.instance.clone(),
        issuer_epoch: DsgUint64::new(2),
        expected_capacity_generation: s.capacity_generation,
        expected_admission_revision: s.admission_revision,
        expected_deadline_ns: DsgUint64::new(100),
        action: DsgAdmissionLeaseAction::Rearm,
    };
    let fact = DsgAdmissionRearmStatus {
        protocol_version: 1,
        vm_shrink_permit: false,
        request: request.clone(),
        retained: true,
        applied_deadline_ns: DsgUint64::new(200),
        current: DsgAdmissionLeaseResponse {
            protocol_version: 1,
            vm_shrink_permit: false,
            request: request.clone(),
            renewed: false,
            armed: true,
            expired: true,
            deadline_ns: DsgUint64::new(300),
            lease_epoch: DsgUint64::new(2),
            timeout_ms: 30000,
            observed: s,
        },
    };
    fact.validate_for(&request, &request.instance).unwrap();
    let value = serde_json::to_value(&fact).unwrap();
    for (path, bad_value) in [
        ("/vm_shrink_permit", serde_json::json!(true)),
        ("/applied_deadline_ns", serde_json::json!("100")),
        ("/applied_deadline_ns", serde_json::json!("301")),
        ("/retained", serde_json::json!(false)),
        ("/current/renewed", serde_json::json!(true)),
        ("/current/armed", serde_json::json!(false)),
        ("/current/lease_epoch", serde_json::json!("1")),
        (
            "/current/observed/issuer_floor_epoch",
            serde_json::json!("1"),
        ),
        (
            "/current/observed/admission_revision",
            serde_json::json!("0"),
        ),
        ("/current/observed/admission_open", serde_json::json!(true)),
        ("/request/expected_deadline_ns", serde_json::json!("101")),
        (
            "/current/request/expected_deadline_ns",
            serde_json::json!("101"),
        ),
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(path).unwrap() = bad_value;
        let parsed: DsgAdmissionRearmStatus = serde_json::from_value(bad).unwrap();
        assert!(
            parsed.validate_for(&request, &request.instance).is_err(),
            "{path}"
        );
    }
    let mut unknown = fact;
    unknown.retained = false;
    unknown.applied_deadline_ns = DsgUint64::new(0);
    unknown.validate_for(&request, &request.instance).unwrap();
    let raw = serde_json::to_string(&unknown).unwrap();
    for bad in [
        raw.replace(
            "\"retained\":false",
            "\"retained\":false,\"retained\":false",
        ),
        raw.replace("\"applied_deadline_ns\":\"0\"", "\"applied_deadline_ns\":0"),
        raw.replacen("\"request\":", "\"extra\":true,\"request\":", 1),
    ] {
        assert!(serde_json::from_str::<DsgAdmissionRearmStatus>(&bad).is_err());
    }
}
