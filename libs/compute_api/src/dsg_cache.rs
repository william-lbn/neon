//! Samples PostgreSQL database-level buffer activity for the cache observer.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::dsg_control::{DsgInstanceIdentity, DsgUint64};

pub const MAX_CACHE_DATABASES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgCacheDatabaseCounters {
    pub database_oid: u32,
    pub stats_reset_at_unix_micros: Option<DsgUint64>,
    pub buffer_hits: DsgUint64,
    pub buffer_reads: DsgUint64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgCacheCounters {
    pub sampled_at_unix_micros: DsgUint64,
    pub tenant_id: String,
    pub timeline_id: String,
    pub databases: Vec<DsgCacheDatabaseCounters>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgCacheObservation {
    pub protocol_version: u32,
    pub instance: DsgInstanceIdentity,
    pub counters: DsgCacheCounters,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgDemandWindow {
    pub seconds: u32,
    pub distinct_pages: u64,
    pub shared_hit_pages: u64,
    pub shared_miss_pages: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgDemandSeries {
    pub version: u32,
    pub scope: String,
    pub block_bytes: u32,
    pub covered_seconds: u32,
    pub windows: Vec<DsgDemandWindow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgDemandSample {
    pub sampled_at_unix_micros: DsgUint64,
    pub tenant_id: String,
    pub timeline_id: String,
    pub demand: DsgDemandSeries,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DsgLogicalDemandObservation {
    pub protocol_version: u32,
    pub instance: DsgInstanceIdentity,
    pub sample: DsgDemandSample,
}

fn valid_storage_identity(value: &str) -> bool {
    value.len() == 32
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl DsgLogicalDemandObservation {
    pub(crate) fn validate(&self) -> Result<()> {
        let sample = &self.sample;
        let series = &sample.demand;
        ensure!(self.protocol_version == 1 && series.version == 1, "unsupported demand version");
        ensure!(
            sample.sampled_at_unix_micros.get() > 0
                && valid_storage_identity(&sample.tenant_id)
                && valid_storage_identity(&sample.timeline_id)
                && series.scope == "postmaster_shared_relation_demand",
            "logical demand observation has no matching source"
        );
        ensure!(
            series.block_bytes.is_power_of_two() && (512..=32768).contains(&series.block_bytes),
            "invalid logical demand block size"
        );
        ensure!(
            series.covered_seconds <= 3600 && series.windows.len() == (series.covered_seconds / 60) as usize,
            "logical demand windows are incomplete"
        );
        for (index, window) in series.windows.iter().enumerate() {
            ensure!(
                window.seconds == (index as u32 + 1) * 60
                    && [window.distinct_pages, window.shared_hit_pages, window.shared_miss_pages]
                        .into_iter().all(|pages| pages <= (1 << 32)),
                "invalid logical demand window"
            );
        }
        Ok(())
    }
}

impl DsgCacheObservation {
    pub(crate) fn validate(&self) -> Result<()> {
        let counters = &self.counters;
        ensure!(self.protocol_version == 1, "unsupported cache observation protocol");
        ensure!(counters.sampled_at_unix_micros.get() > 0, "missing sample time");
        ensure!(
            valid_storage_identity(&counters.tenant_id) && valid_storage_identity(&counters.timeline_id),
            "missing cache storage identity"
        );
        ensure!(
            !counters.databases.is_empty() && counters.databases.len() <= MAX_CACHE_DATABASES,
            "database observations unavailable or over limit"
        );
        let mut previous = 0;
        for database in &counters.databases {
            ensure!(database.database_oid > previous, "database observations are not unique and ordered");
            if let Some(at) = database.stats_reset_at_unix_micros {
                ensure!(
                    at.get() > 0 && at.get() <= counters.sampled_at_unix_micros.get(),
                    "database statistics epoch is outside the observation window"
                );
            }
            database.buffer_hits.get().checked_add(database.buffer_reads.get())
                .ok_or_else(|| anyhow::anyhow!("logical buffer observations overflow"))?;
            previous = database.database_oid;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_demand_distinguishes_warmup_from_zero_and_checks_window_layout() {
        let mut sample = DsgLogicalDemandObservation {
            protocol_version: 1,
            instance: DsgInstanceIdentity {
                vm_uid: "vm".into(),
                compute_instance_id: "compute".into(),
                pg_boot_id: DsgUint64::new(7),
            },
            sample: DsgDemandSample {
                sampled_at_unix_micros: DsgUint64::new(10),
                tenant_id: "1".repeat(32),
                timeline_id: "2".repeat(32),
                demand: DsgDemandSeries {
                    version: 1,
                    scope: "postmaster_shared_relation_demand".into(),
                    block_bytes: 8192,
                    covered_seconds: 59,
                    windows: Vec::new(),
                },
            },
        };
        sample.validate().unwrap();
        sample.sample.demand.covered_seconds = 60;
        assert!(sample.validate().is_err());
        sample.sample.demand.windows.push(DsgDemandWindow {
            seconds: 60,
            distinct_pages: 0,
            shared_hit_pages: 0,
            shared_miss_pages: 0,
        });
        sample.validate().unwrap();
        let mut wire = serde_json::to_value(&sample).unwrap();
        wire["sample"]["demand"]["windows"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<DsgLogicalDemandObservation>(wire).is_err());
        sample.sample.demand.windows[0].seconds = 59;
        assert!(sample.validate().is_err());
    }

    #[test]
    fn cache_observation_preserves_zero_and_refuses_missing_or_drifted_epochs() {
        let counters = DsgCacheCounters {
            sampled_at_unix_micros: DsgUint64::new(10),
            tenant_id: "1".repeat(32),
            timeline_id: "2".repeat(32),
            databases: vec![DsgCacheDatabaseCounters {
                database_oid: 1,
                stats_reset_at_unix_micros: None,
                buffer_hits: DsgUint64::new(1537),
                buffer_reads: DsgUint64::new(0),
            }],
        };
        let sample = DsgCacheObservation {
            protocol_version: 1,
            instance: DsgInstanceIdentity {
                vm_uid: "vm".into(),
                compute_instance_id: "compute".into(),
                pg_boot_id: DsgUint64::new(1),
            },
            counters,
        };
        sample.validate().unwrap();
        let mut unobserved = sample.clone();
        unobserved.counters.databases.clear();
        assert!(unobserved.validate().is_err());
        let mut duplicate = sample.clone();
        duplicate.counters.databases.push(sample.counters.databases[0].clone());
        assert!(duplicate.validate().is_err());
        let mut reset_after_read = sample.clone();
        reset_after_read.counters.databases[0].stats_reset_at_unix_micros = Some(DsgUint64::new(11));
        assert!(reset_after_read.validate().is_err());
        let wire = serde_json::to_value(sample).unwrap();
        assert_eq!(wire["counters"]["databases"][0]["buffer_reads"], "0");
        assert_eq!(wire["counters"]["databases"][0]["stats_reset_at_unix_micros"], serde_json::Value::Null);
    }
}
