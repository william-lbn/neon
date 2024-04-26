//! HyperLogLog is an algorithm for the count-distinct problem,
//! approximating the number of distinct elements in a multiset.
//! Calculating the exact cardinality of the distinct elements
//! of a multiset requires an amount of memory proportional to
//! the cardinality, which is impractical for very large data sets.
//! Probabilistic cardinality estimators, such as the HyperLogLog algorithm,
//! use significantly less memory than this, but can only approximate the cardinality.

use std::{
    collections::HashMap,
    hash::{BuildHasher, BuildHasherDefault, Hash, Hasher},
    sync::{atomic::AtomicU8, Arc, RwLock},
};

use prometheus::{
    core::{self, Describer},
    proto, Opts,
};
use twox_hash::xxh3;

/// Create an [`HyperLogLogVec`] and registers to default registry.
#[macro_export(local_inner_macros)]
macro_rules! register_hll_vec {
    ($N:literal, $OPTS:expr, $LABELS_NAMES:expr $(,)?) => {{
        let hll_vec = $crate::HyperLogLogVec::<$N>::new($OPTS, $LABELS_NAMES).unwrap();
        $crate::register(Box::new(hll_vec.clone())).map(|_| hll_vec)
    }};

    ($N:literal, $NAME:expr, $HELP:expr, $LABELS_NAMES:expr $(,)?) => {{
        $crate::register_hll_vec!($N, $crate::opts!($NAME, $HELP), $LABELS_NAMES)
    }};
}

/// Create an [`HyperLogLog`] and registers to default registry.
#[macro_export(local_inner_macros)]
macro_rules! register_hll {
    ($N:literal, $OPTS:expr $(,)?) => {{
        let hll = $crate::HyperLogLog::<$N>::with_opts($OPTS).unwrap();
        $crate::register(Box::new(hll.clone())).map(|_| hll)
    }};

    ($N:literal, $NAME:expr, $HELP:expr $(,)?) => {{
        $crate::register_hll!($N, $crate::opts!($NAME, $HELP), $LABELS_NAMES)
    }};
}

/// HLL is a probabilistic cardinality measure.
///
/// How to use this time-series for a metric name `my_metrics_total_hll`:
///
/// ```promql
/// # harmonic mean
/// 1 / (
///     sum (
///         2 ^ -(
///             # HLL merge operation
///             max (my_metrics_total_hll{}) by (hll_shard, other_labels...)
///         )
///     ) without (hll_shard)
/// )
/// * alpha
/// * shards_count
/// * shards_count
/// ```
///
/// If you want an estimate over time, you can use the following query:
///
/// ```promql
/// # harmonic mean
/// 1 / (
///     sum (
///         2 ^ -(
///             # HLL merge operation
///             max (
///                 max_over_time(my_metrics_total_hll{}[$__rate_interval])
///             ) by (hll_shard, other_labels...)
///         )
///     ) without (hll_shard)
/// )
/// * alpha
/// * shards_count
/// * shards_count
/// ```
///
/// In the case of low cardinality, you might want to use the linear counting approximation:
///
/// ```promql
/// # LinearCounting(m, V) = m log (m / V)
/// shards_count * ln(shards_count /
///     # calculate V = how many shards contain a 0
///     count(max (proxy_connecting_endpoints{}) by (hll_shard, protocol) == 0) without (hll_shard)
/// )
/// ```
///
/// See <https://en.wikipedia.org/wiki/HyperLogLog#Practical_considerations> for estimates on alpha
#[derive(Clone)]
pub struct HyperLogLogVec<const N: usize> {
    core: Arc<HyperLogLogVecCore<N>>,
}

struct HyperLogLogVecCore<const N: usize> {
    pub children: RwLock<HashMap<u64, HyperLogLog<N>, BuildHasherDefault<xxh3::Hash64>>>,
    pub desc: core::Desc,
    pub opts: Opts,
}

impl<const N: usize> core::Collector for HyperLogLogVec<N> {
    fn desc(&self) -> Vec<&core::Desc> {
        vec![&self.core.desc]
    }

    fn collect(&self) -> Vec<proto::MetricFamily> {
        let mut m = proto::MetricFamily::default();
        m.set_name(self.core.desc.fq_name.clone());
        m.set_help(self.core.desc.help.clone());
        m.set_field_type(proto::MetricType::GAUGE);

        let mut metrics = Vec::new();
        for child in self.core.children.read().unwrap().values() {
            child.core.collect_into(&mut metrics);
        }
        m.set_metric(metrics);

        vec![m]
    }
}

impl<const N: usize> HyperLogLogVec<N> {
    /// Create a new [`HyperLogLogVec`] based on the provided
    /// [`Opts`] and partitioned by the given label names. At least one label name must be
    /// provided.
    pub fn new(opts: Opts, label_names: &[&str]) -> prometheus::Result<Self> {
        assert!(N.is_power_of_two());
        let variable_names = label_names.iter().map(|s| (*s).to_owned()).collect();
        let opts = opts.variable_labels(variable_names);

        let desc = opts.describe()?;
        let v = HyperLogLogVecCore {
            children: RwLock::new(HashMap::default()),
            desc,
            opts,
        };

        Ok(Self { core: Arc::new(v) })
    }

    /// `get_metric_with_label_values` returns the [`HyperLogLog<P>`] for the given slice
    /// of label values (same order as the VariableLabels in Desc). If that combination of
    /// label values is accessed for the first time, a new [`HyperLogLog<P>`] is created.
    ///
    /// An error is returned if the number of label values is not the same as the
    /// number of VariableLabels in Desc.
    pub fn get_metric_with_label_values(
        &self,
        vals: &[&str],
    ) -> prometheus::Result<HyperLogLog<N>> {
        self.core.get_metric_with_label_values(vals)
    }

    /// `with_label_values` works as `get_metric_with_label_values`, but panics if an error
    /// occurs.
    pub fn with_label_values(&self, vals: &[&str]) -> HyperLogLog<N> {
        self.get_metric_with_label_values(vals).unwrap()
    }
}

impl<const N: usize> HyperLogLogVecCore<N> {
    pub fn get_metric_with_label_values(
        &self,
        vals: &[&str],
    ) -> prometheus::Result<HyperLogLog<N>> {
        let h = self.hash_label_values(vals)?;

        if let Some(metric) = self.children.read().unwrap().get(&h).cloned() {
            return Ok(metric);
        }

        self.get_or_create_metric(h, vals)
    }

    pub(crate) fn hash_label_values(&self, vals: &[&str]) -> prometheus::Result<u64> {
        if vals.len() != self.desc.variable_labels.len() {
            return Err(prometheus::Error::InconsistentCardinality {
                expect: self.desc.variable_labels.len(),
                got: vals.len(),
            });
        }

        let mut h = xxh3::Hash64::default();
        for val in vals {
            h.write(val.as_bytes());
        }

        Ok(h.finish())
    }

    fn get_or_create_metric(
        &self,
        hash: u64,
        label_values: &[&str],
    ) -> prometheus::Result<HyperLogLog<N>> {
        let mut children = self.children.write().unwrap();
        // Check exist first.
        if let Some(metric) = children.get(&hash).cloned() {
            return Ok(metric);
        }

        let metric = HyperLogLog::with_opts_and_label_values(&self.opts, label_values)?;
        children.insert(hash, metric.clone());
        Ok(metric)
    }
}

/// HLL is a probabilistic cardinality measure.
///
/// How to use this time-series for a metric name `my_metrics_total_hll`:
///
/// ```promql
/// # harmonic mean
/// 1 / (
///     sum (
///         2 ^ -(
///             # HLL merge operation
///             max (my_metrics_total_hll{}) by (hll_shard, other_labels...)
///         )
///     ) without (hll_shard)
/// )
/// * alpha
/// * shards_count
/// * shards_count
/// ```
///
/// If you want an estimate over time, you can use the following query:
///
/// ```promql
/// # harmonic mean
/// 1 / (
///     sum (
///         2 ^ -(
///             # HLL merge operation
///             max (
///                 max_over_time(my_metrics_total_hll{}[$__rate_interval])
///             ) by (hll_shard, other_labels...)
///         )
///     ) without (hll_shard)
/// )
/// * alpha
/// * shards_count
/// * shards_count
/// ```
///
/// In the case of low cardinality, you might want to use the linear counting approximation:
///
/// ```promql
/// # LinearCounting(m, V) = m log (m / V)
/// shards_count * ln(shards_count /
///     # calculate V = how many shards contain a 0
///     count(max (proxy_connecting_endpoints{}) by (hll_shard, protocol) == 0) without (hll_shard)
/// )
/// ```
///
/// See <https://en.wikipedia.org/wiki/HyperLogLog#Practical_considerations> for estimates on alpha
#[derive(Clone)]
pub struct HyperLogLog<const N: usize> {
    core: Arc<HyperLogLogCore<N>>,
}

impl<const N: usize> HyperLogLog<N> {
    /// Create a [`HyperLogLog`] with the `name` and `help` arguments.
    pub fn new<S1: Into<String>, S2: Into<String>>(name: S1, help: S2) -> prometheus::Result<Self> {
        assert!(N.is_power_of_two());
        let opts = Opts::new(name, help);
        Self::with_opts(opts)
    }

    /// Create a [`HyperLogLog`] with the `opts` options.
    pub fn with_opts(opts: Opts) -> prometheus::Result<Self> {
        Self::with_opts_and_label_values(&opts, &[])
    }

    fn with_opts_and_label_values(opts: &Opts, label_values: &[&str]) -> prometheus::Result<Self> {
        let desc = opts.describe()?;
        let labels = make_label_pairs(&desc, label_values)?;

        let v = HyperLogLogCore {
            shards: [0; N].map(AtomicU8::new),
            desc,
            labels,
        };
        Ok(Self { core: Arc::new(v) })
    }

    pub fn measure(&self, item: &impl Hash) {
        // changing the hasher will break compatibility with previous measurements.
        self.record(BuildHasherDefault::<xxh3::Hash64>::default().hash_one(item));
    }

    fn record(&self, hash: u64) {
        let p = N.ilog2() as u8;
        let j = hash & (N as u64 - 1);
        let rho = (hash >> p).leading_zeros() as u8 + 1 - p;
        self.core.shards[j as usize].fetch_max(rho, std::sync::atomic::Ordering::Relaxed);
    }
}

struct HyperLogLogCore<const N: usize> {
    shards: [AtomicU8; N],
    desc: core::Desc,
    labels: Vec<proto::LabelPair>,
}

impl<const N: usize> core::Collector for HyperLogLog<N> {
    fn desc(&self) -> Vec<&core::Desc> {
        vec![&self.core.desc]
    }

    fn collect(&self) -> Vec<proto::MetricFamily> {
        let mut m = proto::MetricFamily::default();
        m.set_name(self.core.desc.fq_name.clone());
        m.set_help(self.core.desc.help.clone());
        m.set_field_type(proto::MetricType::GAUGE);

        let mut metrics = Vec::new();
        self.core.collect_into(&mut metrics);
        m.set_metric(metrics);

        vec![m]
    }
}

impl<const N: usize> HyperLogLogCore<N> {
    fn collect_into(&self, metrics: &mut Vec<proto::Metric>) {
        self.shards.iter().enumerate().for_each(|(i, x)| {
            let mut shard_label = proto::LabelPair::default();
            shard_label.set_name("hll_shard".to_owned());
            shard_label.set_value(format!("{i}"));

            // We reset the counter to 0 so we can perform a cardinality measure over any time slice in prometheus.

            // This seems like it would be a race condition,
            // but HLL is not impacted by a write in one shard happening in between.
            // This is because in PromQL we will be implementing a harmonic mean of all buckets.
            // we will also merge samples in a time series using `max by (hll_shard)`.

            // TODO: maybe we shouldn't reset this on every collect, instead, only after a time window.
            // this would mean that a dev port-forwarding the metrics url won't break the sampling.
            let v = x.swap(0, std::sync::atomic::Ordering::Relaxed);

            let mut m = proto::Metric::default();
            let mut c = proto::Gauge::default();
            c.set_value(v as f64);
            m.set_gauge(c);

            let mut labels = Vec::with_capacity(self.labels.len() + 1);
            labels.extend_from_slice(&self.labels);
            labels.push(shard_label);

            m.set_label(labels);
            metrics.push(m);
        })
    }
}

fn make_label_pairs(
    desc: &core::Desc,
    label_values: &[&str],
) -> prometheus::Result<Vec<proto::LabelPair>> {
    if desc.variable_labels.len() != label_values.len() {
        return Err(prometheus::Error::InconsistentCardinality {
            expect: desc.variable_labels.len(),
            got: label_values.len(),
        });
    }

    let total_len = desc.variable_labels.len() + desc.const_label_pairs.len();
    if total_len == 0 {
        return Ok(vec![]);
    }

    if desc.variable_labels.is_empty() {
        return Ok(desc.const_label_pairs.clone());
    }

    let mut label_pairs = Vec::with_capacity(total_len);
    for (i, n) in desc.variable_labels.iter().enumerate() {
        let mut label_pair = proto::LabelPair::default();
        label_pair.set_name(n.clone());
        label_pair.set_value(label_values[i].to_owned());
        label_pairs.push(label_pair);
    }

    for label_pair in &desc.const_label_pairs {
        label_pairs.push(label_pair.clone());
    }
    label_pairs.sort();
    Ok(label_pairs)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use prometheus::{proto, Opts};
    use rand::{rngs::StdRng, Rng, SeedableRng};
    use rand_distr::{Distribution, Zipf};

    use crate::HyperLogLogVec;

    fn collect(hll: &HyperLogLogVec<32>) -> Vec<proto::Metric> {
        let mut metrics = vec![];
        hll.core
            .children
            .read()
            .unwrap()
            .values()
            .for_each(|c| c.core.collect_into(&mut metrics));
        metrics
    }
    fn get_cardinality(metrics: &[proto::Metric], filter: impl Fn(&proto::Metric) -> bool) -> f64 {
        let mut buckets = [0.0; 32];
        for metric in metrics.chunks_exact(32) {
            if filter(&metric[0]) {
                for (i, m) in metric.iter().enumerate() {
                    buckets[i] = f64::max(buckets[i], m.get_gauge().get_value());
                }
            }
        }

        buckets
            .into_iter()
            .map(|f| 2.0f64.powf(-f))
            .sum::<f64>()
            .recip()
            * 0.697
            * 32.0
            * 32.0
    }

    fn test_cardinality(n: usize, dist: impl Distribution<f64>) -> ([usize; 3], [f64; 3]) {
        let hll = HyperLogLogVec::<32>::new(Opts::new("foo", "bar"), &["x"]).unwrap();

        let mut iter = StdRng::seed_from_u64(0x2024_0112).sample_iter(dist);
        let mut set_a = HashSet::new();
        let mut set_b = HashSet::new();

        for x in iter.by_ref().take(n) {
            set_a.insert(x.to_bits());
            hll.with_label_values(&["a"]).measure(&x.to_bits());
        }
        for x in iter.by_ref().take(n) {
            set_b.insert(x.to_bits());
            hll.with_label_values(&["b"]).measure(&x.to_bits());
        }
        let merge = &set_a | &set_b;

        let metrics = collect(&hll);
        let len = get_cardinality(&metrics, |_| true);
        let len_a = get_cardinality(&metrics, |l| l.get_label()[0].get_value() == "a");
        let len_b = get_cardinality(&metrics, |l| l.get_label()[0].get_value() == "b");

        ([merge.len(), set_a.len(), set_b.len()], [len, len_a, len_b])
    }

    #[test]
    fn test_cardinality_small() {
        let (actual, estimate) = test_cardinality(100, Zipf::new(100, 1.2f64).unwrap());

        assert_eq!(actual, [46, 30, 32]);
        assert!(51.3 < estimate[0] && estimate[0] < 51.4);
        assert!(44.0 < estimate[1] && estimate[1] < 44.1);
        assert!(39.0 < estimate[2] && estimate[2] < 39.1);
    }

    #[test]
    fn test_cardinality_medium() {
        let (actual, estimate) = test_cardinality(10000, Zipf::new(10000, 1.2f64).unwrap());

        assert_eq!(actual, [2529, 1618, 1629]);
        assert!(2309.1 < estimate[0] && estimate[0] < 2309.2);
        assert!(1566.6 < estimate[1] && estimate[1] < 1566.7);
        assert!(1629.5 < estimate[2] && estimate[2] < 1629.6);
    }

    #[test]
    fn test_cardinality_large() {
        let (actual, estimate) = test_cardinality(1_000_000, Zipf::new(1_000_000, 1.2f64).unwrap());

        assert_eq!(actual, [129077, 79579, 79630]);
        assert!(126067.2 < estimate[0] && estimate[0] < 126067.3);
        assert!(83076.8 < estimate[1] && estimate[1] < 83076.9);
        assert!(64251.2 < estimate[2] && estimate[2] < 64251.3);
    }

    #[test]
    fn test_cardinality_small2() {
        let (actual, estimate) = test_cardinality(100, Zipf::new(200, 0.8f64).unwrap());

        assert_eq!(actual, [92, 58, 60]);
        assert!(116.1 < estimate[0] && estimate[0] < 116.2);
        assert!(81.7 < estimate[1] && estimate[1] < 81.8);
        assert!(69.3 < estimate[2] && estimate[2] < 69.4);
    }

    #[test]
    fn test_cardinality_medium2() {
        let (actual, estimate) = test_cardinality(10000, Zipf::new(20000, 0.8f64).unwrap());

        assert_eq!(actual, [8201, 5131, 5051]);
        assert!(6846.4 < estimate[0] && estimate[0] < 6846.5);
        assert!(5239.1 < estimate[1] && estimate[1] < 5239.2);
        assert!(4292.8 < estimate[2] && estimate[2] < 4292.9);
    }

    #[test]
    fn test_cardinality_large2() {
        let (actual, estimate) = test_cardinality(1_000_000, Zipf::new(2_000_000, 0.8f64).unwrap());

        assert_eq!(actual, [777847, 482069, 482246]);
        assert!(699437.4 < estimate[0] && estimate[0] < 699437.5);
        assert!(374948.9 < estimate[1] && estimate[1] < 374949.0);
        assert!(434609.7 < estimate[2] && estimate[2] < 434609.8);
    }
}
