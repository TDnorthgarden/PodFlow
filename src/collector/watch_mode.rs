//! Watch Mode - Real-time container data monitoring
//!
//! Provides real-time monitoring of container data by directly accessing
//! the NRI mapping table instead of using API calls.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use serde_json::json;

use super::nri_v3::NriV3;

/// Watch mode configuration
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Target pod UID to monitor
    pub pod_uid: Option<String>,
    /// Namespace to filter
    pub namespace: Option<String>,
    /// Monitoring interval in seconds
    pub interval_secs: u64,
    /// Maximum number of iterations
    pub max_iterations: Option<u32>,
    /// Whether to show detailed output
    pub detailed: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            pod_uid: None,
            namespace: None,
            interval_secs: 5,
            max_iterations: None,
            detailed: false,
        }
    }
}

/// Real-time container data watcher
pub struct ContainerWatcher {
    nri_v3: Arc<NriV3>,
    config: WatchConfig,
}

impl ContainerWatcher {
    /// Create a new container watcher
    pub fn new(nri_v3: Arc<NriV3>, config: WatchConfig) -> Self {
        Self { nri_v3, config }
    }

    /// Start watching containers with real data
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("🐿️  Nuts Observer Real-time Watch Mode");
        println!("{}", "─".repeat(80));

        let mut interval = interval(Duration::from_secs(self.config.interval_secs));
        let mut iteration = 0u32;

        loop {
            iteration += 1;
            
            // Get real-time data from NRI mapping table
            let watch_data = self.collect_real_time_data().await?;
            
            // Display the data
            self.display_watch_data(&watch_data, iteration).await;

            // Check if we should stop
            if let Some(max_iter) = self.config.max_iterations {
                if iteration >= max_iter {
                    println!("✅ Watch completed ({} iterations)", iteration);
                    break;
                }
            }

            interval.tick().await;
        }

        Ok(())
    }

    /// Collect real-time data from NRI mapping table
    async fn collect_real_time_data(&self) -> Result<WatchData, Box<dyn std::error::Error + Send + Sync>> {
        let table = self.nri_v3.table();
        let metrics = self.nri_v3.metrics();

        // Get table statistics
        let pod_count = table.pod_count();
        let container_count = table.container_count();
        let cgroup_count = table.cgroup_count();
        let pid_count = table.pid_count();

        // Get target pod details if specified
        let target_pod_info = if let Some(ref pod_uid) = self.config.pod_uid {
            table.get_pod_details(pod_uid).map(|(pod, containers)| {
                json!({
                    "pod": pod,
                    "containers": containers,
                    "container_count": containers.len()
                })
            })
        } else {
            None
        };

        // Get metrics summary
        let metrics_summary = metrics.export_prometheus();
        
        // Get recent events from metrics
        let recent_events = self.get_recent_events_count(&metrics_summary).await;

        Ok(WatchData {
            timestamp: chrono::Utc::now().to_rfc3339(),
            table_stats: TableStats {
                pod_count,
                container_count,
                cgroup_count,
                pid_count,
            },
            target_pod_info,
            recent_events,
            metrics_summary: metrics_summary.chars().count(), // Just count chars for summary
        })
    }

    /// Get recent events count from metrics
    async fn get_recent_events_count(&self, metrics_summary: &str) -> u64 {
        // Parse metrics to get recent events count
        // This is a simplified implementation - in production, you'd parse Prometheus format
        if metrics_summary.contains("nri_events_total") {
            // Extract the count from metrics (simplified)
            metrics_summary
                .lines()
                .find(|line| line.contains("nri_events_total"))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        } else {
            0
        }
    }

    /// Display watch data with enhanced formatting
    async fn display_watch_data(&self, data: &WatchData, iteration: u32) {
        let timestamp = chrono::DateTime::parse_from_rfc3339(&data.timestamp)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .format("%H:%M:%S")
            .to_string();

        println!("📊 Iteration {} | {}", iteration, timestamp);
        
        // Display table statistics
        println!("  🏗️  Infrastructure:");
        println!("    Pods: {} | Containers: {} | Cgroups: {} | PIDs: {}",
            data.table_stats.pod_count,
            data.table_stats.container_count,
            data.table_stats.cgroup_count,
            data.table_stats.pid_count
        );

        // Display target pod info if available
        if let Some(ref pod_info) = data.target_pod_info {
            println!("  🎯 Target Pod:");
            if self.config.detailed {
                println!("    {}", serde_json::to_string_pretty(pod_info).unwrap_or_default());
            } else {
                if let Some(container_count) = pod_info.get("container_count").and_then(|v| v.as_u64()) {
                    println!("    Containers: {}", container_count);
                }
            }
        }

        // Display events summary
        println!("  📈 Events: {} recent | Metrics: {} chars",
            data.recent_events,
            data.metrics_summary
        );

        println!("{}", "─".repeat(40));
    }
}

/// Watch data structure
#[derive(Debug, Clone)]
pub struct WatchData {
    pub timestamp: String,
    pub table_stats: TableStats,
    pub target_pod_info: Option<serde_json::Value>,
    pub recent_events: u64,
    pub metrics_summary: usize,
}

#[derive(Debug, Clone)]
pub struct TableStats {
    pub pod_count: usize,
    pub container_count: usize,
    pub cgroup_count: usize,
    pub pid_count: usize,
}

/// Convenience function to start watch mode with NRI V3
pub async fn start_real_time_watch(
    nri_v3: Arc<NriV3>,
    config: WatchConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let watcher = ContainerWatcher::new(nri_v3, config);
    watcher.start().await
}
