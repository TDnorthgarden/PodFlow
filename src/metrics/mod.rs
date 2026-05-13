//! 统一指标收集模块
//!
//! 提供完整的系统可观测性指标：
//! - NRI事件指标：映射表大小、事件处理速率、归属查询性能
//! - 性能指标：操作执行时间、内存使用、性能基线对比
//! - 持久化指标：快照状态、恢复统计
//! - 系统指标：连接状态、版本控制、错误率

pub mod performance;

pub use performance::{
    PerformanceMetrics, PerformanceBaseline, PerformanceRating, PerformanceMonitor,
    PerformanceTimer, PerformanceReport, OperationSummary,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::collections::HashMap;
use dashmap::DashMap;

/// 统一指标收集器
#[derive(Debug)]
pub struct UnifiedMetrics {
    /// NRI 事件指标
    pub nri: NriEventMetrics,
    
    /// 性能指标
    pub performance: PerformanceMetrics,
    
    /// 系统指标
    pub system: SystemMetrics,
}

/// 系统指标
#[derive(Debug)]
pub struct SystemMetrics {
    /// 错误计数
    pub errors_total: AtomicU64,
    /// 最后错误时间戳
    pub last_error_timestamp: AtomicU64,
}

/// NRI 事件相关指标
#[derive(Debug)]
pub struct NriEventMetrics {
    /// 事件计数器
    events_total: AtomicU64,
    events_by_type: DashMap<String, AtomicU64>,

    /// 处理延迟统计（微秒）
    event_processing_duration_us: AtomicU64,
    event_processing_count: AtomicU64,

    /// 归属查询统计
    attribution_queries_total: AtomicU64,
    attribution_cache_hits: AtomicU64,
    attribution_cache_misses: AtomicU64,

    /// 查询延迟（微秒）
    attribution_duration_us: AtomicU64,
    attribution_query_count: AtomicU64,

    /// 映射表统计
    mapping_table_pods: AtomicU64,
    mapping_table_containers: AtomicU64,
    mapping_table_cgroups: AtomicU64,
    mapping_table_pids: AtomicU64,

    /// 批量处理统计
    batch_flushes_total: AtomicU64,
    batch_events_processed: AtomicU64,
    batch_queue_depth: AtomicU64,

    // --- Containerd NRI 连接统计 ---
    /// 事件成功处理数
    events_success: AtomicU64,
    /// 事件处理失败数
    events_failed: AtomicU64,
    /// 注册重试次数
    retry_count: AtomicU64,
    /// 熔断器打开次数
    circuit_breaker_opened: AtomicU64,
    /// 当前连接状态 (1=connected, 0=disconnected)
    connected: AtomicU32,
}

impl Default for UnifiedMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            errors_total: AtomicU64::new(0),
            last_error_timestamp: AtomicU64::new(0),
        }
    }
}

impl Default for NriEventMetrics {
    fn default() -> Self {
        Self {
            events_total: AtomicU64::new(0),
            events_by_type: DashMap::new(),
            event_processing_duration_us: AtomicU64::new(0),
            event_processing_count: AtomicU64::new(0),
            attribution_queries_total: AtomicU64::new(0),
            attribution_cache_hits: AtomicU64::new(0),
            attribution_cache_misses: AtomicU64::new(0),
            attribution_duration_us: AtomicU64::new(0),
            attribution_query_count: AtomicU64::new(0),
            mapping_table_pods: AtomicU64::new(0),
            mapping_table_containers: AtomicU64::new(0),
            mapping_table_cgroups: AtomicU64::new(0),
            mapping_table_pids: AtomicU64::new(0),
            batch_flushes_total: AtomicU64::new(0),
            batch_events_processed: AtomicU64::new(0),
            batch_queue_depth: AtomicU64::new(0),
            events_success: AtomicU64::new(0),
            events_failed: AtomicU64::new(0),
            retry_count: AtomicU64::new(0),
            circuit_breaker_opened: AtomicU64::new(0),
            connected: AtomicU32::new(0),
        }
    }
}

impl UnifiedMetrics {
    /// 创建新的指标收集器
    pub fn new() -> Self {
        Self {
            nri: NriEventMetrics::default(),
            performance: PerformanceMetrics::default(),
            system: SystemMetrics::default(),
        }
    }

    /// 记录containerd事件
    pub fn record_containerd_event(&self, success: bool) {
        if success {
            self.nri.events_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.nri.events_failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 设置连接状态
    pub fn set_connected(&self, connected: bool) {
        self.nri.connected.store(connected as u32, Ordering::Relaxed);
    }

    /// 记录重试
    pub fn record_retry(&self) {
        self.nri.retry_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 更新映射表大小指标
    pub fn update_mapping_table_size(&self, pods: u64, containers: u64, cgroups: u64, pids: u64) {
        self.nri.mapping_table_pods.store(pods, Ordering::Relaxed);
        self.nri.mapping_table_containers.store(containers, Ordering::Relaxed);
        self.nri.mapping_table_cgroups.store(cgroups, Ordering::Relaxed);
        self.nri.mapping_table_pids.store(pids, Ordering::Relaxed);
    }

    /// 导出为JSON格式
    pub fn export_json(&self) -> serde_json::Value {
        let events_by_type: std::collections::HashMap<String, u64> = self.nri.events_by_type
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().load(Ordering::Relaxed)))
            .collect();

        serde_json::json!({
            "events": {
                "total": self.nri.events_total.load(Ordering::Relaxed),
                "by_type": events_by_type,
                "success": self.nri.events_success.load(Ordering::Relaxed),
                "failed": self.nri.events_failed.load(Ordering::Relaxed),
                "processing_duration_us": self.nri.event_processing_duration_us.load(Ordering::Relaxed),
                "processing_count": self.nri.event_processing_count.load(Ordering::Relaxed),
            },
            "attribution": {
                "queries_total": self.nri.attribution_queries_total.load(Ordering::Relaxed),
                "cache_hits": self.nri.attribution_cache_hits.load(Ordering::Relaxed),
                "cache_misses": self.nri.attribution_cache_misses.load(Ordering::Relaxed),
                "duration_us": self.nri.attribution_duration_us.load(Ordering::Relaxed),
                "query_count": self.nri.attribution_query_count.load(Ordering::Relaxed),
            },
            "mapping_table": {
                "pods": self.nri.mapping_table_pods.load(Ordering::Relaxed),
                "containers": self.nri.mapping_table_containers.load(Ordering::Relaxed),
                "cgroups": self.nri.mapping_table_cgroups.load(Ordering::Relaxed),
                "pids": self.nri.mapping_table_pids.load(Ordering::Relaxed),
            },
            "batch": {
                "flushes_total": self.nri.batch_flushes_total.load(Ordering::Relaxed),
                "events_processed": self.nri.batch_events_processed.load(Ordering::Relaxed),
                "queue_depth": self.nri.batch_queue_depth.load(Ordering::Relaxed),
            },
            "connection": {
                "retry_count": self.nri.retry_count.load(Ordering::Relaxed),
                "circuit_breaker_opened": self.nri.circuit_breaker_opened.load(Ordering::Relaxed),
                "connected": self.nri.connected.load(Ordering::Relaxed),
            },
            "system": {
                "errors_total": self.system.errors_total.load(Ordering::Relaxed),
                "last_error_timestamp": self.system.last_error_timestamp.load(Ordering::Relaxed),
            }
        })
    }

    /// 导出为Prometheus格式文本
    pub fn export_prometheus(&self) -> String {
        let mut output = String::with_capacity(4096);

        // 帮助信息
        output.push_str("# HELP nri_events_total Total number of NRI events processed\n");
        output.push_str("# TYPE nri_events_total counter\n");
        output.push_str(&format!(
            "nri_events_total {}\n",
            self.nri.events_total.load(Ordering::Relaxed)
        ));

        // 按类型的事件计数
        for entry in self.nri.events_by_type.iter() {
            output.push_str(&format!(
                "nri_events_total{{type=\"{}\"}} {}\n",
                entry.key(),
                entry.value().load(Ordering::Relaxed)
            ));
        }

        // 成功和失败计数
        output.push_str("# HELP nri_events_success Total number of successful NRI events\n");
        output.push_str("# TYPE nri_events_success counter\n");
        output.push_str(&format!(
            "nri_events_success {}\n",
            self.nri.events_success.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP nri_events_failed Total number of failed NRI events\n");
        output.push_str("# TYPE nri_events_failed counter\n");
        output.push_str(&format!(
            "nri_events_failed {}\n",
            self.nri.events_failed.load(Ordering::Relaxed)
        ));

        // 归属查询统计
        output.push_str("# HELP nri_attribution_queries_total Total number of attribution queries\n");
        output.push_str("# TYPE nri_attribution_queries_total counter\n");
        output.push_str(&format!(
            "nri_attribution_queries_total {}\n",
            self.nri.attribution_queries_total.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP nri_attribution_cache_hits Total number of attribution cache hits\n");
        output.push_str("# TYPE nri_attribution_cache_hits counter\n");
        output.push_str(&format!(
            "nri_attribution_cache_hits {}\n",
            self.nri.attribution_cache_hits.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP nri_attribution_cache_misses Total number of attribution cache misses\n");
        output.push_str("# TYPE nri_attribution_cache_misses counter\n");
        output.push_str(&format!(
            "nri_attribution_cache_misses {}\n",
            self.nri.attribution_cache_misses.load(Ordering::Relaxed)
        ));

        // 映射表大小
        output.push_str("# HELP nri_mapping_table_pods Number of pods in mapping table\n");
        output.push_str("# TYPE nri_mapping_table_pods gauge\n");
        output.push_str(&format!(
            "nri_mapping_table_pods {}\n",
            self.nri.mapping_table_pods.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP nri_mapping_table_containers Number of containers in mapping table\n");
        output.push_str("# TYPE nri_mapping_table_containers gauge\n");
        output.push_str(&format!(
            "nri_mapping_table_containers {}\n",
            self.nri.mapping_table_containers.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP nri_mapping_table_cgroups Number of cgroups in mapping table\n");
        output.push_str("# TYPE nri_mapping_table_cgroups gauge\n");
        output.push_str(&format!(
            "nri_mapping_table_cgroups {}\n",
            self.nri.mapping_table_cgroups.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP nri_mapping_table_pids Number of PIDs in mapping table\n");
        output.push_str("# TYPE nri_mapping_table_pids gauge\n");
        output.push_str(&format!(
            "nri_mapping_table_pids {}\n",
            self.nri.mapping_table_pids.load(Ordering::Relaxed)
        ));

        // 系统错误
        output.push_str("# HELP nri_system_errors_total Total number of system errors\n");
        output.push_str("# TYPE nri_system_errors_total counter\n");
        output.push_str(&format!(
            "nri_system_errors_total {}\n",
            self.system.errors_total.load(Ordering::Relaxed)
        ));

        output
    }
}

/// 创建指标收集器实例
pub fn create_metrics() -> UnifiedMetrics {
    UnifiedMetrics::new()
}
