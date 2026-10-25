use compute_api::{
    dsg_connection::DsgConnectionResponse, dsg_prepared::DsgPreparedResponse,
    dsg_state::DsgControlResponse, dsg_work::DsgWorkResponse,
};

#[test]
fn prepared_slots_are_not_inferred_from_frontend_idle_and_envelope_is_strict() {
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
    let fact = DsgPreparedResponse {
        protocol_version: 1,
        prepared_api_version: 1,
        vm_shrink_permit: false,
        prepared_slots: 1,
        maximum_prepared_slots: 4,
        runtime_prepares_blocked: true,
        connections: DsgConnectionResponse {
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
        },
    };
    let bound = fact.connections.work.observed.instance.clone();
    fact.validate_for(&bound).unwrap();
    for mutate in [
        |r: &mut DsgPreparedResponse| r.prepared_slots = 5,
        |r: &mut DsgPreparedResponse| r.maximum_prepared_slots = 0,
        |r: &mut DsgPreparedResponse| r.runtime_prepares_blocked = false,
        |r: &mut DsgPreparedResponse| r.vm_shrink_permit = true,
        |r: &mut DsgPreparedResponse| r.prepared_api_version = 0,
        |r: &mut DsgPreparedResponse| {
            r.connections.work.observed.instance.vm_uid = "foreign".into()
        },
    ] {
        let mut bad = fact.clone();
        mutate(&mut bad);
        assert!(bad.validate_for(&bound).is_err());
    }
    let mut empty = fact.clone();
    empty.prepared_slots = 0;
    empty.maximum_prepared_slots = 0;
    empty.validate_for(&bound).unwrap();
    let value = serde_json::to_value(&fact).unwrap();
    for key in [
        "prepared_slots",
        "maximum_prepared_slots",
        "runtime_prepares_blocked",
        "connections",
    ] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<DsgPreparedResponse>(missing).is_err());
        let mut null = value.clone();
        null[key] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DsgPreparedResponse>(null).is_err());
    }
    let raw = serde_json::to_string(&fact).unwrap().replacen(
        "\"prepared_slots\":1",
        "\"prepared_slots\":1,\"prepared_slots\":1",
        1,
    );
    assert!(serde_json::from_str::<DsgPreparedResponse>(&raw).is_err());
}
