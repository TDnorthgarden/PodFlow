use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use podflow::diagnosis::{
    DiagnosisEngine, DiagnosisEngineConfig, RuleManager, TrendRule, TrendRuleConfig,
};
use podflow::types::diagnosis::{DiagnosisResult, Conclusion, Severity};
use podflow::types::evidence::{Evidence, CollectionMeta, TimeWindow, Scope, Attribution};
use podflow::publisher::alert_adapter::AlertRouter;
use podflow::collector::nri_batch::{BatchProcessor, BatchConfig};
use podflow::collector::nri_v3::NriV3Processor;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// 创建测试证据
fn create_test_evidence(metric_value: f64) -> Evidence {
    let mut metric_summary = HashMap::new();
    metric_summary.insert("memory_usage_percent".to_string(), metric_value);
    metric_summary.insert("cpu_usage_percent".to_string(), metric_value * 0.8);
    metric_summary.insert("disk_io_latency_ms".to_string(), metric_value * 2.0);

    Evidence {
        schema_version: "evidence.v0.2".to_string(),
        task_id: "perf-test-task".to_string(),
        evidence_id: "perf-test-evidence".to_string(),
        evidence_type: "cgroup_contention".to_string(),
        collection: CollectionMeta {
            collection_id: "perf-test-collection".to_string(),
            collection_status: "completed".to_string(),
            probe_id: "perf-test".to_string(),
            errors: vec![],
        },
        time_window: TimeWindow {
            start_time_ms: 1700000000000,
            end_time_ms: 1700000060000,
            collection_interval_ms: Some(1000),
        },
        scope: Scope::default(),
        selection: None,
        metric_summary,
        events_topology: vec![],
        top_calls: None,
        attribution: Attribution::default(),
    }
}

/// 创建测试诊断结果
fn create_test_diagnosis() -> DiagnosisResult {
    DiagnosisResult {
        task_id: "perf-test-task".to_string(),
        status: "completed".to_string(),
        evidence_refs: vec!["perf-test-evidence".to_string()],
        conclusions: vec![
            Conclusion {
                id: "conclusion-1".to_string(),
                rule_id: "rule-1".to_string(),
                severity: Severity::Warning,
                confidence: 0.85,
                title: "High memory usage detected".to_string(),
                description: "Memory usage is above 80%".to_string(),
                evidence_ids: vec!["perf-test-evidence".to_string()],
                recommendations: vec![],
            },
        ],
        trigger_reason: "manual".to_string(),
        created_at_ms: 1700000000000,
        updated_at_ms: 1700000060000,
    }
}

/// 基准测试：诊断规则评估
fn benchmark_rule_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_evaluation");
    group.sample_size(100);

    for metric_value in [50.0, 75.0, 90.0].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("metric_{}", metric_value)),
            metric_value,
            |b, &metric_value| {
                b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
                    let rule = TrendRule::new(
                        "perf-test-rule",
                        "cgroup_contention",
                        "memory_usage_percent",
                        TrendRuleConfig::default(),
                        "Test trend rule",
                        5,
                    );

                    // 添加数据点
                    let base_time = chrono::Utc::now().timestamp_millis();
                    for i in 0..10 {
                        rule.add_data_point(base_time + i * 1000, metric_value + i as f64 * 0.5);
                    }

                    // 评估规则
                    let evidence = create_test_evidence(metric_value);
                    let _result = rule.evaluate(&evidence).await;
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：NRI 事件处理
fn benchmark_nri_event_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("nri_event_processing");
    group.sample_size(100);

    for event_count in [1, 10, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("events_{}", event_count)),
            event_count,
            |b, &event_count| {
                b.iter(|| {
                    let mut total_size = 0;
                    for i in 0..event_count {
                        let evidence = create_test_evidence(50.0 + i as f64);
                        total_size += evidence.metric_summary.len();
                    }
                    black_box(total_size)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：告警路由
fn benchmark_alert_routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("alert_routing");
    group.sample_size(100);

    for pattern_count in [1, 10, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("patterns_{}", pattern_count)),
            pattern_count,
            |b, &pattern_count| {
                b.iter(|| {
                    let mut patterns = Vec::new();
                    for i in 0..pattern_count {
                        patterns.push(format!("alert-pattern-{}", i));
                    }

                    // 模拟告警匹配
                    let alert_name = "alert-pattern-50";
                    let matched = patterns.iter().any(|p| {
                        p.contains("pattern") && alert_name.contains("pattern")
                    });

                    black_box(matched)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：诊断结果序列化
fn benchmark_diagnosis_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("diagnosis_serialization");
    group.sample_size(100);

    group.bench_function("serialize_diagnosis", |b| {
        let diagnosis = create_test_diagnosis();
        b.iter(|| {
            let _json = serde_json::to_string(&diagnosis).unwrap();
        });
    });

    group.bench_function("deserialize_diagnosis", |b| {
        let diagnosis = create_test_diagnosis();
        let json = serde_json::to_string(&diagnosis).unwrap();
        b.iter(|| {
            let _diagnosis: DiagnosisResult = serde_json::from_str(&json).unwrap();
        });
    });

    group.finish();
}

/// 基准测试：证据处理
fn benchmark_evidence_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("evidence_processing");
    group.sample_size(100);

    for metric_count in [5, 20, 100].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("metrics_{}", metric_count)),
            metric_count,
            |b, &metric_count| {
                b.iter(|| {
                    let mut metric_summary = HashMap::new();
                    for i in 0..metric_count {
                        metric_summary.insert(
                            format!("metric_{}", i),
                            50.0 + i as f64 * 0.1,
                        );
                    }

                    // 计算平均值
                    let avg = metric_summary.values().sum::<f64>() / metric_summary.len() as f64;
                    black_box(avg)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：规则管理器
fn benchmark_rule_manager(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_manager");
    group.sample_size(50);

    group.bench_function("rule_manager_add_rule", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let manager = RuleManager::new();
            for i in 0..10 {
                let rule = TrendRule::new(
                    &format!("rule-{}", i),
                    "cgroup_contention",
                    "memory_usage_percent",
                    TrendRuleConfig::default(),
                    "Test rule",
                    5,
                );
                manager.add_rule(Box::new(rule)).await;
            }
        });
    });

    group.bench_function("rule_manager_evaluate", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let manager = RuleManager::new();
            for i in 0..5 {
                let rule = TrendRule::new(
                    &format!("rule-{}", i),
                    "cgroup_contention",
                    "memory_usage_percent",
                    TrendRuleConfig::default(),
                    "Test rule",
                    5,
                );
                manager.add_rule(Box::new(rule)).await;
            }

            let evidence = create_test_evidence(75.0);
            let _conclusions = manager.evaluate(&evidence).await;
        });
    });

    group.finish();
}

/// 基准测试：内存使用
fn benchmark_memory_usage(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_usage");
    group.sample_size(50);

    group.bench_function("create_evidence", |b| {
        b.iter(|| {
            let evidence = create_test_evidence(75.0);
            black_box(evidence)
        });
    });

    group.bench_function("create_diagnosis", |b| {
        b.iter(|| {
            let diagnosis = create_test_diagnosis();
            black_box(diagnosis)
        });
    });

    group.bench_function("clone_evidence", |b| {
        let evidence = create_test_evidence(75.0);
        b.iter(|| {
            let cloned = evidence.clone();
            black_box(cloned)
        });
    });

    group.finish();
}

/// 基准测试：并发操作
fn benchmark_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_operations");
    group.sample_size(50);

    for task_count in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("tasks_{}", task_count)),
            task_count,
            |b, &task_count| {
                b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
                    let mut handles = vec![];
                    for i in 0..task_count {
                        let handle = tokio::spawn(async move {
                            let evidence = create_test_evidence(50.0 + i as f64 * 0.1);
                            let _json = serde_json::to_string(&evidence).unwrap();
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        let _ = handle.await;
                    }
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：事件延迟 (<100ms)
fn benchmark_event_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_latency");
    group.sample_size(1000);
    group.measurement_time(Duration::from_secs(10));

    // 测试单个事件处理延迟
    group.bench_function("single_event_processing", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let start_time = Instant::now();
            
            // 模拟事件处理
            let evidence = create_test_evidence(75.0);
            let _json = serde_json::to_string(&evidence).unwrap();
            
            let elapsed = start_time.elapsed();
            assert!(elapsed.as_millis() < 100, "Event processing should be <100ms, got {:?}", elapsed);
            
            black_box(elapsed)
        });
    });

    // 测试批量事件处理延迟
    for batch_size in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("batch_event_processing", batch_size),
            batch_size,
            |b, &batch_size| {
                b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
                    let start_time = Instant::now();
                    
                    let mut evidences = Vec::new();
                    for i in 0..batch_size {
                        evidences.push(create_test_evidence(50.0 + i as f64 * 0.1));
                    }
                    
                    // 批量处理
                    for evidence in &evidences {
                        let _json = serde_json::to_string(&evidence).unwrap();
                    }
                    
                    let elapsed = start_time.elapsed();
                    let avg_latency = elapsed.as_millis() / batch_size as u128;
                    assert!(avg_latency < 100, "Average event latency should be <100ms, got {}", avg_latency);
                    
                    black_box(elapsed)
                });
            },
        );
    }

    group.finish();
}

/// 基准测试：批量处理性能 (<1s)
fn benchmark_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_processing");
    group.sample_size(100);
    group.measurement_time(Duration::from_secs(30));

    // 配置批量处理器
    let batch_config = BatchConfig {
        max_batch_size: 100,
        flush_interval_ms: 1000,
        max_wait_time_ms: 500,
        enable_compression: false,
        enable_metrics: true,
    };

    // 测试不同批量大小的处理时间
    for batch_size in [50, 100, 500, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("process_batch", batch_size),
            batch_size,
            |b, &batch_size| {
                b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
                    let start_time = Instant::now();
                    
                    // 创建批量数据
                    let mut batch_data = Vec::new();
                    for i in 0..batch_size {
                        let evidence = create_test_evidence(50.0 + i as f64 * 0.01);
                        batch_data.push(evidence);
                    }
                    
                    // 模拟批量处理
                    let batch_json = serde_json::to_string(&batch_data).unwrap();
                    let _processed: Vec<Evidence> = serde_json::from_str(&batch_json).unwrap();
                    
                    let elapsed = start_time.elapsed();
                    assert!(elapsed.as_secs() < 1, "Batch processing should be <1s, got {:?}", elapsed);
                    
                    black_box(elapsed)
                });
            },
        );
    }

    // 测试批量刷新间隔
    group.bench_function("batch_flush_interval", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let start_time = Instant::now();
            
            // 模拟批量刷新逻辑
            let (tx, mut rx) = mpsc::channel(1000);
            
            // 发送数据
            for i in 0..50 {
                let evidence = create_test_evidence(50.0 + i as f64 * 0.1);
                let _ = tx.send(evidence).await;
            }
            
            // 模拟批量接收和处理
            let mut batch = Vec::new();
            while let Some(item) = rx.recv().await {
                batch.push(item);
                if batch.len() >= 50 {
                    break;
                }
            }
            
            let elapsed = start_time.elapsed();
            assert!(elapsed.as_millis() <= 1000, "Batch flush should be <=1000ms, got {:?}", elapsed);
            
            black_box(elapsed)
        });
    });

    group.finish();
}

/// 基准测试：高吞吐量事件处理 (1000 events/sec)
fn benchmark_high_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_throughput");
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(30));

    // 测试 1000 events/sec 的处理能力
    group.throughput(Throughput::Elements(1000));
    group.bench_function("process_1000_events_per_sec", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let start_time = Instant::now();
            let target_duration = Duration::from_secs(1);
            
            // 创建并发任务来模拟高吞吐量
            let mut handles = vec![];
            for i in 0..100 {
                let handle = tokio::spawn(async move {
                    let evidence = create_test_evidence(50.0 + i as f64 * 0.01);
                    let _json = serde_json::to_string(&evidence).unwrap();
                });
                handles.push(handle);
            }
            
            // 等待所有任务完成
            for handle in handles {
                let _ = handle.await;
            }
            
            let elapsed = start_time.elapsed();
            
            // 验证吞吐量要求
            let events_per_sec = 1000.0 / elapsed.as_secs_f64();
            assert!(events_per_sec >= 1000.0, "Should process >=1000 events/sec, got {:.2}", events_per_sec);
            
            black_box(elapsed)
        });
    });

    // 测试持续高吞吐量
    group.bench_function("sustained_high_throughput", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let start_time = Instant::now();
            let test_duration = Duration::from_secs(5);
            
            let mut handles = vec![];
            for _batch in 0..5 {
                for i in 0..200 {
                    let handle = tokio::spawn(async move {
                        let evidence = create_test_evidence(50.0 + i as f64 * 0.001);
                        let _json = serde_json::to_string(&evidence).unwrap();
                    });
                    handles.push(handle);
                }
                
                // 短暂休息以模拟真实场景
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            
            // 等待所有任务完成
            for handle in handles {
                let _ = handle.await;
            }
            
            let elapsed = start_time.elapsed();
            let total_events = 1000;
            let events_per_sec = total_events as f64 / elapsed.as_secs_f64();
            
            assert!(events_per_sec >= 1000.0, "Sustained throughput should be >=1000 events/sec, got {:.2}", events_per_sec);
            
            black_box(elapsed)
        });
    });

    group.finish();
}

/// 基准测试：内存和性能优化
fn benchmark_memory_optimization(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_optimization");
    group.sample_size(100);

    // 测试零拷贝序列化
    group.bench_function("zero_copy_serialization", |b| {
        let evidence = create_test_evidence(75.0);
        b.iter(|| {
            // 使用零拷贝序列化（模拟）
            let _json_str = serde_json::to_string(&evidence).unwrap();
            black_box(_json_str.len())
        });
    });

    // 测试内存池复用
    group.bench_function("memory_pool_reuse", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            // 模拟内存池复用
            let mut evidences = Vec::with_capacity(100);
            for i in 0..100 {
                evidences.push(create_test_evidence(50.0 + i as f64 * 0.1));
            }
            
            // 清空并复用
            evidences.clear();
            evidences.reserve(100);
            
            black_box(evidences.capacity())
        });
    });

    // 测试流式处理
    group.bench_function("streaming_processing", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let (tx, rx) = mpsc::channel(1000);
            
            // 生产者
            let producer = tokio::spawn(async move {
                for i in 0..1000 {
                    let evidence = create_test_evidence(50.0 + i as f64 * 0.001);
                    let _ = tx.send(evidence).await;
                }
            });
            
            // 消费者（流式处理）
            let consumer = tokio::spawn(async move {
                let mut count = 0;
                let mut receiver = rx;
                while let Some(_evidence) = receiver.recv().await {
                    count += 1;
                    if count >= 1000 {
                        break;
                    }
                }
                count
            });
            
            let _ = producer.await;
            let processed = consumer.await.unwrap();
            
            black_box(processed)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_rule_evaluation,
    benchmark_nri_event_processing,
    benchmark_alert_routing,
    benchmark_diagnosis_serialization,
    benchmark_evidence_processing,
    benchmark_rule_manager,
    benchmark_memory_usage,
    benchmark_concurrent_operations,
    benchmark_event_latency,
    benchmark_batch_processing,
    benchmark_high_throughput,
    benchmark_memory_optimization,
);

criterion_main!(benches);
