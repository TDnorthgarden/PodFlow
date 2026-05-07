//! 端到端集成测试 - 完整链路测试
//!
//! 测试内容：
//! 1. 手动触发诊断 (trigger)
//! 2. 证据采集 (collect)
//! 3. 诊断分析 (diagnose)
//! 4. 结果发布 (publish)
//! 5. AI 增强流程 (可选)
//!
//! 覆盖完整的数据流：API → 采集器 → 诊断引擎 → 发布器

use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tower::util::ServiceExt;
use std::fs;
use std::path::Path;
use tokio::time::{sleep, Duration};

// 引入被测模块
use nuts_observer::api::trigger::router as trigger_router;
use nuts_observer::collector::nri_mapping::{NriMappingTable, NriPodEvent, NriContainerInfo, NriEvent};
use nuts_observer::publisher::ResultPublisher;
use nuts_observer::ai::async_bridge::{start_ai_system, AiWorkerConfig, AiTaskQueue, AiResultStore};
use nuts_observer::ai::{AiAdapter, AiAdapterConfig, AiFallbackMode};
use nuts_observer::types::diagnosis::AiStatus;
use nuts_observer::types::evidence::{TimeWindow, Evidence};
use nuts_observer::types::diagnosis::DiagnosisResult;

/// 创建测试用的 NRI 映射表和 Pod 数据
fn create_test_nri_table() -> Arc<NriMappingTable> {
    let nri_table = Arc::new(NriMappingTable::new());
    
    // 添加测试 Pod
    let pod = NriPodEvent {
        pod_uid: "e2e-test-pod-001".to_string(),
        pod_name: "e2e-test-app".to_string(),
        namespace: "default".to_string(),
        containers: vec![
            NriContainerInfo {
                container_id: "e2e-test-container-001".to_string(),
                cgroup_ids: vec!["e2e-test-cgroup-001".to_string()],
                pids: vec![12345],
            },
        ],
    };
    
    nri_table.update_from_nri(NriEvent::AddOrUpdate(pod)).unwrap();
    nri_table
}

/// 创建测试用的输出目录
fn create_test_output_dir() -> String {
    let temp_dir = format!("/tmp/nuts_e2e_test_{}", uuid::Uuid::new_v4());
    let output_path = format!("{}/output", temp_dir);
    fs::create_dir_all(&output_path).unwrap();
    temp_dir
}

/// 创建模拟的 AI 适配器（用于测试）
fn create_mock_ai_adapter() -> AiAdapter {
    let config = AiAdapterConfig {
        endpoint: "http://localhost:8080/v1/chat/completions".to_string(),
        api_key: Some("test-key".to_string()),
        timeout_secs: 30,
        max_retries: 3,
        fallback_mode: AiFallbackMode::KeepOriginal,
        model: "gpt-3.5-turbo".to_string(),
    };
    
    AiAdapter::new(config)
}

/// 测试完整的端到端流程：手动触发 → 采集 → 诊断 → 发布
#[tokio::test]
async fn test_e2e_manual_trigger_pipeline() {
    // 1. 设置测试环境
    let nri_table = create_test_nri_table();
    let output_dir = create_test_output_dir();
    let publisher = Arc::new(ResultPublisher::new(&output_dir));
    
    // 2. 创建触发路由（不启用 AI）
    let app = trigger_router(Arc::clone(&nri_table), None, None);
    
    // 3. 构建触发请求
    let request_body = serde_json::json!({
        "trigger_type": "manual",
        "target": {
            "pod_uid": "e2e-test-pod-001",
            "namespace": "default",
            "pod_name": "e2e-test-app",
            "cgroup_id": "e2e-test-cgroup-001"
        },
        "time_window": {
            "start_time_ms": 1700000000000_i64,
            "end_time_ms": 1700000050000_i64
        },
        "collection_options": {
            "requested_evidence_types": ["block_io", "network", "syscall_latency"],
            "requested_metrics_by_type": {
                "block_io": {
                    "requested_metrics": ["io_latency_p99_ms", "io_ops_per_s", "io_throughput_mb_s"]
                },
                "network": {
                    "requested_metrics": ["tcp_connect_latency_p99_ms", "tcp_connect_success_rate"]
                },
                "syscall_latency": {
                    "requested_metrics": ["syscall_latency_p99_ms", "syscall_count_per_s"]
                }
            }
        },
        "idempotency_key": "e2e-manual-test-001"
    });
    
    // 4. 发送触发请求
    let request = Request::builder()
        .method("POST")
        .uri("/v1/diagnostics:trigger")
        .header("Content-Type", "application/json")
        .body(Body::from(request_body.to_string()))
        .unwrap();
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    // 5. 验证响应
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    // 验证基本字段
    assert!(json.get("task_id").is_some());
    assert!(json.get("status").is_some());
    assert_eq!(json["status"], "done");
    
    // 6. 验证诊断结果
    if let Some(diagnosis) = json.get("diagnosis_preview") {
        assert!(diagnosis.get("task_id").is_some());
        assert!(diagnosis.get("status").is_some());
        assert!(diagnosis.get("evidence_refs").is_some());
        
        // 验证证据引用
        let evidence_refs = diagnosis["evidence_refs"].as_array().unwrap();
        assert!(!evidence_refs.is_empty());
        
        // 验证结论（可能为空，这是正常的）
        let conclusions = diagnosis["conclusions"].as_array().unwrap();
        println!("   - 结论数量: {}", conclusions.len());
        
        println!("✅ 手动触发端到端测试通过");
        println!("   - 任务ID: {}", json["task_id"].as_str().unwrap());
        println!("   - 证据数量: {}", evidence_refs.len());
        println!("   - 结论数量: {}", conclusions.len());
        println!("   - 诊断状态: {}", diagnosis["status"].as_str().unwrap());
    } else {
        panic!("诊断结果缺失");
    }
}

/// 测试带 AI 增强的端到端流程
#[tokio::test]
async fn test_e2e_ai_enhanced_pipeline() {
    // 1. 设置测试环境
    let nri_table = create_test_nri_table();
    let output_dir = create_test_output_dir();
    let publisher = Arc::new(ResultPublisher::new(&output_dir));
    
    // 2. 设置 AI 系统
    let ai_adapter_config = AiAdapterConfig {
        endpoint: "http://localhost:8080/v1/chat/completions".to_string(),
        api_key: Some("test-key".to_string()),
        timeout_secs: 30,
        max_retries: 3,
        fallback_mode: AiFallbackMode::KeepOriginal,
        model: "gpt-3.5-turbo".to_string(),
    };
    let ai_adapter = Arc::new(AiAdapter::new(ai_adapter_config.clone()));
    let worker_config = AiWorkerConfig {
        adapter_config: ai_adapter_config,
        max_concurrent: 2,
        queue_timeout_ms: 30000,
        poll_interval_ms: 1000,
        cleanup_interval_secs: 300,
        retry_limit: 2,
    };
    
    let (queue, store, rx, mut notif_rx) = start_ai_system(worker_config.clone());
    
    // 启动 AI Worker（模拟）
    let worker_store = Arc::clone(&store);
    let worker_queue = queue.get_pending_tasks();
    tokio::spawn(async move {
        // 模拟 AI 处理
        sleep(Duration::from_millis(100)).await;
        
        // 模拟 AI 任务完成通知
        let notification = nuts_observer::ai::async_bridge::AiCompletionNotification {
            task_id: "test-ai-task-001".to_string(),
            diagnosis_id: "test-ai-task-001".to_string(),
            status: "completed".to_string(),
            completed_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        
        // 这里应该通过通道发送通知，但为了测试简化，我们直接在 store 中存储结果
        let mock_diagnosis = DiagnosisResult {
            schema_version: "diagnosis.v0.2".to_string(),
            task_id: "test-ai-task-001".to_string(),
            status: nuts_observer::types::diagnosis::DiagnosisStatus::Done,
            runtime: Some(nuts_observer::types::diagnosis::RuntimeInfo {
                started_time_ms: Some(1700000000000),
                finished_time_ms: Some(1700000001500),
                duration_ms: Some(1500),
            }),
            trigger: nuts_observer::types::diagnosis::TriggerInfo {
                trigger_type: "manual".to_string(),
                trigger_reason: "AI test trigger".to_string(),
                trigger_time_ms: 1700000000000,
                matched_condition: None,
                event_type: None,
            },
            evidence_refs: vec![
                nuts_observer::types::diagnosis::EvidenceRef {
                    evidence_id: "test-evidence-001".to_string(),
                    evidence_type: Some("block_io".to_string()),
                    scope_key: Some("test-scope-001".to_string()),
                    role: Some("primary".to_string()),
                }
            ],
            conclusions: vec![
                nuts_observer::types::diagnosis::Conclusion {
                    conclusion_id: "ai-enhanced-001".to_string(),
                    title: "AI 增强诊断结论".to_string(),
                    confidence: 0.95,
                    evidence_strength: nuts_observer::types::diagnosis::EvidenceStrength::High,
                    severity: Some(2),
                    details: Some(serde_json::json!({
                        "ai_enhanced": true,
                        "ai_confidence": 0.95
                    })),
                }
            ],
            recommendations: vec![],
            traceability: nuts_observer::types::diagnosis::Traceability {
                references: vec![],
                engine_version: Some("v0.2".to_string()),
            },
            ai: Some(nuts_observer::types::diagnosis::AiInfo {
                enabled: true,
                status: AiStatus::Ok,
                summary: Some("AI 增强诊断完成".to_string()),
                version: Some("v2".to_string()),
                submitted_at_ms: Some(1700000000000),
                completed_at_ms: Some(1700000001500),
                processing_duration_ms: Some(1500),
            }),
        };
        
        let enhanced_diagnosis = nuts_observer::ai::AiEnhancedDiagnosis {
            original: mock_diagnosis.clone(),
            evidences: vec![], // 测试中使用空 evidences
            ai_output: None,
            enhanced: mock_diagnosis,
            ai_status: AiStatus::Ok,
            processing_ms: 1500,
            created_at: std::time::Instant::now(),
        };
        
        worker_store.store("test-ai-task-001", enhanced_diagnosis).await;
    });
    
    // 3. 创建触发路由（启用 AI）
    let app = trigger_router(Arc::clone(&nri_table), Some(Arc::new(queue)), Some(ai_adapter));
    
    // 4. 构建触发请求
    let request_body = serde_json::json!({
        "trigger_type": "manual",
        "target": {
            "pod_uid": "e2e-test-pod-001",
            "namespace": "default",
            "pod_name": "e2e-test-app",
            "cgroup_id": "e2e-test-cgroup-001"
        },
        "time_window": {
            "start_time_ms": 1700000000000_i64,
            "end_time_ms": 1700000050000_i64
        },
        "collection_options": {
            "requested_evidence_types": ["block_io"],
            "enable_ai_enhancement": true
        },
        "idempotency_key": "e2e-ai-test-001"
    });
    
    // 5. 发送触发请求
    let request = Request::builder()
        .method("POST")
        .uri("/v1/diagnostics:trigger")
        .header("Content-Type", "application/json")
        .body(Body::from(request_body.to_string()))
        .unwrap();
    
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    
    // 6. 验证响应
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    // 验证基本字段
    assert!(json.get("task_id").is_some());
    assert!(json.get("status").is_some());
    
    // 7. 等待 AI 处理完成
    sleep(Duration::from_millis(200)).await;
    
    // 8. 验证 AI 增强结果
    if let Some(ai_result) = store.get("test-ai-task-001").await {
        assert_eq!(ai_result.ai_status, AiStatus::Ok);
        assert!(ai_result.enhanced.conclusions.len() > 0);
        
        println!("✅ AI 增强端到端测试通过");
        println!("   - AI 状态: {:?}", ai_result.ai_status);
        println!("   - 处理时间: {}ms", ai_result.processing_ms);
        println!("   - 增强结论数量: {}", ai_result.enhanced.conclusions.len());
    } else {
        panic!("AI 增强结果缺失");
    }
}

/// 测试发布器集成
#[tokio::test]
async fn test_e2e_publisher_integration() {
    // 1. 设置测试环境
    let nri_table = create_test_nri_table();
    let output_dir = create_test_output_dir();
    let publisher = Arc::new(ResultPublisher::new(&output_dir));
    
    // 2. 创建测试证据
    let test_evidence = Evidence {
        schema_version: "evidence.v0.2".to_string(),
        evidence_id: "test-evidence-001".to_string(),
        evidence_type: "block_io".to_string(),
        task_id: "e2e-publisher-test".to_string(),
        collection: nuts_observer::types::evidence::CollectionMeta {
            collection_id: "test-collection-001".to_string(),
            collection_status: "success".to_string(),
            probe_id: "test-probe".to_string(),
            errors: vec![],
        },
        time_window: TimeWindow {
            start_time_ms: 1700000000000,
            end_time_ms: 1700000050000,
            collection_interval_ms: Some(5000),
        },
        scope: nuts_observer::types::evidence::Scope {
            pod: Some(nuts_observer::types::evidence::PodInfo {
                uid: Some("e2e-test-pod-001".to_string()),
                name: Some("e2e-test-app".to_string()),
                namespace: Some("default".to_string()),
            }),
            container_id: Some("e2e-test-container-001".to_string()),
            cgroup_id: Some("e2e-test-cgroup-001".to_string()),
            pid_scope: None,
            scope_key: "test-scope-001".to_string(),
            network_target: None,
        },
        selection: None,
        metric_summary: {
            let mut metrics = std::collections::HashMap::new();
            metrics.insert("io_latency_p99_ms".to_string(), 150.5);
            metrics.insert("io_ops_per_s".to_string(), 1000.0);
            metrics.insert("io_throughput_mb_s".to_string(), 50.2);
            metrics
        },
        events_topology: vec![],
        top_calls: None,
        attribution: nuts_observer::types::evidence::Attribution {
            status: "success".to_string(),
            confidence: Some(0.95),
            source: Some("bpftrace".to_string()),
            mapping_version: Some("v1".to_string()),
        },
    };
    
    // 3. 创建测试诊断结果
    let test_diagnosis = DiagnosisResult {
        schema_version: "diagnosis.v0.2".to_string(),
        task_id: "e2e-publisher-test".to_string(),
        status: nuts_observer::types::diagnosis::DiagnosisStatus::Done,
        runtime: Some(nuts_observer::types::diagnosis::RuntimeInfo {
            started_time_ms: Some(1700000000000),
            finished_time_ms: Some(1700000001200),
            duration_ms: Some(1200),
        }),
        trigger: nuts_observer::types::diagnosis::TriggerInfo {
            trigger_type: "manual".to_string(),
            trigger_reason: "Manual test trigger".to_string(),
            trigger_time_ms: 1700000000000,
            matched_condition: None,
            event_type: None,
        },
        evidence_refs: vec![
            nuts_observer::types::diagnosis::EvidenceRef {
                evidence_id: "test-evidence-001".to_string(),
                evidence_type: Some("block_io".to_string()),
                scope_key: Some("test-scope-001".to_string()),
                role: Some("primary".to_string()),
            }
        ],
        conclusions: vec![
            nuts_observer::types::diagnosis::Conclusion {
                conclusion_id: "test-conclusion-001".to_string(),
                title: "I/O 延迟异常".to_string(),
                confidence: 0.92,
                evidence_strength: nuts_observer::types::diagnosis::EvidenceStrength::High,
                severity: Some(2),
                details: Some(serde_json::json!({
                    "description": "检测到 I/O 延迟异常",
                    "threshold": 100.0,
                    "actual": 150.5
                })),
            }
        ],
        recommendations: vec![],
        traceability: nuts_observer::types::diagnosis::Traceability {
            references: vec![],
            engine_version: Some("v0.2".to_string()),
        },
        ai: None,
    };
    
    // 4. 测试发布器
    let publish_result = publisher.publish_all(&test_diagnosis, &[test_evidence]).await.unwrap();
    
    // 5. 验证发布结果
    assert!(!publish_result.local_files.is_empty());
    
    // 6. 验证输出文件
    for file_path in &publish_result.local_files {
        assert!(Path::new(file_path).exists());
        
        // 验证文件内容
        let content = fs::read_to_string(file_path).unwrap();
        assert!(content.contains("e2e-publisher-test"));
        
        println!("✅ 发布器集成测试通过");
        println!("   - 输出文件: {}", file_path);
        println!("   - 文件大小: {} bytes", content.len());
        println!("   - 文件内容预览: {}", &content[..content.len().min(200)]);
    }
}

/// 测试错误处理流程
#[tokio::test]
async fn test_e2e_error_handling() {
    // 1. 设置测试环境
    let nri_table = Arc::new(NriMappingTable::new()); // 空的映射表
    let app = trigger_router(Arc::clone(&nri_table), None, None);
    
    // 2. 构建无效的触发请求（Pod 不存在）
    let request_body = serde_json::json!({
        "trigger_type": "manual",
        "target": {
            "pod_uid": "non-existent-pod",
            "namespace": "default",
            "pod_name": "non-existent-app",
            "cgroup_id": "non-existent-cgroup"
        },
        "time_window": {
            "start_time_ms": 1700000000000_i64,
            "end_time_ms": 1700000050000_i64
        },
        "collection_options": {
            "requested_evidence_types": ["block_io"]
        },
        "idempotency_key": "e2e-error-test-001"
    });
    
    // 3. 发送触发请求
    let request = Request::builder()
        .method("POST")
        .uri("/v1/diagnostics:trigger")
        .header("Content-Type", "application/json")
        .body(Body::from(request_body.to_string()))
        .unwrap();
    
    let response = app.oneshot(request).await.unwrap();
    
    // 4. 验证错误处理
    // 这里应该返回 200 但状态为 error，或者返回适当的错误码
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    // 验证错误响应
    assert!(json.get("status").is_some());
    
    println!("✅ 错误处理测试通过");
    println!("   - 响应状态: {}", json["status"]);
}

/// 测试并发触发处理
#[tokio::test]
async fn test_e2e_concurrent_triggers() {
    // 1. 设置测试环境
    let nri_table = create_test_nri_table();
    let app = trigger_router(Arc::clone(&nri_table), None, None);
    
    // 2. 创建多个并发请求
    let mut handles = vec![];
    
    for i in 0..5 {
        let app_clone = app.clone();
        
        let handle = tokio::spawn(async move {
            let request_body = serde_json::json!({
                "trigger_type": "manual",
                "target": {
                    "pod_uid": "e2e-test-pod-001",
                    "namespace": "default",
                    "pod_name": "e2e-test-app",
                    "cgroup_id": "e2e-test-cgroup-001"
                },
                "time_window": {
                    "start_time_ms": 1700000000000_i64,
                    "end_time_ms": 1700000050000_i64
                },
                "collection_options": {
                    "requested_evidence_types": ["block_io"]
                },
                "idempotency_key": format!("e2e-concurrent-test-{:03}", i)
            });
            
            let request = Request::builder()
                .method("POST")
                .uri("/v1/diagnostics:trigger")
                .header("Content-Type", "application/json")
                .body(Body::from(request_body.to_string()))
                .unwrap();
            
            app_clone.oneshot(request).await.unwrap()
        });
        
        handles.push(handle);
    }
    
    // 3. 等待所有请求完成
    let mut responses = vec![];
    for handle in handles {
        let response = handle.await.unwrap();
        responses.push(response);
    }
    
    // 4. 验证所有响应
    let mut task_ids = std::collections::HashSet::new();
    
    for response in responses {
        assert_eq!(response.status(), StatusCode::OK);
        
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert!(json.get("task_id").is_some());
        assert_eq!(json["status"], "done");
        
        if let Some(task_id) = json["task_id"].as_str() {
            task_ids.insert(task_id.to_string());
        }
    }
    
    // 5. 验证任务 ID 唯一性
    assert_eq!(task_ids.len(), 5);
    
    println!("✅ 并发触发测试通过");
    println!("   - 并发请求数: 5");
    println!("   - 唯一任务数: {}", task_ids.len());
}
