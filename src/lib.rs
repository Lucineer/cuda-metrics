/*!
# cuda-metrics

Telemetry and metrics for agent observability.

You can't improve what you can't measure. Metrics give fleets a pulse —
CPU usage, decision latency, error rates, confidence trends. Without
telemetry, debugging agents is blindfolded surgery.

- Counters (monotonic increments)
- Gauges (up and down values)
- Histograms (distribution tracking)
- Timers (duration measurement)
- Health status (composite health check)
- Metric aggregation
*/

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A counter — always goes up
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Counter {
    pub name: String,
    pub value: u64,
    pub labels: HashMap<String, String>,
}

impl Counter {
    pub fn new(name: &str) -> Self { Counter { name: name.to_string(), value: 0, labels: HashMap::new() } }

    pub fn inc(&mut self) { self.value += 1; }
    pub fn inc_by(&mut self, n: u64) { self.value += n; }
    pub fn with_label(mut self, key: &str, val: &str) -> Self { self.labels.insert(key.to_string(), val.to_string()); self }
    pub fn reset(&mut self) { self.value = 0; }
}

/// A gauge — goes up and down
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gauge {
    pub name: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub labels: HashMap<String, String>,
}

impl Gauge {
    pub fn new(name: &str) -> Self { Gauge { name: name.to_string(), value: 0.0, min: f64::MAX, max: f64::MIN, labels: HashMap::new() } }

    pub fn set(&mut self, val: f64) {
        self.value = val;
        self.min = self.min.min(val);
        self.max = self.max.max(val);
    }

    pub fn inc(&mut self, delta: f64) { self.set(self.value + delta); }
    pub fn dec(&mut self, delta: f64) { self.set(self.value - delta); }

    pub fn range(&self) -> f64 { self.max - self.min }
    pub fn normalized(&self) -> f64 { // 0-1 between min and max
        let range = self.range();
        if range < 1e-10 { return 0.5; }
        (self.value - self.min) / range
    }
}

/// A histogram — distribution of values
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Histogram {
    pub name: String,
    pub buckets: Vec<f64>,
    pub counts: Vec<u64>,
    pub count: u64,
    pub sum: f64,
}

impl Histogram {
    pub fn new(name: &str, boundaries: &[f64]) -> Self {
        let mut buckets = boundaries.to_vec();
        buckets.sort_by(|a, b| a.partial_cmp(b).unwrap());
        buckets.dedup();
        Histogram { name: name.to_string(), buckets, counts: vec![0; buckets.len() + 1], count: 0, sum: 0.0 }
    }

    pub fn observe(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;
        for (i, &boundary) in self.buckets.iter().enumerate() {
            if value <= boundary { self.counts[i] += 1; return; }
        }
        // Overflow bucket
        *self.counts.last_mut().unwrap() += 1;
    }

    pub fn percentile(&self, p: f64) -> f64 {
        if self.count == 0 { return 0.0; }
        let target = (p * self.count as f64 / 100.0) as u64;
        let mut cumulative = 0u64;
        for (i, &count) in self.counts.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                return if i < self.buckets.len() { self.buckets[i] } else { self.buckets.last().copied().unwrap_or(0.0) * 1.5 };
            }
        }
        self.buckets.last().copied().unwrap_or(0.0)
    }

    pub fn avg(&self) -> f64 { if self.count == 0 { return 0.0; } self.sum / self.count as f64 }

    pub fn reset(&mut self) { self.counts = vec![0; self.counts.len()]; self.count = 0; self.sum = 0.0; }
}

/// A timer — measures duration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Timer {
    pub name: String,
    pub histogram: Histogram,
}

impl Timer {
    pub fn new(name: &str) -> Self {
        Timer { name: name.to_string(), histogram: Histogram::new(&format!("{}_hist", name), &[1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 5000.0]) }
    }

    pub fn record(&mut self, duration_ms: f64) { self.histogram.observe(duration_ms); }

    pub fn avg_ms(&self) -> f64 { self.histogram.avg() }
    pub fn p50(&self) -> f64 { self.histogram.percentile(50.0) }
    pub fn p95(&self) -> f64 { self.histogram.percentile(95.0) }
    pub fn p99(&self) -> f64 { self.histogram.percentile(99.0) }
}

/// Health status
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus { Healthy, Degraded, Unhealthy, Unknown }

/// A health check
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: HealthStatus,
    pub message: String,
    pub last_check: u64,
    pub response_time_ms: f64,
    pub consecutive_failures: u32,
}

impl HealthCheck {
    pub fn healthy(name: &str) -> Self { HealthCheck { name: name.to_string(), status: HealthStatus::Healthy, message: "ok".into(), last_check: now(), response_time_ms: 0.0, consecutive_failures: 0 } }
    pub fn unhealthy(name: &str, reason: &str) -> Self { HealthCheck { name: name.to_string(), status: HealthStatus::Unhealthy, message: reason.to_string(), last_check: now(), response_time_ms: 0.0, consecutive_failures: 1 } }

    pub fn check_ok(&mut self) { self.status = HealthStatus::Healthy; self.consecutive_failures = 0; self.message = "ok".into(); self.last_check = now(); }
    pub fn check_fail(&mut self, reason: &str) { self.consecutive_failures += 1; if self.consecutive_failures >= 3 { self.status = HealthStatus::Unhealthy; } else { self.status = HealthStatus::Degraded; } self.message = reason.to_string(); self.last_check = now(); }
}

/// The metrics registry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricsRegistry {
    pub counters: HashMap<String, Counter>,
    pub gauges: HashMap<String, Gauge>,
    pub histograms: HashMap<String, Histogram>,
    pub timers: HashMap<String, Timer>,
    pub health_checks: HashMap<String, HealthCheck>,
    pub start_time: u64,
}

impl MetricsRegistry {
    pub fn new() -> Self { MetricsRegistry { counters: HashMap::new(), gauges: HashMap::new(), histograms: HashMap::new(), timers: HashMap::new(), health_checks: HashMap::new(), start_time: now() } }

    pub fn counter(&mut self, name: &str) -> &mut Counter { self.counters.entry(name.to_string()).or_insert_with(|| Counter::new(name)) }
    pub fn gauge(&mut self, name: &str) -> &mut Gauge { self.gauges.entry(name.to_string()).or_insert_with(|| Gauge::new(name)) }
    pub fn histogram(&mut self, name: &str, bounds: &[f64]) -> &mut Histogram { self.histograms.entry(name.to_string()).or_insert_with(|| Histogram::new(name, bounds)) }
    pub fn timer(&mut self, name: &str) -> &mut Timer { self.timers.entry(name.to_string()).or_insert_with(|| Timer::new(name)) }

    pub fn health(&mut self, name: &str) -> &mut HealthCheck { self.health_checks.entry(name.to_string()).or_insert_with(|| HealthCheck::healthy(name)) }

    /// Composite health — worst status wins
    pub fn overall_health(&self) -> HealthStatus {
        if self.health_checks.is_empty() { return HealthStatus::Unknown; }
        let mut worst = HealthStatus::Healthy;
        for check in self.health_checks.values() {
            match (check.status, worst) {
                (HealthStatus::Unhealthy, _) | (_, HealthStatus::Healthy) => worst = check.status,
                (HealthStatus::Degraded, HealthStatus::Healthy) => worst = HealthStatus::Degraded,
                _ => {}
            }
        }
        worst
    }

    /// Uptime in seconds
    pub fn uptime_secs(&self) -> f64 { (now() - self.start_time) as f64 / 1000.0 }

    /// Collect all metrics as key-value pairs for reporting
    pub fn collect(&self) -> Vec<(String, f64)> {
        let mut metrics = vec![];
        for (name, c) in &self.counters { metrics.push((format!("counter:{}", name), c.value as f64)); }
        for (name, g) in &self.gauges { metrics.push((format!("gauge:{}", name), g.value)); }
        for (name, h) in &self.histograms { metrics.push((format!("hist:{}_avg", name), h.avg())); metrics.push((format!("hist:{}_p95", name), h.percentile(95.0))); }
        for (name, t) in &self.timers { metrics.push((format!("timer:{}_avg", name), t.avg_ms())); metrics.push((format!("timer:{}_p50", name), t.p50())); }
        metrics
    }

    /// Summary
    pub fn summary(&self) -> String {
        let health_str = match self.overall_health() { HealthStatus::Healthy => "HEALTHY", HealthStatus::Degraded => "DEGRADED", HealthStatus::Unhealthy => "UNHEALTHY", HealthStatus::Unknown => "UNKNOWN" };
        format!("Metrics[{:?}]: {} counters, {} gauges, {} histograms, {} timers, {} health checks, uptime={:.0}s",
            health_str, self.counters.len(), self.gauges.len(), self.histograms.len(), self.timers.len(), self.health_checks.len(), self.uptime_secs())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let mut c = Counter::new("requests");
        c.inc(); c.inc(); c.inc_by(5);
        assert_eq!(c.value, 7);
    }

    #[test]
    fn test_gauge() {
        let mut g = Gauge::new("temperature");
        g.set(20.0); g.set(30.0); g.set(25.0);
        assert!((g.value - 25.0).abs() < 0.01);
        assert_eq!(g.min, 20.0);
        assert_eq!(g.max, 30.0);
    }

    #[test]
    fn test_gauge_normalized() {
        let mut g = Gauge::new("x");
        g.set(0.0); g.set(100.0);
        g.set(50.0);
        assert!((g.normalized() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_histogram_percentile() {
        let mut h = Histogram::new("latency", &[10.0, 50.0, 100.0]);
        for _ in 0..80 { h.observe(5.0); }  // ≤10
        for _ in 0..15 { h.observe(30.0); } // 10-50
        for _ in 0..5 { h.observe(75.0); }  // 50-100
        assert_eq!(h.count, 100);
        let p50 = h.percentile(50.0);
        assert_eq!(p50, 10.0); // 50th percentile is in first bucket
    }

    #[test]
    fn test_histogram_avg() {
        let mut h = Histogram::new("x", &[10.0]);
        h.observe(10.0); h.observe(20.0); h.observe(30.0);
        assert!((h.avg() - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_timer() {
        let mut t = Timer::new("api_call");
        t.record(50.0); t.record(100.0); t.record(150.0);
        assert!((t.avg_ms() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_health_check_ok() {
        let mut hc = HealthCheck::healthy("db");
        hc.check_ok();
        assert_eq!(hc.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_check_degraded_then_unhealthy() {
        let mut hc = HealthCheck::healthy("api");
        hc.check_fail("timeout");
        assert_eq!(hc.status, HealthStatus::Degraded);
        hc.check_fail("timeout");
        hc.check_fail("timeout");
        assert_eq!(hc.status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_registry_overall_health() {
        let mut mr = MetricsRegistry::new();
        mr.health("db").check_ok();
        mr.health("api").check_fail("down");
        assert_eq!(mr.overall_health(), HealthStatus::Degraded);
    }

    #[test]
    fn test_collect() {
        let mut mr = MetricsRegistry::new();
        mr.counter("requests").inc_by(42);
        mr.gauge("cpu").set(0.75);
        let metrics = mr.collect();
        assert!(metrics.len() >= 2);
    }

    #[test]
    fn test_summary() {
        let mr = MetricsRegistry::new();
        let s = mr.summary();
        assert!(s.contains("HEALTHY"));
    }
}
