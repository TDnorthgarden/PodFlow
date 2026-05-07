use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use nuts_observer::diagnosis::{
    DiagnosisEngine, DiagnosisEngineConfig, RuleManager, TrendRule, TrendRuleConfig,
};
use nuts_observer::types::diagnosis::{DiagnosisResult, Conclusion, Severity};
use nuts_observer::types::evidence::{Evidence, CollectionMeta, TimeWindow, Scope, Attribution};
use nuts_observer::publisher::alert_adapter::AlertRouter;
use std::collections::HashMap;
use std::sync::Arc;

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
);

criterion_main!(benches);
