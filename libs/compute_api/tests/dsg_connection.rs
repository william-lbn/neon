use compute_api::{
    dsg_connection::DsgConnectionResponse, dsg_state::DsgControlResponse, dsg_work::DsgWorkResponse,
};

#[test]
fn connection_ack_requires_same_native_state_and_mode_even_at_saturation() {
    let base: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    let mut work = DsgWorkResponse {
        protocol_version: 1,
        work_api_version: 2,
        vm_shrink_permit: false,
        client_processes: 0,
        startup_clients: 0,
        idle_clients: 0,
        frontend_reads: 0,
        frontend_executes: 0,
        bounded_inputs: 0,
        retained_transactions: 0,
        background_processes: 0,
        auxiliary_processes: 0,
        observer_excluded: true,
        foreground_idle: true,
        observed: base.observed,
    };
    work.observed.admission_open = false;
    let fact = DsgConnectionResponse {
        protocol_version: 1,
        connection_api_version: 1,
        vm_shrink_permit: false,
        enabled: true,
        requested_serial: 3,
        applied_serial: 3,
        requested_open: false,
        applied_open: false,
        acknowledged: true,
        private_management_ready: true,
        public_connections_blocked: true,
        work,
    };
    let bound = fact.work.observed.instance.clone();
    fact.validate_for(&bound).unwrap();
    for change in [
        |r: &mut DsgConnectionResponse| r.vm_shrink_permit = true,
        |r: &mut DsgConnectionResponse| r.applied_serial = 4,
        |r: &mut DsgConnectionResponse| r.applied_serial = 2,
        |r: &mut DsgConnectionResponse| r.applied_open = true,
        |r: &mut DsgConnectionResponse| r.requested_open = true,
        |r: &mut DsgConnectionResponse| r.requested_serial = u32::MAX,
        |r: &mut DsgConnectionResponse| r.enabled = false,
        |r: &mut DsgConnectionResponse| r.work.observed.instance.vm_uid = "other".into(),
    ] {
        let mut bad = fact.clone();
        change(&mut bad);
        assert!(bad.validate_for(&bound).is_err());
    }
    let mut pending = fact.clone();
    pending.applied_serial = 2;
    pending.applied_open = true;
    pending.acknowledged = false;
    pending.public_connections_blocked = false;
    pending.validate_for(&bound).unwrap();
    pending.requested_serial = u32::MAX >> 1;
    pending.applied_serial = pending.requested_serial;
    pending.validate_for(&bound).unwrap();
    pending.acknowledged = true;
    assert!(pending.validate_for(&bound).is_err());
    let raw = serde_json::to_value(&fact).unwrap();
    for key in [
        "enabled",
        "applied_serial",
        "acknowledged",
        "private_management_ready",
        "work",
    ] {
        let mut missing = raw.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<DsgConnectionResponse>(missing).is_err());
        let mut null = raw.clone();
        null[key] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DsgConnectionResponse>(null).is_err());
    }
    let raw = serde_json::to_string(&fact).unwrap().replacen(
        "\"acknowledged\":true",
        "\"acknowledged\":true,\"acknowledged\":true",
        1,
    );
    assert!(serde_json::from_str::<DsgConnectionResponse>(&raw).is_err());
}
