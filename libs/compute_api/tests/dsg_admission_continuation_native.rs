//! 本机隔离PG真实执行；外层WAL以显式恢复输入构造，不冒充Agent完整接管或Linux验收。
#![cfg(all(feature = "postgres-client", unix))]
use anyhow::Result;
use compute_api::dsg_admission::{
    DsgAdmissionContinuationRequest as Request, DsgAdmissionDisposition as D, DsgAdmissionRequest,
};
use compute_api::dsg_client::DsgSqlClient;
use compute_api::dsg_control::DsgUint64;
use std::time::Duration;
#[path = "support/dsg_cluster.rs"]
mod dsg_cluster;
use dsg_cluster::{COMPUTE, Cluster, VM};

#[tokio::test]
#[ignore = "strict gate creates a real isolated Neon PG cluster"]
async fn continuation_native_unreceived_applied_conflict_and_repeated_owners() -> Result<()> {
    let mut cluster = Cluster::new()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    execution
        .batch_execute("ALTER EXTENSION neon UPDATE TO '1.10'")
        .await?;
    let client =
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, Duration::from_secs(5)).await?;
    let original = DsgAdmissionRequest {
        protocol_version: 1,
        instance: client.instance().clone(),
        issuer_epoch: DsgUint64::new(1),
        revision: DsgUint64::new(1),
        expected_capacity_generation: DsgUint64::new(1),
        open: false,
        budget_commitment_id: "native-continuation-budget".into(),
    };
    let mut first = Request {
        protocol_version: 1,
        original: original.clone(),
        previous_close: original.clone(),
        close: original.clone(),
    };
    first.close.issuer_epoch = DsgUint64::new(2);
    client.restore_admission_pending(&original, None)?;
    let unreceived = client.admission_status(&original).await?;
    assert_eq!(unreceived.disposition, D::NotFound);
    assert_eq!(client.pending_admission()?.0, Some(original.clone()));
    let proof = client.continue_admission_closure(&first).await?;
    assert_eq!(proof.closed_by.disposition, D::Applied);
    assert!(proof.prior_may_have_applied && !proof.vm_shrink_permit);
    assert!(client.pending_admission()?.0.is_none());
    // 明确模拟第二owner写前后、尚未派发即退出。第三owner不得换写第二owner，
    // 保留最初与前驱，用新epoch和同一未消费global revision完成实际关闭。
    let mut second = first.clone();
    second.previous_close = first.close.clone();
    second.close.issuer_epoch = DsgUint64::new(3);
    second.close.revision = DsgUint64::new(2);
    client.restore_admission_continuation(&second)?;
    assert!(client.admission_continuation_status(&second).await.is_err());
    assert_eq!(
        client.pending_admission_continuation()?,
        Some(second.clone())
    );
    let mut third = second.clone();
    third.previous_close = second.close.clone();
    third.close.issuer_epoch = DsgUint64::new(4);
    let proof = client.continue_admission_closure(&third).await?;
    assert_eq!(proof.closed_by.disposition, D::Applied);
    assert_eq!(proof.closed_by.observed.admission_revision.get(), 2);
    assert_eq!(proof.closed_by.observed.issuer_floor_epoch.get(), 4);
    // 外层未收到成功时，新SDK必须恢复完整三份意图，仅查询原回执即可解除本地
    // 栅栏。没有重发setter，也没有按closed布尔补成功；旧owner迟到写由真实PG拒绝。
    let recovered =
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, Duration::from_secs(5)).await?;
    recovered.restore_admission_continuation(&third)?;
    assert!(
        recovered
            .restore_admission_pending(&original, Some(&third.close))
            .is_err()
    );
    assert!(
        recovered
            .admission_closure_status(&original, &third.close)
            .await
            .is_err()
    );
    assert_eq!(
        recovered.pending_admission_continuation()?,
        Some(third.clone())
    );
    let proof = recovered.admission_continuation_status(&third).await?;
    assert_eq!(proof.closed_by.disposition, D::Replayed);
    assert!(recovered.pending_admission_continuation()?.is_none());
    assert_eq!(
        client.set_admission(&second.close).await?.disposition,
        D::Stale
    );
    // 第二owner迟到由原生floor拒绝，回执不能据此解除新选择。另一个
    // epoch4已生效的回执，epoch5误选同revision必须留未知，显式epoch6+next才完成。
    let mut conflicting = third.clone();
    conflicting.previous_close = third.close.clone();
    conflicting.close.issuer_epoch = DsgUint64::new(5);
    recovered.restore_admission_continuation(&third)?;
    assert!(
        recovered
            .continue_admission_closure(&conflicting)
            .await
            .is_err()
    );
    assert_eq!(
        recovered.pending_admission_continuation()?,
        Some(conflicting.clone())
    );
    assert!(
        recovered
            .admission_continuation_status(&third)
            .await
            .is_err()
    );
    let mut final_close = conflicting.clone();
    final_close.previous_close = conflicting.close.clone();
    final_close.close.issuer_epoch = DsgUint64::new(6);
    final_close.close.revision = DsgUint64::new(3);
    let proof = recovered.continue_admission_closure(&final_close).await?;
    assert_eq!(proof.closed_by.disposition, D::Applied);
    assert!(!proof.closed_by.observed.admission_open);
    assert_eq!(proof.closed_by.observed.capacity_generation.get(), 1);
    assert!(matches!(
        recovered.continue_admission_closure(&first).await,
        Err(compute_api::dsg_client::DsgAdmissionCallError::NotSent(_))
    ));
    assert!(
        recovered.pending_admission()?.0.is_none(),
        "迟到owner不得凭Stale占住空SDK"
    );
    // 已有同一旧选择的未知则必须逐字保留，Stale只回退本次候选，不清旧未知。
    recovered.restore_admission_continuation(&first)?;
    assert!(recovered.continue_admission_closure(&first).await.is_err());
    assert_eq!(
        recovered.pending_admission_continuation()?,
        Some(first.clone())
    );
    // 后续已应用的完整选择不能越过不同旧原始未知；本例Original完全相同，
    // 显式链通过更高epoch接续，不能借一次本地清空实现恢复。
    let mut final_receipt = final_close.clone();
    final_receipt.previous_close = first.close.clone();
    assert!(
        recovered
            .admission_continuation_status(&final_receipt)
            .await
            .is_err()
    );
    // JSONB/SHA在真实PG上独立核对完整选择摘要，避免两份手写canonical互证。
    let raw = serde_json::to_string(&final_close)?;
    let digest: String = observer
        .query_one(
            "SELECT encode(sha256(($1::text::jsonb)::text::bytea),'hex')",
            &[&raw],
        )
        .await?
        .get(0);
    assert_eq!(final_close.context_sha256()?, digest);
    for vector in serde_json::from_str::<Vec<serde_json::Value>>(include_str!(
        "../testdata/dsg_admission_continuation_v1.json"
    ))? {
        if vector["valid"] == true {
            let raw = serde_json::to_string(&vector["request"])?;
            let digest: String = observer
                .query_one(
                    "SELECT encode(sha256(($1::text::jsonb)::text::bytea),'hex')",
                    &[&raw],
                )
                .await?
                .get(0);
            assert_eq!(Some(digest.as_str()), vector["context_sha256"].as_str());
        }
    }
    // 真正重启Postmaster后不允许把同VM的新boot改绑进原关闭选择。此项只
    // 验证SDK拒绝跨boot复用，外层迁移/跨boot预算生命周期仍须单独实现。
    // 单独新SDK从外层WAL恢复最终选择，不覆盖上面保留的旧未知。
    let restart_holder =
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, Duration::from_secs(5)).await?;
    restart_holder.restore_admission_continuation(&final_close)?;
    cluster.restart()?;
    let execution = cluster.connect().await?;
    let observer = cluster.connect().await?;
    let fresh =
        DsgSqlClient::bind(&execution, &observer, VM, COMPUTE, Duration::from_secs(5)).await?;
    assert_ne!(fresh.instance(), &final_close.close.instance);
    assert!(fresh.restore_admission_continuation(&final_close).is_err());
    assert_eq!(
        restart_holder.pending_admission_continuation()?,
        Some(final_close)
    );
    cluster.passed = true;
    Ok(())
}
