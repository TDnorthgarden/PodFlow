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

pub struct FsStallCollectorConfig {
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
struct BpftraceFsEvent {
    #[serde(rename = "type")]
    event_type: String,
    pid: Option<u32>,
    comm: Option<String>,
    syscall_name: Option<String>,
    latency_us: Option<u64>,
    fs_op: Option<String>, // 文件系统操作类型：read/write/open/close/sync
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

/// 运行 bpftrace 文件系统卡顿采集探针
/// 
/// 最小实现方案：监控文件系统相关系统调用（read/write/open/sync等）的延迟
/// 与 block_io 证据联动分析，可形成"文件系统卡顿与I/O"的关联结论
pub fn run_fs_stall_collect_poc(cfg: FsStallCollectorConfig) -> Evidence {
    let scope_key = make_scope_key(
        cfg.pod.as_ref().and_then(|p| p.uid.as_deref()),
        cfg.cgroup_id.as_deref(),
    );
    
    let collection_id = uuid::Uuid::new_v4().to_string();
    let probe_id = "fs_stall.bt";
    
    // 计算采集持续时间
    let duration_ms = cfg.time_window.end_time_ms - cfg.time_window.start_time_ms;
    let duration_sec = (duration_ms / 1000).clamp(1, 60) as u64;
    
    let script_path = "scripts/bpftrace/fs/fs_stall.bt";
    
    // 存储采集结果
    let fs_latencies: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let fs_ops: Arc<Mutex<HashMap<String, Vec<u64>>>> = Arc::new(Mutex::new(HashMap::new()));
    let errors = Arc::new(Mutex::new(Vec::<CollectionError>::new()));
    
    let latencies_clone = fs_latencies.clone();
    let ops_clone = fs_ops.clone();
    
    // 启动 bpftrace 采集
    let mut child = match Command::new("sudo")
        .args(["bpftrace", script_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let mut errors_guard = errors.lock().unwrap();
            errors_guard.push(CollectionError {
                code: "BPFTRACE_SCRIPT_LOAD_FAILED".into(),
                message: format!("Failed to start bpftrace: {}", e),
                retryable: Some(false),
                detail: None,
            });
            drop(errors_guard);
            // 错误路径：安全获取数据
            let latencies: Vec<u64> = Arc::try_unwrap(latencies_clone)
                .map(|m| m.into_inner().unwrap_or_default())
                .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
            let ops: std::collections::HashMap<String, Vec<u64>> = Arc::try_unwrap(ops_clone)
                .map(|m| m.into_inner().unwrap_or_default())
                .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
            let errors: Vec<CollectionError> = Arc::try_unwrap(errors)
                .map(|m| m.into_inner().unwrap_or_default())
                .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
            return build_evidence(
                cfg, scope_key, collection_id, probe_id,
                latencies, ops, errors, "failed",
            );
        }
    };
    
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let reader = BufReader::new(stdout);
    
    // 采集超时控制
    let start_time = Instant::now();
    let timeout = Duration::from_secs(duration_sec);
    
    // 解析 bpftrace 输出
    for line in reader.lines() {
        if start_time.elapsed() > timeout {
            break;
        }
        
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        
        // 解析 JSON 输出
        if let Ok(event) = serde_json::from_str::<BpftraceFsEvent>(&line) {
            match event.event_type.as_str() {
                "fs_op_complete" => {
                    if let Some(latency) = event.latency_us {
                        let mut latencies = latencies_clone.lock().unwrap();
                        latencies.push(latency);
                        
                        // 按操作类型分类统计
                        let op_type = event.fs_op.clone()
                            .or_else(|| event.syscall_name.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        
                        let mut ops = ops_clone.lock().unwrap();
                        ops.entry(op_type)
                            .or_insert_with(Vec::new)
                            .push(latency);
                    }
                }
                "fs_stall_detected" => {
                    // 直接检测到卡顿事件
                    tracing::debug!("FS stall detected: {:?}", event);
                }
                _ => {}
            }
        }
    }
    
    // 停止 bpftrace
    let _ = child.kill();
    
    // 收集结果（使用 lock 获取数据，避免 Arc::try_unwrap 因引用计数失败）
    let fs_latencies: Vec<u64> = Arc::try_unwrap(fs_latencies)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    let fs_ops: std::collections::HashMap<String, Vec<u64>> = Arc::try_unwrap(fs_ops)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    let errors: Vec<CollectionError> = Arc::try_unwrap(errors)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|m| m.clone()).unwrap_or_default());
    
    let collection_status = if errors.is_empty() { "success" } else { "partial" };
    
    build_evidence(
        cfg, scope_key, collection_id, probe_id,
        fs_latencies, fs_ops, errors, collection_status,
    )
}

fn build_evidence(
    cfg: FsStallCollectorConfig,
    scope_key: String,
    collection_id: String,
    probe_id: &str,
    fs_latencies: Vec<u64>,
    fs_ops: HashMap<String, Vec<u64>>,
    errors: Vec<CollectionError>,
    collection_status: &str,
) -> Evidence {
    let mut metric_summary = HashMap::new();
    
    // 计算文件系统操作延迟分位
    if !fs_latencies.is_empty() {
        let mut sorted = fs_latencies.clone();
        sorted.sort();
        
        let len = sorted.len();
        let p50 = sorted[len * 50 / 100] as f64 / 1000.0; // us -> ms
        let p90 = sorted[len * 90 / 100] as f64 / 1000.0;
        let p99 = sorted[len * 99 / 100] as f64 / 1000.0;
        
        let is_requested = |m: &str| {
            cfg.requested_metrics.is_empty() || cfg.requested_metrics.contains(&m.to_string())
        };
        
        if is_requested("fs_stall_p50_ms") {
            metric_summary.insert("fs_stall_p50_ms".into(), p50);
        }
        if is_requested("fs_stall_p90_ms") {
            metric_summary.insert("fs_stall_p90_ms".into(), p90);
        }
        if is_requested("fs_stall_p99_ms") {
            metric_summary.insert("fs_stall_p99_ms".into(), p99);
        }
        
        // 计算平均延迟
        if is_requested("fs_stall_avg_ms") {
            let avg = sorted.iter().sum::<u64>() as f64 / len as f64 / 1000.0;
            metric_summary.insert("fs_stall_avg_ms".into(), avg);
        }
    }
    
    // 构建 top_calls（文件系统操作 Top N）
    let mut top_calls_vec: Vec<(String, u64, f64)> = fs_ops
        .iter()
        .map(|(op, latencies)| {
            let count = latencies.len() as u64;
            let p99 = if !latencies.is_empty() {
                let mut sorted = latencies.clone();
                sorted.sort();
                sorted[sorted.len() * 99 / 100.min(sorted.len().saturating_sub(1).max(1))] as f64 / 1000.0
            } else {
                0.0
            };
            (op.clone(), count, p99)
        })
        .collect();
    
    top_calls_vec.sort_by(|a, b| b.1.cmp(&a.1)); // 按 count 排序
    
    let top_calls_by_call: Vec<TopCall> = top_calls_vec
        .iter()
        .take(10)
        .map(|(op, count, p99)| {
            TopCall {
                call_name: op.clone(),
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
    
    // 检测文件系统卡顿突增（阈值 50ms）
    if let Some(p99) = metric_summary.get("fs_stall_p99_ms") {
        if *p99 > 50.0 {
            let is_requested = cfg.requested_events.is_empty() 
                || cfg.requested_events.contains(&"fs_stall_spike".to_string());
            if is_requested {
                // 找出导致卡顿的主要操作
                let top_op = top_calls_vec.first()
                    .map(|(op, _, _)| op.clone())
                    .unwrap_or_default();
                
                // spike_window / baseline_window 默认切分
                let window_duration = cfg.time_window.end_time_ms - cfg.time_window.start_time_ms;
                let spike_start = cfg.time_window.start_time_ms + window_duration / 2;
                
                events_topology.push(Event {
                    event_type: "fs_stall_spike".into(),
                    event_time_ms: spike_start,
                    severity: Some(8),
                    details: Some(json!({
                        "latency_ms_at_spike": p99,
                        "delta_p99_ms": p99 - 20.0, // 简化基线 20ms
                        "top_op_name": top_op,
                        "spike_window": {
                            "start_time_ms": spike_start,
                            "end_time_ms": cfg.time_window.end_time_ms,
                        },
                        "baseline_window": {
                            "start_time_ms": cfg.time_window.start_time_ms,
                            "end_time_ms": spike_start,
                        },
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
    
    let evidence_id = make_evidence_id(&cfg.task_id, "fs_stall", &collection_id, &scope_key);
    
    Evidence {
        schema_version: "evidence.v0.2".into(),
        task_id: cfg.task_id,
        evidence_id,
        evidence_type: "fs_stall".into(),
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
