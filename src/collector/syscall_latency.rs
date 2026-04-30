use crate::types::error::NutsError;
use crate::types::evidence::*;
use crate::types::evidence::{TopCalls, TopCall};
use crate::collector::nri_mapping::{AttributionSource, NriMappingTable};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct SyscallCollectorConfig {
    pub task_id: String,
    pub time_window: TimeWindow,
    pub pod: Option<PodInfo>,
    pub container_id: Option<String>,
    pub cgroup_id: Option<String>,
    pub requested_metrics: Vec<String>,
    pub requested_events: Vec<String>,
    /// NRI 映射表引用，用于查询归属
    pub nri_table: Option<Arc<NriMappingTable>>,
}

#[derive(Debug, Clone, Deserialize)]
struct BpftraceSyscallEvent {
    #[serde(rename = "type")]
    event_type: String,
    pid: Option<u32>,
    comm: Option<String>,
    syscall_name: String,
    latency_us: Option<u64>,
    ts_ms: Option<u64>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
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

/// 运行 bpftrace syscall 采集探针
pub fn run_syscall_collect_poc(cfg: SyscallCollectorConfig) -> Result<Evidence, NutsError> {
    let scope_key = make_scope_key(
        cfg.pod.as_ref().and_then(|p| p.uid.as_deref()),
        cfg.cgroup_id.as_deref(),
    );
    
    let collection_id = uuid::Uuid::new_v4().to_string();
    let probe_id = "syscall_latency.bt";
    
    // 计算采集持续时间
    let duration_ms = cfg.time_window.end_time_ms - cfg.time_window.start_time_ms;
    let duration_sec = (duration_ms / 1000).clamp(1, 60) as u64;
    
    let script_path = "scripts/bpftrace/syscall/syscall_latency.bt";
    
    // 存储采集结果
    let syscall_stats: Arc<Mutex<HashMap<String, Vec<u64>>>> = Arc::new(Mutex::new(HashMap::new()));
    let errors = Arc::new(Mutex::new(Vec::<CollectionError>::new()));
    
    let stats_clone = syscall_stats.clone();
    
    // 启动 bpftrace 采集
    let mut child = match Command::new("sudo")
        .args(["bpftrace", script_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let mut errors_guard = errors.lock().map_err(|_| NutsError::lock_error("Failed to acquire lock"))?;
            errors_guard.push(CollectionError {
                code: "BPFTRACE_SCRIPT_LOAD_FAILED".into(),
                message: format!("Failed to start bpftrace: {}", e),
                retryable: Some(false),
                detail: None,
            });
            drop(errors_guard);
            // 错误路径：安全获取数据
            let stats: std::collections::HashMap<String, Vec<u64>> = Arc::try_unwrap(stats_clone)
                .map(|m| m.into_inner().unwrap_or_default())
                .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
            let errors: Vec<CollectionError> = Arc::try_unwrap(errors)
                .map(|m| m.into_inner().unwrap_or_default())
                .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
            return Ok(build_evidence(
                cfg, scope_key, collection_id, probe_id,
                stats, errors, "failed",
            ));
        }
    };
    
    let stdout = child.stdout.take().ok_or_else(|| NutsError::internal("Failed to capture stdout"))?;
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
        if let Ok(event) = serde_json::from_str::<BpftraceSyscallEvent>(&line_str) {
            match event.event_type.as_str() {
                "syscall_exit" => {
                    if let Some(latency) = event.latency_us {
                        let mut stats = stats_clone.lock().map_err(|_| NutsError::lock_error("Failed to acquire lock"))?;
                        stats.entry(event.syscall_name.clone())
                            .or_insert_with(Vec::new)
                            .push(latency);
                    }
                }
                _ => {}
            }
        }
    }
    
    // 停止 bpftrace
    let _ = child.kill();
    
    // 收集结果（使用 lock 获取数据，避免 Arc::try_unwrap 因引用计数失败）
    let syscall_stats: std::collections::HashMap<String, Vec<u64>> = Arc::try_unwrap(syscall_stats)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    let errors: Vec<CollectionError> = Arc::try_unwrap(errors)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    
    let collection_status = if errors.is_empty() { "success" } else { "partial" };
    
Ok(    build_evidence(
        cfg, scope_key, collection_id, probe_id,
        syscall_stats, errors, collection_status,
    ))
}

fn build_evidence(
    cfg: SyscallCollectorConfig,
    scope_key: String,
    collection_id: String,
    probe_id: &str,
    syscall_stats: HashMap<String, Vec<u64>>,
    errors: Vec<CollectionError>,
    collection_status: &str,
) -> Evidence {
    let mut metric_summary = HashMap::new();
    
    // 计算所有系统调用的总延迟分布
    let all_latencies: Vec<u64> = syscall_stats.values()
        .flat_map(|v| v.iter().cloned())
        .collect();
    
    if !all_latencies.is_empty() {
        let mut sorted = all_latencies.clone();
        sorted.sort();
        
        let len = sorted.len();
        let p99 = sorted[len * 99 / 100] as f64 / 1000.0; // us -> ms
        
        let is_requested = |m: &str| {
            cfg.requested_metrics.is_empty() || cfg.requested_metrics.contains(&m.to_string())
        };
        
        if is_requested("syscall_latency_p99_ms") {
            metric_summary.insert("syscall_latency_p99_ms".into(), p99);
        }
    }
    
    // 构建 top_calls
    let mut top_calls_vec: Vec<(String, u64, f64)> = syscall_stats
        .iter()
        .map(|(name, latencies)| {
            let count = latencies.len() as u64;
            let p99 = if !latencies.is_empty() {
                let mut sorted = latencies.clone();
                sorted.sort();
                sorted[sorted.len() * 99 / 100.min(sorted.len() - 1)] as f64 / 1000.0
            } else {
                0.0
            };
            (name.clone(), count, p99)
        })
        .collect();
    
    // 按 count 排序，取 Top N
    top_calls_vec.sort_by(|a, b| b.1.cmp(&a.1));
    
    let top_calls_by_call: Vec<TopCall> = top_calls_vec
        .iter()
        .take(10) // Top 10
        .map(|(name, count, p99)| {
            TopCall {
                call_name: name.clone(),
                count: *count,
                p95_latency_ms: None,
                p99_latency_ms: Some(*p99),
            }
        })
        .collect();
    
    let top_calls = if !top_calls_by_call.is_empty() {
        Some(TopCalls {
            by_call: top_calls_by_call,
        })
    } else {
        None
    };
    
    // 构建 events_topology
    let mut events_topology = Vec::new();
    
    // 检测系统调用延迟突增（阈值 10ms）
    if let Some(p99) = metric_summary.get("syscall_latency_p99_ms") {
        if *p99 > 10.0 {
            let is_requested = cfg.requested_events.is_empty() 
                || cfg.requested_events.contains(&"syscall_latency_spike".to_string());
            if is_requested {
                // 找出导致突增的主要系统调用
                let top_call = top_calls_vec.first()
                    .map(|(name, _, _)| name.clone())
                    .unwrap_or_default();
                
                events_topology.push(Event {
                    event_type: "syscall_latency_spike".into(),
                    event_time_ms: cfg.time_window.start_time_ms + (cfg.time_window.end_time_ms - cfg.time_window.start_time_ms) / 2,
                    severity: Some(7),
                    details: Some(json!({
                        "top_call_name": top_call,
                        "delta_p99_ms": p99 - 5.0, // 简化基线
                        "syscall_latency_p99_ms": p99,
                    })),
                });
            }
        }
    }
    
    let collected_metrics: Vec<String> = metric_summary.keys().cloned().collect();
    let collected_events: Vec<String> = events_topology.iter().map(|e| e.event_type.clone()).collect();
    
    let selection = Selection {
        requested_metrics: cfg.requested_metrics.clone(),
        collected_metrics,
        requested_events: cfg.requested_events.clone(),
        collected_events,
    };
    
    // 保存 cgroup_id 存在状态
    let has_cgroup_id = cfg.cgroup_id.is_some();
    
    // 查询 NRI 映射表获取归属信息
    let attribution_info = if let Some(ref table) = cfg.nri_table {
        let pod_uid = cfg.pod.as_ref().and_then(|p| p.uid.as_deref());
        let cgroup_id = cfg.cgroup_id.as_deref();
        table.resolve_attribution(pod_uid, cgroup_id, None).ok()
    } else {
        None
    };
    
    let scope = Scope {
        pod: cfg.pod,
        container_id: cfg.container_id,
        cgroup_id: cfg.cgroup_id,
        pid_scope: None,
        scope_key: scope_key.clone(),
        network_target: None,
    };
    
    // 根据 NRI 映射结果构建归因信息
    let attribution = if let Some(ref info) = attribution_info {
        Attribution {
            status: info.status.to_string(),
            confidence: Some(info.confidence),
            source: Some(match info.source {
                AttributionSource::Nri => "nri".into(),
                AttributionSource::PidMap => "pid_map".into(),
                AttributionSource::Uncertain => "uncertain".into(),
            }),
            mapping_version: Some(info.mapping_version.clone()),
        }
    } else {
        Attribution {
            status: if has_cgroup_id { "nri_mapped".into() } else { "pid_cgroup_fallback".into() },
            confidence: Some(if has_cgroup_id { 0.9 } else { 0.6 }),
            source: if has_cgroup_id { Some("nri".into()) } else { Some("pid_map".into()) },
            mapping_version: None,
        }
    };
    
    let collection = CollectionMeta {
        collection_id: collection_id.clone(),
        collection_status: collection_status.into(),
        probe_id: probe_id.into(),
        errors,
    };
    
    let evidence_id = make_evidence_id(&cfg.task_id, "syscall_latency", &collection_id, &scope_key);
    
    Evidence {
        schema_version: "evidence.v0.2".into(),
        task_id: cfg.task_id,
        evidence_id,
        evidence_type: "syscall_latency".into(),
        collection,
        time_window: cfg.time_window,
        scope,
        selection: Some(selection),
        metric_summary,
        events_topology,
        top_calls,
        attribution,
    }
}
