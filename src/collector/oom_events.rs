//! OOM (Out of Memory) 事件监听模块
//!
//! 监听内核 OOM Kill 事件，自动触发故障诊断

use crate::types::error::NutsError;
use crate::collector::nri_mapping::{NriMappingTable, AttributionInfo};
use serde::Deserialize;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing;

/// OOM 事件结构
#[derive(Debug, Clone, Deserialize)]
pub struct OomEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub pid: u32,
    pub comm: String,
    pub victim_pid: Option<u32>,
    pub victim_comm: Option<String>,
    pub ts_ms: u64,
}

/// OOM 事件监听器配置
#[derive(Debug, Clone)]
pub struct OomListenerConfig {
    /// 是否启用 OOM 监听
    pub enabled: bool,
    /// 触发诊断的证据类型
    pub evidence_types: Vec<String>,
    /// 采集时间窗（秒）
    pub collection_window_secs: u64,
    /// 冷却期（秒，避免重复触发）
    pub cooldown_secs: u64,
    /// 服务地址（用于触发诊断）
    pub server_url: String,
}

impl Default for OomListenerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            evidence_types: vec!["block_io".to_string(), "syscall_latency".to_string()],
            collection_window_secs: 10,
            cooldown_secs: 60,
            server_url: "http://localhost:3000".to_string(),
        }
    }
}

/// OOM 事件监听器
pub struct OomEventListener {
    config: OomListenerConfig,
    nri_table: Arc<NriMappingTable>,
    last_trigger: std::sync::Mutex<std::collections::HashMap<String, u64>>,
}

impl OomEventListener {
    /// 创建新的 OOM 监听器
    pub fn new(config: OomListenerConfig, nri_table: Arc<NriMappingTable>) -> Self {
        Self {
            config,
            nri_table,
            last_trigger: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// 启动 OOM 事件监听
    pub async fn start(&self) {
        if !self.config.enabled {
            tracing::info!("OOM event listener is disabled");
            return;
        }

        tracing::info!("Starting OOM event listener...");

        let (tx, mut rx) = mpsc::channel::<OomEvent>(100);

        // 启动 bpftrace 监听 OOM 事件
        let script = r#"
            tracepoint:oom:oom_score_adj_update,
            kprobe:oom_kill_process
            {
                printf("{\"type\":\"oom_kill\",\"pid\":%d,\"comm\":\"%s\",\"ts_ms\":%u}\n",
                    pid, comm, nsecs / 1000000);
            }
        "#;

        let mut child = match Command::new("bpftrace")
            .arg("-e")
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to start bpftrace for OOM monitoring: {}", e);
                return;
            }
        };

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                tracing::error!("Failed to capture stdout from bpftrace process");
                return;
            }
        };
        let reader = BufReader::new(stdout);
        let tx_clone = tx.clone();

        // 在独立线程中读取 bpftrace 输出
        std::thread::spawn(move || {
            for line in reader.lines() {
                if let Ok(line_str) = line {
                    if let Ok(event) = serde_json::from_str::<OomEvent>(&line_str) {
                        if event.event_type == "oom_kill" {
                            let _ = tx_clone.try_send(event);
                        }
                    }
                }
            }
        });

        // 处理 OOM 事件
        while let Some(event) = rx.recv().await {
            tracing::warn!(
                "OOM Kill detected: pid={}, comm={}",
                event.pid, event.comm
            );

            self.handle_oom_event(event).await;
        }

        let _ = child.kill();
    }

    /// 处理单个 OOM 事件
    async fn handle_oom_event(&self, event: OomEvent) -> Result<(), NutsError> {
        // 通过 PID 查询归属信息
        let attribution = match self.nri_table.resolve_attribution(None, None, Some(event.pid)) {
            Ok(info) => info,
            Err(e) => {
                tracing::warn!("Could not resolve attribution for PID {}: {:?}", event.pid, e);
                // 即使没有归属信息，也尝试触发诊断
                AttributionInfo {
                    pod_uid: None,
                    container_id: None,
                    cgroup_id: String::new(),
                    status: crate::collector::nri_mapping::AttributionStatus::Unknown,
                    confidence: 0.0,
                    source: crate::collector::nri_mapping::AttributionSource::Uncertain,
                    mapping_version: "0".to_string(),
                }
            }
        };

        let scope_key = if let Some(ref pod_uid) = attribution.pod_uid {
            format!("oom-{}-{}", pod_uid, event.pid)
        } else {
            format!("oom-pid-{}", event.pid)
        };

        // 检查冷却期
        if self.is_in_cooldown(&scope_key, event.ts_ms)? {
            tracing::info!("OOM event for {} is in cooldown, skipping", scope_key);
            return Ok(());
        }

        // 记录触发时间
        self.record_trigger(&scope_key, event.ts_ms)?;

        // 触发诊断
        self.trigger_diagnosis(&event, &attribution).await;
        Ok(())
    }

    /// 检查是否在冷却期
    fn is_in_cooldown(&self, key: &str, now_ms: u64) -> Result<bool, NutsError> {
        let cooldown_ms = self.config.cooldown_secs * 1000;
        let triggers = self.last_trigger.lock().map_err(|_| NutsError::lock_error("Failed to acquire lock"))?;
        
        if let Some(&last_time) = triggers.get(key) {
            Ok((now_ms - last_time) < cooldown_ms)
        } else {
            Ok(false)
        }
    }

    /// 记录触发时间
    fn record_trigger(&self, key: &str, ts_ms: u64) -> Result<(), NutsError> {
        let mut triggers = self.last_trigger.lock().map_err(|_| NutsError::lock_error("Failed to acquire lock"))?;
        triggers.insert(key.to_string(), ts_ms);
        Ok(())
    }

    /// 触发诊断请求
    async fn trigger_diagnosis(&self, event: &OomEvent, attribution: &AttributionInfo) {
        let now = chrono::Utc::now().timestamp_millis();
        let window_ms = self.config.collection_window_secs as i64 * 1000;

        let request = serde_json::json!({
            "trigger_type": "oom_event",
            "target": {
                "pod_uid": attribution.pod_uid,
                "namespace": "default", // OOM 事件通常需要结合运行时信息
                "pod_name": attribution.pod_uid.as_ref().map(|uid| format!("oom-pod-{}", uid)),
                "cgroup_id": if attribution.cgroup_id.is_empty() { None } else { Some(&attribution.cgroup_id) }
            },
            "time_window": {
                "start_time_ms": now - window_ms,
                "end_time_ms": now
            },
            "collection_options": {
                "requested_evidence_types": self.config.evidence_types
            },
            "idempotency_key": format!("oom-{}-{}", event.pid, now),
            "trigger_context": {
                "event_type": "oom_kill",
                "pid": event.pid,
                "comm": event.comm,
                "attribution": {
                    "pod_uid": attribution.pod_uid,
                    "container_id": attribution.container_id,
                    "confidence": attribution.confidence
                }
            }
        });

        let url = format!("{}/v1/diagnostics:trigger", self.config.server_url);
        
        tracing::info!("Triggering OOM diagnosis for PID {} at {}", event.pid, url);

        match reqwest::Client::new()
            .post(&url)
            .json(&request)
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    tracing::info!("OOM diagnosis triggered successfully for PID {}", event.pid);
                } else {
                    tracing::error!(
                        "Failed to trigger OOM diagnosis: HTTP {}",
                        response.status()
                    );
                }
            }
            Err(e) => {
                tracing::error!("Failed to send OOM diagnosis request: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oom_event_deserialize() -> Result<(), Box<dyn std::error::Error>> {
        let json = r#"{"type":"oom_kill","pid":1234,"comm":"java","ts_ms":1700000000000}"#;
        let event: OomEvent = serde_json::from_str(json).map_err(NutsError::Json)?;
        assert_eq!(event.pid, 1234);
        assert_eq!(event.comm, "java");
        Ok(())
    }

    #[test]
    fn test_cooldown_mechanism() {
        let config = OomListenerConfig::default();
        let nri_table = Arc::new(NriMappingTable::new());
        let listener = OomEventListener::new(config, nri_table);

        // 第一次触发
        assert!(!listener.is_in_cooldown("test-key", 1000).unwrap());
        listener.record_trigger("test-key", 1000).unwrap();

        // 冷却期内
        assert!(listener.is_in_cooldown("test-key", 2000).unwrap());

        // 冷却期后 (默认 60秒 = 60000ms)
        assert!(!listener.is_in_cooldown("test-key", 62000).unwrap());
    }
}
