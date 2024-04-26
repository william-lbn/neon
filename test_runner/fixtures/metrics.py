from collections import defaultdict
from typing import Dict, List, Optional, Tuple

from prometheus_client.parser import text_string_to_metric_families
from prometheus_client.samples import Sample

from fixtures.log_helper import log


class Metrics:
    metrics: Dict[str, List[Sample]]
    name: str

    def __init__(self, name: str = ""):
        self.metrics = defaultdict(list)
        self.name = name

    def query_all(self, name: str, filter: Optional[Dict[str, str]] = None) -> List[Sample]:
        filter = filter or {}
        res = []

        for sample in self.metrics[name]:
            try:
                if all(sample.labels[k] == v for k, v in filter.items()):
                    res.append(sample)
            except KeyError:
                pass
        return res

    def query_one(self, name: str, filter: Optional[Dict[str, str]] = None) -> Sample:
        res = self.query_all(name, filter or {})
        assert len(res) == 1, f"expected single sample for {name} {filter}, found {res}"
        return res[0]


class MetricsGetter:
    """
    Mixin for types that implement a `get_metrics` function and would like associated
    helpers for querying the metrics
    """

    def get_metrics(self) -> Metrics:
        raise NotImplementedError()

    def get_metric_value(
        self, name: str, filter: Optional[Dict[str, str]] = None
    ) -> Optional[float]:
        metrics = self.get_metrics()
        results = metrics.query_all(name, filter=filter)
        if not results:
            log.info(f'could not find metric "{name}"')
            return None
        assert len(results) == 1, f"metric {name} with given filters is not unique, got: {results}"
        return results[0].value

    def get_metrics_values(
        self, names: list[str], filter: Optional[Dict[str, str]] = None, absence_ok=False
    ) -> Dict[str, float]:
        """
        When fetching multiple named metrics, it is more efficient to use this
        than to call `get_metric_value` repeatedly.

        Throws RuntimeError if no metrics matching `names` are found, or if
        not all of `names` are found: this method is intended for loading sets
        of metrics whose existence is coupled.

        If it's expected that there may be no results for some of the metrics,
        specify `absence_ok=True`. The returned dict will then not contain values
        for these metrics.
        """
        metrics = self.get_metrics()
        samples = []
        for name in names:
            samples.extend(metrics.query_all(name, filter=filter))

        result = {}
        for sample in samples:
            if sample.name in result:
                raise RuntimeError(f"Multiple values found for {sample.name}")
            result[sample.name] = sample.value

        if not absence_ok:
            if len(result) != len(names):
                log.info(f"Metrics found: {metrics.metrics}")
                raise RuntimeError(f"could not find all metrics {' '.join(names)}")

        return result


def parse_metrics(text: str, name: str = "") -> Metrics:
    metrics = Metrics(name)
    gen = text_string_to_metric_families(text)
    for family in gen:
        for sample in family.samples:
            metrics.metrics[sample.name].append(sample)

    return metrics


def histogram(prefix_without_trailing_underscore: str) -> List[str]:
    assert not prefix_without_trailing_underscore.endswith("_")
    return [f"{prefix_without_trailing_underscore}_{x}" for x in ["bucket", "count", "sum"]]


PAGESERVER_PER_TENANT_REMOTE_TIMELINE_CLIENT_METRICS: Tuple[str, ...] = (
    "pageserver_remote_timeline_client_calls_started_total",
    "pageserver_remote_timeline_client_calls_finished_total",
    "pageserver_remote_physical_size",
    "pageserver_remote_timeline_client_bytes_started_total",
    "pageserver_remote_timeline_client_bytes_finished_total",
)

PAGESERVER_GLOBAL_METRICS: Tuple[str, ...] = (
    "pageserver_storage_operations_seconds_global_count",
    "pageserver_storage_operations_seconds_global_sum",
    "pageserver_storage_operations_seconds_global_bucket",
    "pageserver_unexpected_ondemand_downloads_count_total",
    "libmetrics_launch_timestamp",
    "libmetrics_build_info",
    "libmetrics_tracing_event_count_total",
    "pageserver_materialized_cache_hits_total",
    "pageserver_materialized_cache_hits_direct_total",
    "pageserver_page_cache_read_hits_total",
    "pageserver_page_cache_read_accesses_total",
    "pageserver_page_cache_size_current_bytes",
    "pageserver_page_cache_size_max_bytes",
    "pageserver_getpage_reconstruct_seconds_bucket",
    "pageserver_getpage_reconstruct_seconds_count",
    "pageserver_getpage_reconstruct_seconds_sum",
    *[f"pageserver_basebackup_query_seconds_{x}" for x in ["bucket", "count", "sum"]],
    *histogram("pageserver_smgr_query_seconds_global"),
    *histogram("pageserver_read_num_fs_layers"),
    *histogram("pageserver_getpage_get_reconstruct_data_seconds"),
    *histogram("pageserver_wait_lsn_seconds"),
    *histogram("pageserver_remote_operation_seconds"),
    *histogram("pageserver_io_operations_seconds"),
    "pageserver_tenant_states_count",
)

PAGESERVER_PER_TENANT_METRICS: Tuple[str, ...] = (
    "pageserver_current_logical_size",
    "pageserver_resident_physical_size",
    "pageserver_io_operations_bytes_total",
    "pageserver_last_record_lsn",
    "pageserver_smgr_query_seconds_bucket",
    "pageserver_smgr_query_seconds_count",
    "pageserver_smgr_query_seconds_sum",
    "pageserver_storage_operations_seconds_count_total",
    "pageserver_storage_operations_seconds_sum_total",
    "pageserver_created_persistent_files_total",
    "pageserver_written_persistent_bytes_total",
    "pageserver_evictions_total",
    "pageserver_evictions_with_low_residence_duration_total",
    *PAGESERVER_PER_TENANT_REMOTE_TIMELINE_CLIENT_METRICS,
    # "pageserver_directory_entries_count", -- only used if above a certain threshold
    # "pageserver_broken_tenants_count" -- used only for broken
)
