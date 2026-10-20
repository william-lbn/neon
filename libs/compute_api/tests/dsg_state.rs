//! 跨字段协议校验单元测试；人工快照只测解码/关联规则，不冒充物理或集群执行。
use compute_api::dsg_control::{DsgResizeRequest, DsgUint64};
use compute_api::dsg_state::*;
use serde_json::json;

fn request() -> DsgResizeRequest {
    serde_json::from_str(include_str!("../testdata/dsg_request_v1.json")).unwrap()
}

fn snapshot(request: &DsgResizeRequest, generation: u64, high: u32) -> DsgSnapshot {
    let arrays = [
        DsgArrayKind::Descriptors,
        DsgArrayKind::Blocks,
        DsgArrayKind::Iocv,
        DsgArrayKind::Checkpoint,
    ]
    .into_iter()
    .map(|array| DsgArrayFact {
        array,
        committed_upper_bytes: DsgUint64::new(16 * 1024 * 1024),
        allocation_unknown: false,
        release_state: DsgReleaseState::None,
        release_key: None,
        release_capacity_generation: DsgUint64::new(0),
        provider: DsgProviderFact {
            has_result: false,
            status: DsgProviderStatus::Unknown,
            system_errno: 0,
            requested_offset: DsgUint64::new(0),
            requested_bytes: DsgUint64::new(0),
            affected_offset: DsgUint64::new(0),
            affected_bytes: DsgUint64::new(0),
            physical_release_verified: false,
            allocation_may_be_partial: false,
            release_may_be_partial: false,
        },
    })
    .collect();
    DsgSnapshot {
        instance: request.key.instance.clone(),
        capacity_generation: DsgUint64::new(generation),
        low_buffers: high,
        high_buffers: high,
        maximum_buffers: 8192,
        admission_open: false,
        admission_revision: DsgUint64::new(0),
        startup_shared_upper_bytes: DsgUint64::new(32 * 1024 * 1024),
        arrays,
        operation: Some(DsgOperationFact {
            key: request.key.clone(),
            kind: request.kind,
            phase: DsgPhase::Committed,
            reason: DsgReason::None,
            target_buffers: request.target_buffers,
            expected_capacity_generation: request.expected_capacity_generation,
            context_sha256: "a".repeat(64),
            cancel_requested: false,
            cancel_closed: true,
            replayed: true,
        }),
    }
}

fn response() -> DsgControlResponse {
    let request = request();
    let original = snapshot(&request, 2, 1024);
    let mut next = request.clone();
    next.key.revision = DsgUint64::new(next.key.revision.get() + 1);
    next.key.operation_id.low = DsgUint64::new(7);
    next.target_buffers = 2048;
    next.expected_capacity_generation = DsgUint64::new(3);
    DsgControlResponse {
        protocol_version: 1,
        vm_shrink_permit: false,
        receipt: DsgReceipt {
            key: request.key.clone(),
            disposition: DsgDisposition::Replayed,
            has_result: true,
            result: Some(original),
        },
        observed: snapshot(&next, 4, 2048),
    }
}

#[test]
fn historical_receipt_and_new_observed_state_are_both_valid() {
    let request = request();
    let response = response();
    response
        .validate_resize(&request, &request.key.instance)
        .unwrap();
    assert_ne!(
        response
            .receipt
            .result
            .as_ref()
            .unwrap()
            .operation
            .as_ref()
            .unwrap()
            .key,
        response.observed.operation.as_ref().unwrap().key
    );
    let encoded = serde_json::to_string(&response).unwrap();
    assert_eq!(
        serde_json::from_str::<DsgControlResponse>(&encoded).unwrap(),
        response
    );
}

#[test]
fn wrong_receipt_key_result_identity_or_boot_is_rejected() {
    let request = request();
    for change in 0..5 {
        let mut response = response();
        match change {
            0 => response.receipt.key.revision = DsgUint64::new(1),
            1 => {
                response
                    .receipt
                    .result
                    .as_mut()
                    .unwrap()
                    .operation
                    .as_mut()
                    .unwrap()
                    .key
                    .revision = DsgUint64::new(1)
            }
            2 => response.observed.instance.pg_boot_id = DsgUint64::new(1),
            3 => response.receipt.has_result = false,
            _ => response.vm_shrink_permit = true,
        }
        assert!(
            response
                .validate_resize(&request, &request.key.instance)
                .is_err(),
            "change={change}"
        );
    }
}

#[test]
fn busy_cannot_borrow_current_operations_success_result() {
    let request = request();
    let mut response = response();
    response.receipt.disposition = DsgDisposition::Busy;
    response.receipt.has_result = false;
    response.receipt.result = None;
    response
        .validate_for(&request.key, &request.key.instance)
        .unwrap();
    response.receipt.result = Some(response.observed.clone());
    assert!(
        response
            .validate_for(&request.key, &request.key.instance)
            .is_err()
    );
}

#[test]
fn previous_verified_release_does_not_hide_new_grow_unknown_cost() {
    let request = request();
    let mut snapshot = response().observed;
    let array = &mut snapshot.arrays[0];
    array.release_state = DsgReleaseState::Verified;
    array.release_key = Some(request.key);
    array.release_capacity_generation = DsgUint64::new(2);
    array.allocation_unknown = true;
    array.provider.has_result = true;
    array.provider.status = DsgProviderStatus::Ok;
    array.provider.physical_release_verified = true;
    array.provider.affected_bytes = DsgUint64::new(4096);
    snapshot.validate().unwrap();
    // 只验证两组独立事实共存，不产生当前VM回收证明。
}

#[test]
fn pending_provider_cannot_claim_completed_or_physical_success() {
    for change in 0..3 {
        let mut snapshot = response().observed;
        let array = &mut snapshot.arrays[0];
        array.release_state = DsgReleaseState::Pending;
        array.release_key = Some(request().key);
        array.release_capacity_generation = DsgUint64::new(2);
        match change {
            0 => array.provider.has_result = true,
            1 => array.provider.status = DsgProviderStatus::Ok,
            _ => array.provider.physical_release_verified = true,
        }
        assert!(snapshot.validate().is_err());
    }
}

#[test]
fn malformed_capacity_arrays_and_operation_are_rejected() {
    for change in 0..8 {
        let mut snapshot = response().observed;
        match change {
            0 => snapshot.low_buffers = snapshot.maximum_buffers + 1,
            1 => snapshot.arrays[1].array = snapshot.arrays[0].array,
            2 => {
                snapshot.arrays.pop();
            }
            3 => snapshot.operation.as_mut().unwrap().context_sha256 = "A".repeat(64),
            4 => snapshot.operation.as_mut().unwrap().cancel_requested = true,
            5 => snapshot.capacity_generation = DsgUint64::new(u64::MAX),
            6 => snapshot.low_buffers = 256,
            _ => {
                snapshot.arrays[0].provider.affected_offset = DsgUint64::new(u64::MAX);
                snapshot.arrays[0].provider.affected_bytes = DsgUint64::new(1);
            }
        }
        assert!(snapshot.validate().is_err(), "change={change}");
    }
}

#[test]
fn response_schema_rejects_duplicate_unknown_and_lossy_numbers() {
    let raw = serde_json::to_string(&response()).unwrap();
    let duplicate = raw.replacen(
        "\"protocol_version\":1",
        "\"protocol_version\":1,\"protocol_version\":1",
        1,
    );
    assert!(serde_json::from_str::<DsgControlResponse>(&duplicate).is_err());
    let mut value = serde_json::to_value(response()).unwrap();
    value["observed"]["capacity_generation"] = json!(4);
    assert!(serde_json::from_value::<DsgControlResponse>(value).is_err());
    let mut value = serde_json::to_value(response()).unwrap();
    value["receipt"]["unknown"] = json!(false);
    assert!(serde_json::from_value::<DsgControlResponse>(value).is_err());
}

#[test]
fn nullable_fields_must_be_present_instead_of_silently_defaulted() {
    for location in 0..3 {
        let mut value = serde_json::to_value(response()).unwrap();
        match location {
            0 => {
                value["receipt"].as_object_mut().unwrap().remove("result");
            }
            1 => {
                value["observed"]
                    .as_object_mut()
                    .unwrap()
                    .remove("operation");
            }
            _ => {
                value["observed"]["arrays"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("release_key");
            }
        }
        assert!(serde_json::from_value::<DsgControlResponse>(value).is_err());
    }
}
