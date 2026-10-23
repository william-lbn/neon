use compute_api::dsg_control::DsgUint64;
use compute_api::dsg_state::{DsgControlResponse, DsgSharedCostResponse};

#[test]
fn shared_cost_requires_same_snapshot_and_checked_deduplicated_totals() {
    let baseline: DsgControlResponse =
        serde_json::from_str(include_str!("../testdata/dsg_response_v1.json")).unwrap();
    let startup = baseline.observed.startup_shared_upper_bytes.get();
    let main = startup / 2;
    let arrays: u64 = baseline
        .observed
        .arrays
        .iter()
        .map(|a| a.committed_upper_bytes.get())
        .sum();
    let fact = DsgSharedCostResponse {
        protocol_version: 1,
        vm_shrink_permit: false,
        startup_main_upper_bytes: DsgUint64::new(main),
        startup_arrays_upper_bytes: DsgUint64::new(startup - main),
        current_main_and_arrays_upper_bytes: DsgUint64::new(main + arrays),
        observed: baseline.observed,
    };
    fact.validate_for(&fact.observed.instance).unwrap();
    let mut duplicate = fact.clone();
    duplicate.current_main_and_arrays_upper_bytes = DsgUint64::new(startup + arrays);
    assert!(duplicate.validate_for(&fact.observed.instance).is_err());
    let mut overflow = fact.clone();
    overflow.observed.arrays[0].committed_upper_bytes = DsgUint64::new(u64::MAX);
    assert!(overflow.validate_for(&fact.observed.instance).is_err());
    let mut permission = fact.clone();
    permission.vm_shrink_permit = true;
    assert!(permission.validate_for(&fact.observed.instance).is_err());
    let raw = serde_json::to_string(&fact).unwrap();
    for bad in [
        raw.replace("\"vm_shrink_permit\":false", "\"vm_shrink_permit\":null"),
        raw.replace(
            "\"vm_shrink_permit\":false",
            "\"vm_shrink_permit\":false,\"vm_shrink_permit\":true",
        ),
    ] {
        assert!(serde_json::from_str::<DsgSharedCostResponse>(&bad).is_err());
    }
}
