use compute_api::{dsg_control::DsgUint64, dsg_dsm::DsgDsmResponse, dsg_state::DsgControlResponse};
use serde_json::json;

fn fact() -> DsgDsmResponse {
    let mut base: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    base.observed.admission_open = false;
    // 分层构造同一fixture，避免巨型json!宏递归；不提高编译器递归限额掩盖结构。
    let work = json!({"protocol_version":1,"work_api_version":2,"vm_shrink_permit":false,
        "client_processes":0,"startup_clients":0,"idle_clients":0,"frontend_reads":0,"frontend_executes":0,
        "bounded_inputs":0,"retained_transactions":0,"background_processes":1,"auxiliary_processes":0,
        "observer_excluded":true,"foreground_idle":true,"observed":base.observed});
    let prepared = json!({"protocol_version":1,"prepared_api_version":1,"vm_shrink_permit":false,
        "prepared_slots":1,"maximum_prepared_slots":4,"runtime_prepares_blocked":true,
        "connections":{"protocol_version":1,"connection_api_version":1,"vm_shrink_permit":false,
        "enabled":true,"requested_serial":3,"applied_serial":3,"requested_open":false,"applied_open":false,
        "acknowledged":true,"private_management_ready":true,"public_connections_blocked":true,"work":work}});
    serde_json::from_value(json!({"protocol_version":1,"dsm_api_version":1,"vm_shrink_permit":false,
        "current_boot_only":true,"reserved_upper_bytes":"32768","control_upper_bytes":"16384",
        "external_budget_bytes":"65536","external_segments":2,"pending_creations":1,"unknown_segments":1,
        "page_bytes":16384,"limit_enforced":true,"accounting_valid":true,"prepared":prepared})).unwrap()
}

#[test]
fn dsm_reservations_keep_unknown_and_unenforced_meaning_with_strict_binding() {
    let fact = fact();
    let bound = fact.prepared.connections.work.observed.instance.clone();
    fact.validate_for(&bound).unwrap();
    for mutate in [
        |r: &mut DsgDsmResponse| r.vm_shrink_permit = true,
        |r: &mut DsgDsmResponse| r.dsm_api_version = 0,
        |r: &mut DsgDsmResponse| r.current_boot_only = false,
        |r: &mut DsgDsmResponse| r.accounting_valid = false,
        |r: &mut DsgDsmResponse| r.page_bytes = 0,
        |r: &mut DsgDsmResponse| r.page_bytes = 12288,
        |r: &mut DsgDsmResponse| r.control_upper_bytes = DsgUint64::new(0),
        |r: &mut DsgDsmResponse| r.reserved_upper_bytes = DsgUint64::new(16384),
        |r: &mut DsgDsmResponse| r.pending_creations = u32::MAX,
        |r: &mut DsgDsmResponse| r.unknown_segments = 2,
        |r: &mut DsgDsmResponse| r.limit_enforced = false,
        |r: &mut DsgDsmResponse| r.external_budget_bytes = DsgUint64::new(16384),
        |r: &mut DsgDsmResponse| {
            r.prepared.connections.work.observed.instance.vm_uid = "foreign".into()
        },
    ] {
        let mut bad = fact.clone();
        mutate(&mut bad);
        assert!(bad.validate_for(&bound).is_err());
    }
    let mut default = fact.clone();
    default.limit_enforced = false;
    default.external_budget_bytes = DsgUint64::new(0);
    default.validate_for(&bound).unwrap();
    let mut empty = default.clone();
    empty.external_segments = 0;
    empty.pending_creations = 0;
    empty.unknown_segments = 0;
    empty.reserved_upper_bytes = DsgUint64::new(0);
    empty.validate_for(&bound).unwrap();
    let value = serde_json::to_value(&fact).unwrap();
    for key in value.as_object().unwrap().keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<DsgDsmResponse>(missing).is_err(),
            "{key}"
        );
        let mut null = value.clone();
        null[key] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<DsgDsmResponse>(null).is_err(),
            "{key}"
        );
    }
    let raw = serde_json::to_string(&fact).unwrap();
    for bad in [
        raw.replacen(
            "\"page_bytes\":16384",
            "\"page_bytes\":16384,\"page_bytes\":16384",
            1,
        ),
        raw.replacen(
            "\"reserved_upper_bytes\":\"32768\"",
            "\"reserved_upper_bytes\":32768",
            1,
        ),
        raw.replacen(
            "\"reserved_upper_bytes\":\"32768\"",
            "\"reserved_upper_bytes\":\"032768\"",
            1,
        ),
    ] {
        assert!(serde_json::from_str::<DsgDsmResponse>(&bad).is_err());
    }
}
