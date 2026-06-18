use crate::types::error::PodflowError;
use crate::types::evidence::*;
use crate::collector::nri_mapping_v2::NriMappingTableV2;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct TcpConnectStagesCollectorConfig {
    pub task_id: String,
    pub time_window: TimeWindow,
    pub pod: Option<PodInfo>,
    pub container_id: Option<String>,
    pub cgroup_id: Option<String>,
    pub network_target: Option<NetworkTarget>,
    pub requested_metrics: Vec<String>,
    pub requested_events: Vec<String>,
    /// NRI 映射表引用，用于查询归属
    pub nri_table: Option<Arc<NriMappingTableV2>>,
    /// 目标 PID 列表（BPFtrace 采集时进行 PID 过滤）
    pub target_pids: Option<Vec<u32>>,
}

fn make_scope_key(pod_uid: Option<&str>, cgroup_id: Option<&str>) -> String {
    let u = pod_uid.unwrap_or("");
    let c = cgroup_id.unwrap_or("");
    let mut hasher = Sha256::new();
    hasher.update(format!("{u}|{c}"));
    format!("{:x}", hasher.finalize())
}

fn make_evidence_id(task_id: &str, evidence_type: &str, collection_id: &str, scope_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{task_id}|{evidence_type}|{collection_id}|{scope_key}"));
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct BpftraceTcpStagesEvent {
    #[serde(rename = "type")]
    event_type: String,
    pid: Option<u32>,
    comm: Option<String>,
    stage: Option<String>,
    duration_us: Option<u64>,
    error_code: Option<i32>,
    ts_ms: Option<u64>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

/// 运行 TCP 连接阶段细分采集
pub fn run_tcp_connect_stages_collect(cfg: TcpConnectStagesCollectorConfig) -> Result<Evidence, PodflowError> {
    let scope_key = make_scope_key(
        cfg.pod.as_ref().and_then(|p| p.uid.as_deref()),
        cfg.cgroup_id.as_deref(),
    );
    
    let collection_id = uuid::Uuid::new_v4().to_string();
    let probe_id = "tcp_connect_stages.bt";
    
    // 计算采集持续时间
    let duration_ms = cfg.time_window.end_time_ms - cfg.time_window.start_time_ms;
    let duration_sec = (duration_ms / 1000).clamp(1, 60) as u64; // 限制 1-60 秒
    
    let script_path = "scripts/bpftrace/network/tcp_connect_stages.bt";
    
    // 存储采集结果
    let stages_events = Arc::new(Mutex::new(Vec::<BpftraceTcpStagesEvent>::new()));
    let stage_counts = Arc::new(Mutex::new(HashMap::<String, u32>::new()));
    let stage_durations = Arc::new(Mutex::new(HashMap::<String, Vec<u64>>::new()));
    let errors = Arc::new(Mutex::new(Vec::<CollectionError>::new()));
    
    let events_clone = stages_events.clone();
    let counts_clone = stage_counts.clone();
    let durations_clone = stage_durations.clone();
    
    // 构建 bpftrace 命令
    let mut cmd = Command::new("sudo");
    cmd.arg("bpftrace").arg(script_path);
    
    // 添加目标 PID 过滤（如果指定了）
    if let Some(ref pids) = cfg.target_pids {
        for pid in pids {
            cmd.arg("-p").arg(pid.to_string());
        }
    }
    
    // 启动 bpftrace 采集
    let mut child = match cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let mut errors_guard = errors.lock().map_err(|_| PodflowError::lock_error("Failed to acquire lock"))?;
            errors_guard.push(CollectionError {
                code: "BPFTRACE_SCRIPT_LOAD_FAILED".into(),
                message: format!("Failed to start bpftrace: {}", e),
                retryable: Some(false),
                detail: None,
            });
            
            return Ok(build_evidence(
                cfg, scope_key, collection_id, probe_id,
                Vec::new(), HashMap::new(), HashMap::new(), Vec::new(), "failed",
            ));
        }
    };
    
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return Err(PodflowError::internal("Failed to capture stdout from bpftrace process")),
    };
    let reader = BufReader::new(stdout);
    
    // 采集超时控制
    let start_time = Instant::now();
    let timeout = Duration::from_secs(duration_sec);
    
    // 解析 bpftrace 输出
    for line in reader.lines() {
        if start_time.elapsed() > timeout {
            break;
        }
        
        let line_str = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        
        // 解析 JSON 输出
        if let Ok(event) = serde_json::from_str::<BpftraceTcpStagesEvent>(&line_str) {
            match event.event_type.as_str() {
                "tcp_attempt_stage" => {
                    let stage = event.stage.clone().unwrap_or_else(|| "unknown".to_string());
                    let duration = event.duration_us.unwrap_or(0);
                    
                    // 记录阶段事件
                    {
                        let mut events = events_clone.lock().map_err(|_| PodflowError::lock_error("Failed to acquire lock"))?;
                        events.push(event);
                    }
                    
                    // 统计阶段计数
                    {
                        let mut counts = counts_clone.lock().map_err(|_| PodflowError::lock_error("Failed to acquire lock"))?;
                        *counts.entry(stage.clone()).or_insert(0) += 1;
                    }
                    
                    // 记录阶段持续时间
                    if duration > 0 {
                        let mut durations = durations_clone.lock().map_err(|_| PodflowError::lock_error("Failed to acquire lock"))?;
                        durations.entry(stage.clone()).or_insert_with(Vec::new).push(duration);
                    }
                }
                "stats" => {
                    // 统计信息事件，可以用于监控
                    tracing::debug!("TCP stages stats: {}", line_str);
                }
                _ => {}
            }
        }
    }
    
    // 停止 bpftrace
    let _ = child.kill();
    
    // 收集结果
    let events = Arc::try_unwrap(stages_events)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    let counts = Arc::try_unwrap(stage_counts)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    let durations = Arc::try_unwrap(stage_durations)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    let errors = Arc::try_unwrap(errors)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    
    let collection_status = if errors.is_empty() { "success" } else { "partial" };
    
    Ok(build_evidence(
        cfg, scope_key, collection_id, probe_id,
        events, counts, durations, errors, collection_status,
    ))
}

fn build_evidence(
    cfg: TcpConnectStagesCollectorConfig,
    scope_key: String,
    collection_id: String,
    probe_id: &str,
    events: Vec<BpftraceTcpStagesEvent>,
    stage_counts: HashMap<String, u32>,
    stage_durations: HashMap<String, Vec<u64>>,
    errors: Vec<CollectionError>,
    collection_status: &str,
) -> Evidence {
    let mut metric_summary = HashMap::new();
    
    // 计算各阶段的统计信息
    for (stage, count) in &stage_counts {
        metric_summary.insert(format!("tcp_connect_{}_count", stage), *count as f64);
    }
    
    // 计算各阶段的延迟统计
    for (stage, durations) in &stage_durations {
        if !durations.is_empty() {
            let mut sorted = durations.clone();
            sorted.sort();
            
            let len = sorted.len();
            let sum: u64 = sorted.iter().sum();
            metric_summary.insert(format!("tcp_connect_{}_avg_us", stage), (sum / len as u64) as f64);
            metric_summary.insert(format!("tcp_connect_{}_p50_us", stage), sorted[len / 2] as f64);
            metric_summary.insert(format!("tcp_connect_{}_p95_us", stage), sorted[(len as f64 * 0.95) as usize] as f64);
            metric_summary.insert(format!("tcp_connect_{}_p99_us", stage), sorted[(len as f64 * 0.99) as usize] as f64);
            metric_summary.insert(format!("tcp_connect_{}_max_us", stage), *sorted.last().unwrap() as f64);
        }
    }
    
    // 计算总体统计
    let total_attempts: u32 = stage_counts.values().sum();
    let successful_connects = stage_counts.get("connect_success").unwrap_or(&0);
    let failed_connects = stage_counts.get("connect_failed").unwrap_or(&0);
    let timeout_connects = stage_counts.get("connect_timeout").unwrap_or(&0);
    let reset_connects = stage_counts.get("reset_received").unwrap_or(&0);
    
    metric_summary.insert("tcp_connect_total_attempts".to_string(), total_attempts as f64);
    metric_summary.insert("tcp_connect_success_count".to_string(), *successful_connects as f64);
    metric_summary.insert("tcp_connect_failed_count".to_string(), *failed_connects as f64);
    metric_summary.insert("tcp_connect_timeout_count".to_string(), *timeout_connects as f64);
    metric_summary.insert("tcp_connect_reset_count".to_string(), *reset_connects as f64);
    
    // 计算成功率
    if total_attempts > 0 {
        let success_rate = (*successful_connects as f64 / total_attempts as f64) * 100.0;
        metric_summary.insert("tcp_connect_success_rate".to_string(), success_rate);
    }
    
    // 构建事件拓扑
    let events_topology: Vec<Event> = events.iter().map(|event| {
        Event {
            event_type: event.event_type.clone(),
            event_time_ms: event.ts_ms.unwrap_or(0) as i64,
            severity: None,
            details: Some(serde_json::to_value(event).unwrap_or_default()),
        }
    }).collect();
    
    Evidence {
        schema_version: "evidence.v0.2".to_string(),
        task_id: cfg.task_id.clone(),
        evidence_id: make_evidence_id(&cfg.task_id, "tcp_attempt_stage", &collection_id, &scope_key),
        evidence_type: "tcp_attempt_stage".to_string(),
        collection: CollectionMeta {
            collection_id,
            collection_status: collection_status.to_string(),
            probe_id: probe_id.to_string(),
            errors,
        },
        time_window: cfg.time_window,
        scope: Scope {
            pod: cfg.pod,
            container_id: cfg.container_id,
            cgroup_id: cfg.cgroup_id,
            pid_scope: None,
            scope_key,
            network_target: cfg.network_target,
        },
        selection: None,
        metric_summary,
        events_topology,
        top_calls: None,
        attribution: Attribution {
            status: collection_status.to_string(),
            confidence: None,
            source: Some("bpftrace".to_string()),
            mapping_version: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::evidence::TimeWindow;
    
    #[test]
    fn test_make_scope_key() {
        let scope_key = make_scope_key(Some("pod-123"), Some("cgroup-456"));
        assert!(!scope_key.is_empty());
    }
    
    #[test]
    fn test_make_evidence_id() {
        let evidence_id = make_evidence_id("task-001", "tcp_attempt_stage", "collection-001", "scope-001");
        assert!(!evidence_id.is_empty());
        assert!(evidence_id.len() > 32); // SHA256 hex length
    }
    
    #[test]
    fn test_build_evidence() {
        let cfg = TcpConnectStagesCollectorConfig {
            task_id: "test-task".to_string(),
            time_window: TimeWindow {
                start_time_ms: 1000,
                end_time_ms: 2000,
                collection_interval_ms: None,
            },
            pod: None,
            container_id: None,
            cgroup_id: None,
            network_target: None,
            requested_metrics: vec![],
            requested_events: vec![],
            nri_table: None,
            target_pids: None,
        };
        
        let evidence = build_evidence(
            cfg,
            "test-scope".to_string(),
            "test-collection".to_string(),
            "test-probe",
            vec![],
            HashMap::new(),
            HashMap::new(),
            vec![],
            "success",
        );
        
        assert_eq!(evidence.evidence_type, "tcp_attempt_stage");
        assert_eq!(evidence.task_id, "test-task");
        assert_eq!(evidence.attribution.status, "success");
    }
}
