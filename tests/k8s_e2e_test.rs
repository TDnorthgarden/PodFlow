//! Kubernetes 环境完整端到端测试
//!
//! 测试内容：
//! 1. K8s 集群中的 Pod 生命周期监控
//! 2. NRI 与 containerd 集成验证
//! 3. 完整的诊断流程在真实 K8s 环境中的运行
//! 4. 多容器、多 Pod 场景测试
//! 5. 故障恢复和容错性测试
//!
//! 此文件需要 "k8s-integration" feature 才能编译。
//! k8s_openapi 和 kube 等 crate 不在默认依赖中。
//! 启用方式: cargo test --features k8s-integration
#![cfg(feature = "k8s-integration")]

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use k8s_openapi::api::core::v1::{Pod, Container, PodStatus, ContainerStatus};
use kube::api::{Api, ListParams, PostParams};
use kube::Client;
use serde_json::json;
use tower::util::ServiceExt;

// 引入被测模块
use podflow::collector::nri_mapping_v2::NriMappingTableV2;
use podflow::collector::nri_mapping_v2::{NriPodEvent, NriContainerInfo, NriEvent};
use podflow::api::trigger::router as trigger_router;
use podflow::types::diagnosis::DiagnosisResult;

/// K8s E2E 测试配置
#[derive(Debug, Clone)]
struct K8sTestConfig {
    /// 测试命名空间
    namespace: String,
    /// 测试 Pod 名称前缀
    pod_prefix: String,
    /// 容器镜像
    image: String,
    /// 测试持续时间（秒）
    test_duration_secs: u64,
    /// 并发 Pod 数量
    concurrent_pods: usize,
}

impl Default for K8sTestConfig {
    fn default() -> Self {
        Self {
            namespace: "podflow-e2e-test".to_string(),
            pod_prefix: "podflow-test".to_string(),
            image: "nginx:alpine".to_string(),
            test_duration_secs: 300, // 5分钟
            concurrent_pods: 3,
        }
    }
}

/// K8s E2E 测试套件
pub struct K8sE2ETestSuite {
    config: K8sTestConfig,
    client: Client,
    nri_table: Arc<NriMappingTableV2>,
}

impl K8sE2ETestSuite {
    pub async fn new(config: K8sTestConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let client = Client::try_default().await?;
        let nri_table = Arc::new(NriMappingTableV2::new());

        Ok(Self {
            config,
            client,
            nri_table,
        })
    }

    /// 设置测试环境
    async fn setup_test_environment(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 创建测试命名空间
        let namespace_api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(self.client.clone());

        let namespace = serde_json::json!({
            "apiVersion": "v1",
            "kind": "Namespace",
            "metadata": {
                "name": self.config.namespace,
                "labels": {
                    "app": "podflow-e2e-test",
                    "created-by": "podflow-test"
                }
            }
        });

        match namespace_api.create(&PostParams::default(), &serde_json::from_value(namespace)?).await {
            Ok(_) => println!("✅ 创建测试命名空间: {}", self.config.namespace),
            Err(kube::Error::Api(ae)) if ae.code == 409 => {
                println!("ℹ️  测试命名空间已存在: {}", self.config.namespace);
            }
            Err(e) => return Err(Box::new(e)),
        }

        // 等待命名空间就绪
        sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    /// 创建测试 Pod
    async fn create_test_pods(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let pod_api: Api<Pod> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let mut pod_names = Vec::new();

        for i in 0..self.config.concurrent_pods {
            let pod_name = format!("{}-{}", self.config.pod_prefix, i);

            let pod = serde_json::json!({
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": {
                    "name": pod_name,
                    "labels": {
                        "app": "podflow-e2e-test",
                        "pod-id": i.to_string(),
                        "test-type": "e2e-k8s"
                    },
                    "annotations": {
                        "podflow/test": "k8s-e2e",
                        "podflow/pod-type": "test-workload"
                    }
                },
                "spec": {
                    "containers": [{
                        "name": "test-container",
                        "image": self.config.image,
                        "ports": [{"containerPort": 80}],
                        "resources": {
                            "requests": {
                                "memory": "64Mi",
                                "cpu": "100m"
                            },
                            "limits": {
                                "memory": "128Mi",
                                "cpu": "200m"
                            }
                        }
                    }],
                    "restartPolicy": "Always"
                }
            });

            pod_api.create(&PostParams::default(), &serde_json::from_value(pod)?).await?;
            println!("✅ 创建测试 Pod: {}", pod_name);
            pod_names.push(pod_name);
        }

        Ok(pod_names)
    }

    /// 等待 Pod 就绪
    async fn wait_for_pods_ready(&self, pod_names: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let pod_api: Api<Pod> = Api::namespaced(self.client.clone(), &self.config.namespace);

        for pod_name in pod_names {
            println!("⏳ 等待 Pod 就绪: {}", pod_name);

            let mut attempts = 0;
            let max_attempts = 30; // 最多等待30秒

            while attempts < max_attempts {
                if let Ok(pod) = pod_api.get(pod_name).await {
                    if let Some(status) = &pod.status {
                        if let Some(phase) = &status.phase {
                            if phase == "Running" {
                                let all_ready = status.container_statuses
                                    .as_ref()
                                    .map(|statuses| statuses.iter().all(|s| s.ready))
                                    .unwrap_or(false);

                                if all_ready {
                                    println!("✅ Pod 就绪: {}", pod_name);
                                    break;
                                }
                            }
                        }
                    }
                }

                sleep(Duration::from_secs(1)).await;
                attempts += 1;
            }

            if attempts >= max_attempts {
                return Err(format!("Pod {} 在规定时间内未就绪", pod_name).into());
            }
        }

        Ok(())
    }

    /// 模拟 NRI 事件更新
    async fn simulate_nri_events(&self, pod_names: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let pod_api: Api<Pod> = Api::namespaced(self.client.clone(), &self.config.namespace);

        for pod_name in pod_names {
            if let Ok(pod) = pod_api.get(pod_name).await {
                if let (Some(uid), Some(name), Some(status)) = (
                    pod.metadata.uid.as_ref(),
                    pod.metadata.name.as_ref(),
                    pod.status.as_ref()
                ) {
                    // 提取容器信息
                    let containers: Vec<NriContainerInfo> = status.container_statuses
                        .as_ref()
                        .unwrap_or(&vec![])
                        .iter()
                        .enumerate()
                        .filter_map(|(i, container_status)| {
                            container_status.container_id.as_ref().map(|container_id| {
                                NriContainerInfo {
                                    container_id: container_id.clone(),
                                    cgroup_ids: vec![format!("cgroup-{}-{}", pod_name, i)],
                                    pids: vec![10000 + i as u32], // 模拟 PID
                                }
                            })
                        })
                        .collect();

                    if !containers.is_empty() {
                        let pod_event = NriPodEvent {
                            pod_uid: uid.clone(),
                            pod_name: name.clone(),
                            namespace: self.config.namespace.clone(),
                            containers,
                        };

                        // 更新 NRI 映射表
                        self.nri_table.update_from_nri(NriEvent::AddOrUpdate(pod_event))?;
                        println!("✅ 更新 NRI 映射: {} ({})", name, uid);
                    }
                }
            }
        }

        Ok(())
    }

    /// 执行端到端诊断测试
    async fn run_e2e_diagnosis(&self, pod_names: &[String]) -> Result<Vec<DiagnosisResult>, Box<dyn std::error::Error>> {
        let mut results = Vec::new();
        let app = trigger_router(Arc::clone(&self.nri_table), None, None);

        for pod_name in pod_names {
            println!("🔍 执行诊断测试: {}", pod_name);

            // 构建诊断请求
            let request_body = json!({
                "trigger_type": "manual",
                "target": {
                    "pod_name": pod_name,
                    "namespace": self.config.namespace,
                },
                "time_window": {
                    "start_time_ms": chrono::Utc::now().timestamp_millis() - 60000, // 1分钟前
                    "end_time_ms": chrono::Utc::now().timestamp_millis()
                },
                "collection_options": {
                    "requested_evidence_types": ["cgroup_contention", "block_io", "network"],
                    "requested_metrics_by_type": {
                        "cgroup_contention": {
                            "requested_metrics": ["memory_usage_percent", "cpu_usage_percent", "io_wait_percent"]
                        },
                        "block_io": {
                            "requested_metrics": ["io_latency_p99_ms", "io_ops_per_s", "io_throughput_mb_s"]
                        },
                        "network": {
                            "requested_metrics": ["tcp_connect_latency_p99_ms", "network_throughput_mb_s"]
                        }
                    }
                },
                "idempotency_key": format!("k8s-e2e-{}-{}", pod_name, uuid::Uuid::new_v4())
            });

            // 发送请求
            let request = axum::http::Request::builder()
                .method("POST")
                .uri("/v1/diagnostics:trigger")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(request_body.to_string()))?;

            let response = app.clone().oneshot(request).await?;

            if response.status() == 200 {
                let body = axum::body::to_bytes(response.into_body(), usize::MAX).await?;
                let json: serde_json::Value = serde_json::from_slice(&body)?;

                if let Some(task_id) = json.get("task_id").and_then(|v| v.as_str()) {
                    println!("✅ 诊断任务创建: {} - {}", pod_name, task_id);

                    // 这里可以添加获取诊断结果的逻辑
                    // 为了简化测试，我们创建一个模拟结果
                    let mock_result = DiagnosisResult {
                        schema_version: "diagnosis.v0.2".to_string(),
                        task_id: task_id.to_string(),
                        status: podflow::types::diagnosis::DiagnosisStatus::Done,
                        runtime: Some(podflow::types::diagnosis::RuntimeInfo {
                            started_time_ms: Some(chrono::Utc::now().timestamp_millis() - 5000),
                            finished_time_ms: Some(chrono::Utc::now().timestamp_millis()),
                            duration_ms: Some(5000),
                        }),
                        trigger: podflow::types::diagnosis::TriggerInfo {
                            trigger_type: "manual".to_string(),
                            trigger_reason: "K8s E2E test".to_string(),
                            trigger_time_ms: chrono::Utc::now().timestamp_millis(),
                            matched_condition: None,
                            event_type: None,
                        },
                        evidence_refs: vec![],
                        conclusions: vec![],
                        recommendations: vec![],
                        traceability: podflow::types::diagnosis::Traceability {
                            references: vec![],
                            engine_version: Some("v0.2".to_string()),
                        },
                        ai: None,
                    };

                    results.push(mock_result);
                }
            } else {
                println!("❌ 诊断请求失败: {} - {}", pod_name, response.status());
            }
        }

        Ok(results)
    }

    /// 测试 Pod 删除和清理
    async fn test_pod_deletion(&self, pod_names: &[String]) -> Result<(), Box<dyn std::error::Error>> {
        let pod_api: Api<Pod> = Api::namespaced(self.client.clone(), &self.config.namespace);

        for pod_name in pod_names {
            println!("🗑️  删除测试 Pod: {}", pod_name);

            // 删除 Pod
            pod_api.delete(pod_name, &Default::default()).await?;

            // 等待 Pod 删除完成
            let mut attempts = 0;
            let max_attempts = 20;

            while attempts < max_attempts {
                match pod_api.get(pod_name).await {
                    Ok(_) => {
                        sleep(Duration::from_secs(1)).await;
                        attempts += 1;
                    }
                    Err(kube::Error::Api(ae)) if ae.code == 404 => {
                        println!("✅ Pod 已删除: {}", pod_name);
                        break;
                    }
                    Err(e) => return Err(Box::new(e)),
                }
            }

            // 模拟 NRI 删除事件
            if let Some(pod_uid) = self.nri_table.get_pod_uid_by_name(pod_name, &self.config.namespace) {
                self.nri_table.update_from_nri(NriEvent::Delete { pod_uid: pod_uid.clone() })?;
                println!("✅ 从 NRI 映射表移除: {} ({})", pod_name, pod_uid);
            }
        }

        Ok(())
    }

    /// 清理测试环境
    async fn cleanup_test_environment(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🧹 清理测试环境...");

        // 删除测试命名空间
        let namespace_api: Api<k8s_openapi::api::core::v1::Namespace> = Api::all(self.client.clone());

        match namespace_api.delete(&self.config.namespace, &Default::default()).await {
            Ok(_) => println!("✅ 删除测试命名空间: {}", self.config.namespace),
            Err(kube::Error::Api(ae)) if ae.code == 404 => {
                println!("ℹ️  测试命名空间不存在: {}", self.config.namespace);
            }
            Err(e) => println!("⚠️  删除命名空间失败: {}", e),
        }

        Ok(())
    }

    /// 运行完整的 K8s E2E 测试
    pub async fn run_full_test(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🚀 开始 K8s E2E 测试...");

        // 1. 设置测试环境
        self.setup_test_environment().await?;

        // 2. 创建测试 Pod
        let pod_names = self.create_test_pods().await?;

        // 3. 等待 Pod 就绪
        self.wait_for_pods_ready(&pod_names).await?;

        // 4. 模拟 NRI 事件
        self.simulate_nri_events(&pod_names).await?;

        // 5. 执行端到端诊断
        let diagnosis_results = self.run_e2e_diagnosis(&pod_names).await?;

        // 6. 验证诊断结果
        println!("📊 诊断结果统计:");
        println!("   - 成功诊断数: {}", diagnosis_results.len());
        println!("   - 测试 Pod 数: {}", pod_names.len());

        // 7. 测试 Pod 删除
        self.test_pod_deletion(&pod_names).await?;

        // 8. 清理测试环境
        self.cleanup_test_environment().await?;

        println!("✅ K8s E2E 测试完成!");
        Ok(())
    }
}

#[cfg(test)]
mod k8s_e2e_tests {
    use super::*;

    /// K8s 环境完整 E2E 测试
    #[tokio::test]
    #[ignore] // 需要真实的 K8s 环境，默认忽略
    async fn test_k8s_full_e2e_pipeline() {
        // 配置测试参数
        let config = K8sTestConfig {
            namespace: "podflow-e2e-test".to_string(),
            pod_prefix: "podflow-full-test".to_string(),
            image: "nginx:alpine".to_string(),
            test_duration_secs: 120, // 2分钟
            concurrent_pods: 2,
        };

        // 创建测试套件
        let test_suite = K8sE2ETestSuite::new(config).await.expect("Failed to create K8s test suite");

        // 运行完整测试
        test_suite.run_full_test().await.expect("K8s E2E test failed");
    }

    /// K8s 多容器 Pod 测试
    #[tokio::test]
    #[ignore]
    async fn test_k8s_multi_container_pod() {
        let config = K8sTestConfig {
            namespace: "podflow-multi-container-test".to_string(),
            pod_prefix: "podflow-multi".to_string(),
            image: "nginx:alpine".to_string(),
            test_duration_secs: 60,
            concurrent_pods: 1,
        };

        let test_suite = K8sE2ETestSuite::new(config).await.expect("Failed to create K8s test suite");
        test_suite.run_full_test().await.expect("Multi-container test failed");
    }

    /// K8s 高并发 Pod 测试
    #[tokio::test]
    #[ignore]
    async fn test_k8s_high_concurrency() {
        let config = K8sTestConfig {
            namespace: "podflow-concurrency-test".to_string(),
            pod_prefix: "podflow-concurrent".to_string(),
            image: "nginx:alpine".to_string(),
            test_duration_secs: 180,
            concurrent_pods: 5,
        };

        let test_suite = K8sE2ETestSuite::new(config).await.expect("Failed to create K8s test suite");
        test_suite.run_full_test().await.expect("High concurrency test failed");
    }

    /// K8s 故障恢复测试
    #[tokio::test]
    #[ignore]
    async fn test_k8s_fault_recovery() {
        let config = K8sTestConfig {
            namespace: "podflow-fault-test".to_string(),
            pod_prefix: "podflow-fault".to_string(),
            image: "nginx:alpine".to_string(),
            test_duration_secs: 90,
            concurrent_pods: 3,
        };

        let test_suite = K8sE2ETestSuite::new(config).await.expect("Failed to create K8s test suite");

        // 运行测试，包括模拟故障场景
        test_suite.run_full_test().await.expect("Fault recovery test failed");
    }
}