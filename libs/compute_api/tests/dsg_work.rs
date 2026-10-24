use compute_api::{dsg_state::DsgControlResponse, dsg_work::DsgWorkResponse};

#[test]
fn work_snapshot_requires_complete_counts_same_boot_and_closed_idle() {
    let baseline: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    let mut fact = DsgWorkResponse {
        protocol_version: 1,
        work_api_version: 1,
        vm_shrink_permit: false,
        client_processes: 2,
        startup_clients: 0,
        idle_clients: 1,
        frontend_reads: 0,
        frontend_executes: 0,
        bounded_inputs: 1,
        retained_transactions: 0,
        background_processes: 2,
        auxiliary_processes: 3,
        observer_excluded: true,
        foreground_idle: true,
        observed: baseline.observed,
    };
    fact.observed.admission_open = false;
    let bound = fact.observed.instance.clone();
    fact.validate_for(&bound).unwrap();
    for mutate in [
        |r: &mut DsgWorkResponse| r.vm_shrink_permit = true,
        |r: &mut DsgWorkResponse| r.work_api_version = 2,
        |r: &mut DsgWorkResponse| r.client_processes = 0,
        |r: &mut DsgWorkResponse| r.startup_clients = u32::MAX,
        |r: &mut DsgWorkResponse| r.retained_transactions = 3,
        |r: &mut DsgWorkResponse| r.retained_transactions = 1,
        |r: &mut DsgWorkResponse| r.observed.admission_open = true,
        |r: &mut DsgWorkResponse| r.observed.instance.compute_instance_id = "other".into(),
    ] {
        let mut bad = fact.clone();
        mutate(&mut bad);
        assert!(bad.validate_for(&bound).is_err());
    }
    let raw = serde_json::to_value(&fact).unwrap();
    for key in [
        "work_api_version",
        "foreground_idle",
        "retained_transactions",
        "observer_excluded",
    ] {
        let mut missing = raw.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<DsgWorkResponse>(missing).is_err());
        let mut null = raw.clone();
        null[key] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DsgWorkResponse>(null).is_err());
    }
    let mut duplicate = serde_json::to_string(&fact).unwrap();
    duplicate = duplicate.replacen(
        "\"work_api_version\":1",
        "\"work_api_version\":1,\"work_api_version\":1",
        1,
    );
    assert!(serde_json::from_str::<DsgWorkResponse>(&duplicate).is_err());
}
