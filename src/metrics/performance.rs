//! 性能分析和优化模块
//!
//! 提供性能监控、分析和优化建议

use std::time::{Instant, Duration};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// 性能指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// 操作名称
    pub operation: String,
    /// 执行时间（毫秒）
    pub duration_ms: f64,
    /// 内存使用（字节）
    pub memory_bytes: Option<u64>,
    /// 时间戳
    pub timestamp_ms: i64,
}

impl PerformanceMetrics {
    pub fn new(operation: String, duration_ms: f64) -> Self {
        Self {
            operation,
            duration_ms,
            memory_bytes: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn with_memory(mut self, memory_bytes: u64) -> Self {
        self.memory_bytes = Some(memory_bytes);
        self
    }
}

/// 性能基线
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceBaseline {
    /// 操作名称
    pub operation: String,
    /// 平均执行时间（毫秒）
    pub avg_duration_ms: f64,
    /// 最小执行时间（毫秒）
    pub min_duration_ms: f64,
    /// 最大执行时间（毫秒）
    pub max_duration_ms: f64,
    /// 标准差
    pub std_dev_ms: f64,
    /// 样本数
    pub sample_count: usize,
}

impl PerformanceBaseline {
    pub fn from_metrics(operation: String, metrics: &[PerformanceMetrics]) -> Self {
        if metrics.is_empty() {
            return Self {
                operation,
                avg_duration_ms: 0.0,
                min_duration_ms: 0.0,
                max_duration_ms: 0.0,
                std_dev_ms: 0.0,
                sample_count: 0,
            };
        }

        let durations: Vec<f64> = metrics.iter().map(|m| m.duration_ms).collect();
        let avg = durations.iter().sum::<f64>() / durations.len() as f64;
        let variance = durations
            .iter()
            .map(|d| (d - avg).powi(2))
            .sum::<f64>()
            / durations.len() as f64;
        let std_dev = variance.sqrt();

        Self {
            operation,
            avg_duration_ms: avg,
            min_duration_ms: durations.iter().cloned().fold(f64::INFINITY, f64::min),
            max_duration_ms: durations.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            std_dev_ms: std_dev,
            sample_count: metrics.len(),
        }
    }

    /// 检查是否超过阈值
    pub fn exceeds_threshold(&self, threshold_ms: f64) -> bool {
        self.avg_duration_ms > threshold_ms
    }

    /// 获取性能评级
    pub fn get_rating(&self) -> PerformanceRating {
        match self.avg_duration_ms {
            d if d < 1.0 => PerformanceRating::Excellent,
            d if d < 10.0 => PerformanceRating::Good,
            d if d < 100.0 => PerformanceRating::Fair,
            d if d < 1000.0 => PerformanceRating::Poor,
            _ => PerformanceRating::Critical,
        }
    }
}

/// 性能评级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PerformanceRating {
    /// 优秀（< 1ms）
    Excellent,
    /// 良好（1-10ms）
    Good,
    /// 一般（10-100ms）
    Fair,
    /// 较差（100-1000ms）
    Poor,
    /// 严重（> 1000ms）
    Critical,
}

impl std::fmt::Display for PerformanceRating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PerformanceRating::Excellent => write!(f, "Excellent"),
            PerformanceRating::Good => write!(f, "Good"),
            PerformanceRating::Fair => write!(f, "Fair"),
            PerformanceRating::Poor => write!(f, "Poor"),
            PerformanceRating::Critical => write!(f, "Critical"),
        }
    }
}

/// 性能监控器
pub struct PerformanceMonitor {
    metrics: Arc<RwLock<Vec<PerformanceMetrics>>>,
    baselines: Arc<RwLock<Vec<PerformanceBaseline>>>,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(Vec::new())),
            baselines: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 记录性能指标
    pub async fn record(&self, metric: PerformanceMetrics) {
        let mut metrics = self.metrics.write().await;
        metrics.push(metric);
    }

    /// 开始计时
    pub fn start_timer() -> PerformanceTimer {
        PerformanceTimer {
            start: Instant::now(),
        }
    }

    /// 建立性能基线
    pub async fn establish_baseline(&self, operation: String) {
        let metrics = self.metrics.read().await;
        let operation_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.operation == operation)
            .cloned()
            .collect();

        if !operation_metrics.is_empty() {
            let baseline = PerformanceBaseline::from_metrics(operation.clone(), &operation_metrics);
            let mut baselines = self.baselines.write().await;
            baselines.push(baseline);
        }
    }

    /// 获取性能基线
    pub async fn get_baseline(&self, operation: &str) -> Option<PerformanceBaseline> {
        let baselines = self.baselines.read().await;
        baselines.iter().find(|b| b.operation == operation).cloned()
    }

    /// 检查性能回归
    pub async fn check_regression(&self, operation: &str, threshold_ms: f64) -> bool {
        if let Some(baseline) = self.get_baseline(operation).await {
            baseline.exceeds_threshold(threshold_ms)
        } else {
            false
        }
    }

    /// 获取所有指标
    pub async fn get_metrics(&self) -> Vec<PerformanceMetrics> {
        self.metrics.read().await.clone()
    }

    /// 获取所有基线
    pub async fn get_baselines(&self) -> Vec<PerformanceBaseline> {
        self.baselines.read().await.clone()
    }

    /// 生成性能报告
    pub async fn generate_report(&self) -> PerformanceReport {
        let metrics = self.metrics.read().await;
        let baselines = self.baselines.read().await;

        let mut operation_stats = std::collections::HashMap::new();
        for metric in metrics.iter() {
            operation_stats
                .entry(metric.operation.clone())
                .or_insert_with(Vec::new)
                .push(metric.duration_ms);
        }

        let mut summary = Vec::new();
        for (operation, durations) in operation_stats {
            let avg = durations.iter().sum::<f64>() / durations.len() as f64;
            let baseline = baselines.iter().find(|b| b.operation == operation);
            let rating = baseline
                .map(|b| b.get_rating())
                .unwrap_or(PerformanceRating::Fair);

            summary.push(OperationSummary {
                operation,
                avg_duration_ms: avg,
                sample_count: durations.len(),
                rating,
            });
        }

        PerformanceReport { summary }
    }

    /// 清理旧指标
    pub async fn cleanup_old_metrics(&self, keep_count: usize) {
        let mut metrics = self.metrics.write().await;
        if metrics.len() > keep_count {
            let remove_count = metrics.len() - keep_count;
            metrics.drain(0..remove_count);
        }
    }
}

impl Default for PerformanceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// 性能计时器
pub struct PerformanceTimer {
    start: Instant,
}

impl PerformanceTimer {
    /// 获取经过的时间（毫秒）
    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// 获取经过的时间（微秒）
    pub fn elapsed_us(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1_000_000.0
    }

    /// 获取经过的时间（纳秒）
    pub fn elapsed_ns(&self) -> u128 {
        self.start.elapsed().as_nanos()
    }
}

/// 性能报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub summary: Vec<OperationSummary>,
}

impl PerformanceReport {
    /// 获取最慢的操作
    pub fn slowest_operations(&self, count: usize) -> Vec<&OperationSummary> {
        let mut ops = self.summary.iter().collect::<Vec<_>>();
        ops.sort_by(|a, b| b.avg_duration_ms.partial_cmp(&a.avg_duration_ms).unwrap());
        ops.into_iter().take(count).collect()
    }

    /// 获取最快的操作
    pub fn fastest_operations(&self, count: usize) -> Vec<&OperationSummary> {
        let mut ops = self.summary.iter().collect::<Vec<_>>();
        ops.sort_by(|a, b| a.avg_duration_ms.partial_cmp(&b.avg_duration_ms).unwrap());
        ops.into_iter().take(count).collect()
    }

    /// 获取性能评级分布
    pub fn rating_distribution(&self) -> std::collections::HashMap<PerformanceRating, usize> {
        let mut dist = std::collections::HashMap::new();
        for op in &self.summary {
            *dist.entry(op.rating).or_insert(0) += 1;
        }
        dist
    }
}

/// 操作摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationSummary {
    pub operation: String,
    pub avg_duration_ms: f64,
    pub sample_count: usize,
    pub rating: PerformanceRating,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_metrics() {
        let metric = PerformanceMetrics::new("test_op".to_string(), 5.5);
        assert_eq!(metric.operation, "test_op");
        assert_eq!(metric.duration_ms, 5.5);
    }

    #[test]
    fn test_performance_baseline() {
        let metrics = vec![
            PerformanceMetrics::new("op".to_string(), 5.0),
            PerformanceMetrics::new("op".to_string(), 10.0),
            PerformanceMetrics::new("op".to_string(), 15.0),
        ];

        let baseline = PerformanceBaseline::from_metrics("op".to_string(), &metrics);
        assert_eq!(baseline.avg_duration_ms, 10.0);
        assert_eq!(baseline.min_duration_ms, 5.0);
        assert_eq!(baseline.max_duration_ms, 15.0);
        assert_eq!(baseline.sample_count, 3);
    }

    #[test]
    fn test_performance_rating() {
        let baseline_excellent = PerformanceBaseline {
            operation: "op".to_string(),
            avg_duration_ms: 0.5,
            min_duration_ms: 0.5,
            max_duration_ms: 0.5,
            std_dev_ms: 0.0,
            sample_count: 1,
        };
        assert_eq!(baseline_excellent.get_rating(), PerformanceRating::Excellent);

        let baseline_good = PerformanceBaseline {
            operation: "op".to_string(),
            avg_duration_ms: 5.0,
            min_duration_ms: 5.0,
            max_duration_ms: 5.0,
            std_dev_ms: 0.0,
            sample_count: 1,
        };
        assert_eq!(baseline_good.get_rating(), PerformanceRating::Good);

        let baseline_critical = PerformanceBaseline {
            operation: "op".to_string(),
            avg_duration_ms: 2000.0,
            min_duration_ms: 2000.0,
            max_duration_ms: 2000.0,
            std_dev_ms: 0.0,
            sample_count: 1,
        };
        assert_eq!(baseline_critical.get_rating(), PerformanceRating::Critical);
    }

    #[tokio::test]
    async fn test_performance_monitor() {
        let monitor = PerformanceMonitor::new();

        // 记录指标
        monitor
            .record(PerformanceMetrics::new("op1".to_string(), 5.0))
            .await;
        monitor
            .record(PerformanceMetrics::new("op1".to_string(), 10.0))
            .await;
        monitor
            .record(PerformanceMetrics::new("op2".to_string(), 20.0))
            .await;

        // 建立基线
        monitor.establish_baseline("op1".to_string()).await;
        monitor.establish_baseline("op2".to_string()).await;

        // 获取基线
        let baseline = monitor.get_baseline("op1").await;
        assert!(baseline.is_some());
        assert_eq!(baseline.unwrap().avg_duration_ms, 7.5);

        // 检查回归
        let regression = monitor.check_regression("op1", 10.0).await;
        assert!(!regression);

        let regression = monitor.check_regression("op1", 5.0).await;
        assert!(regression);
    }

    #[test]
    fn test_performance_timer() {
        let timer = PerformanceTimer {
            start: Instant::now() - Duration::from_millis(10),
        };

        let elapsed_ms = timer.elapsed_ms();
        assert!(elapsed_ms >= 10.0);
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            operation: String::new(),
            duration_ms: 0.0,
            memory_bytes: None,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}
