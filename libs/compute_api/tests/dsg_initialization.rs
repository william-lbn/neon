use compute_api::dsg_initialization::DsgInitializationResponse;
use compute_api::dsg_state::DsgControlResponse;
use compute_api::dynamic_shared_buffers::DynamicSharedBuffersSpec;

fn fixture() -> DsgInitializationResponse {
    let observed = serde_json::from_str::<DsgControlResponse>(include_str!(
        "../testdata/dsg_response_v1.json"
    ))
    .unwrap()
    .observed;
    DsgInitializationResponse {
        protocol_version: 1,
        vm_shrink_permit: false,
        operation_uuid: "04f1f1cd-e7d3-4f0b-a177-baba188d3c45".into(),
        tenant_id: "a".repeat(32),
        timeline_id: "b".repeat(32),
        startup: DynamicSharedBuffersSpec {
            protocol_version: 2,
            vm_uid: observed.instance.vm_uid.clone(),
            compute_instance_id: observed.instance.compute_instance_id.clone(),
            bootstrap_buffer_bytes: 2 << 20,
            maximum_buffer_bytes: 64 << 20,
            minimum_guest_memory_bytes: 128 << 20,
            non_buffer_reserve_bytes: 64 << 20,
        },
        authentication_policy_verified: true,
        ordinary_roles_verified: true,
        observed,
    }
}

#[test]
fn initialization_is_bound_to_original_plan_without_granting_admission() {
    let good = fixture();
    good.validate_for(
        &good.startup,
        &good.operation_uuid,
        &good.tenant_id,
        &good.timeline_id,
    )
    .unwrap();
    assert!(!good.observed.admission_open);
    for mutate in [
        |v: &mut DsgInitializationResponse| v.vm_shrink_permit = true,
        |v: &mut DsgInitializationResponse| v.protocol_version = 0,
        |v: &mut DsgInitializationResponse| v.authentication_policy_verified = false,
        |v: &mut DsgInitializationResponse| v.ordinary_roles_verified = false,
        |v: &mut DsgInitializationResponse| v.operation_uuid = "other".into(),
        |v: &mut DsgInitializationResponse| v.tenant_id = "c".repeat(32),
        |v: &mut DsgInitializationResponse| v.timeline_id = "c".repeat(32),
        |v: &mut DsgInitializationResponse| v.startup.non_buffer_reserve_bytes += 1,
        |v: &mut DsgInitializationResponse| v.observed.instance.vm_uid = "other".into(),
        |v: &mut DsgInitializationResponse| v.observed.maximum_buffers -= 1,
        |v: &mut DsgInitializationResponse| v.observed.low_buffers = 16,
        |v: &mut DsgInitializationResponse| {
            v.observed.instance.pg_boot_id = compute_api::dsg_control::DsgUint64::new(0)
        },
    ] {
        let mut bad = good.clone();
        mutate(&mut bad);
        assert!(
            bad.validate_for(
                &good.startup,
                &good.operation_uuid,
                &good.tenant_id,
                &good.timeline_id
            )
            .is_err()
        );
    }
}

#[test]
fn initialization_requires_all_proof_fields_and_canonical_boot_numbers() {
    let good = serde_json::to_value(fixture()).unwrap();
    for field in [
        "authentication_policy_verified",
        "ordinary_roles_verified",
        "operation_uuid",
        "startup",
        "observed",
        "vm_shrink_permit",
    ] {
        let mut bad = good.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<DsgInitializationResponse>(bad).is_err());
    }
    let mut bad = good.clone();
    bad["observed"]["instance"]["pg_boot_id"] = serde_json::json!(1);
    assert!(serde_json::from_value::<DsgInitializationResponse>(bad).is_err());
    let mut bad = good;
    bad["password"] = serde_json::json!("not allowed");
    assert!(serde_json::from_value::<DsgInitializationResponse>(bad).is_err());
}
