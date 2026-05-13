//! 故障注入测试 - Socket断开/Containerd重启
//!
//! 测试内容：
//! 1. Socket 连接断开故障模拟
//! 2. Containerd 重启故障模拟
//! 3. 网络分区故障模拟
//! 4. 磁盘 I/O 故障模拟
//! 5. 内存不足故障模拟
//! 6. 故障恢复和自愈能力测试

use std::collections::HashMap;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::sleep;

// 引入被测模块
use nuts_observer::collector::nri_mapping_v2::{NriMappingTableV2, NriPodEvent, NriContainerInfo, NriEvent};
#[cfg(feature = "nri-grpc")]
use nuts_observer::collector::nri_grpc::NriGrpcClient;
use nuts_observer::types::evidence::Evidence;

/// 故障类型枚举
#[derive(Debug, Clone)]
pub enum FaultType {
    /// Socket 连接断开
    SocketDisconnect,
    /// Containerd 重启
    ContainerdRestart,
    /// 网络分区
    NetworkPartition,
    /// 磁盘 I/O 故障
    DiskIOFailure,
    /// 内存不足
    OutOfMemory,
    /// 高延迟
    HighLatency,
    /// 数据损坏
    DataCorruption,
}

/// 故障配置
#[derive(Debug, Clone)]
struct FaultConfig {
    /// 故障类型
    fault_type: FaultType,
    /// 故障持续时间（秒）
    duration_secs: u64,
    /// 故障强度（0.0-1.0）
    intensity: f64,
    /// 故障注入延迟（秒）
    delay_secs: u64,
    /// 是否自动恢复
    auto_recovery: bool,
}

/// 故障注入结果
#[derive(Debug)]
struct FaultResult {
    /// 故障类型
    fault_type: FaultType,
    /// 故障开始时间
    start_time: Instant,
    /// 故障结束时间
    end_time: Option<Instant>,
    /// 是否成功注入
    injection_success: bool,
    /// 是否成功恢复
    recovery_success: bool,
    /// 系统响应时间（毫秒）
    response_time_ms: u64,
    /// 数据丢失数量
    data_loss_count: u64,
    /// 错误日志数量
    error_log_count: u64,
}

/// 故障注入测试套件
pub struct FaultInjectionTestSuite {
    nri_table: Arc<NriMappingTableV2>,
    // TODO: NriSocketClient removed - needs replacement for socket fault injection
    // socket_client: Option<Arc<NriSocketClient>>,
    #[cfg(feature = "nri-grpc")]
    grpc_client: Option<Arc<NriGrpcClient>>,
    active_faults: Arc<Mutex<Vec<FaultResult>>>,
    shutdown_signal: Arc<AtomicBool>,
}

impl FaultInjectionTestSuite {
    pub fn new() -> Self {
        Self {
            nri_table: Arc::new(NriMappingTableV2::new()),
            // socket_client: None,
            #[cfg(feature = "nri-grpc")]
            grpc_client: None,
            active_faults: Arc::new(Mutex::new(Vec::new())),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 初始化测试环境
    async fn setup_test_environment(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔧 设置故障注入测试环境...");

        // 创建模拟的 NRI Socket 客户端
        // 注意：这里使用模拟客户端，避免依赖真实的 containerd
        // TODO: NriSocketClient removed - needs replacement
        // self.socket_client = Some(Arc::new(NriSocketClient::new("/tmp/nuts-test-socket")));
        
        #[cfg(feature = "nri-grpc")]
        {
            self.grpc_client = Some(Arc::new(NriGrpcClient::new("localhost:8080")));
        }
        // 添加一些测试 Pod 数据
        self.setup_test_pods().await?;

        println!("✅ 测试环境设置完成");
        Ok(())
    }

    /// 设置测试 Pod 数据
    async fn setup_test_pods(&self) -> Result<(), Box<dyn std::error::Error>> {
        for i in 0..5 {
            let pod = NriPodEvent {
                pod_uid: format!("fault-test-pod-{}", i),
                pod_name: format!("fault-test-app-{}", i),
                namespace: "fault-test-ns".to_string(),
                containers: vec![
                    NriContainerInfo {
                        container_id: format!("fault-test-container-{}", i),
                        cgroup_ids: vec![format!("fault-test-cgroup-{}", i)],
                        pids: vec![20000 + i as u32],
                    },
                ],
            };
            
            self.nri_table.update_from_nri(NriEvent::AddOrUpdate(pod))?;
        }
        
        Ok(())
    }

    /// 注入 Socket 断开故障
    async fn inject_socket_disconnect_fault(&self, config: FaultConfig) -> Result<FaultResult, Box<dyn std::error::Error>> {
        println!("🔌 注入 Socket 断开故障...");
        
        let start_time = Instant::now();
        let result = FaultResult {
            fault_type: FaultType::SocketDisconnect,
            start_time,
            end_time: None,
            injection_success: false,
            recovery_success: false,
            response_time_ms: 0,
            data_loss_count: 0,
            error_log_count: 0,
        };

        // 等待故障注入延迟
        if config.delay_secs > 0 {
            sleep(Duration::from_secs(config.delay_secs)).await;
        }

        // 模拟 Socket 断开
// TODO: NriSocketClient removed - socket fault injection needs replacement
/*
        if let Some(socket_client) = &self.socket_client {
            // 在实际实现中，这里会断开 Socket 连接
            // 为了测试，我们模拟这个过程
            
            println!("⚠️  Socket 连接已断开");
            result.injection_success = true;

            // 记录故障期间的系统行为
            let fault_start = Instant::now();
            let mut events_processed = 0u64;
            let mut errors_detected = 0u64;

            // 模拟故障期间的系统行为
            while fault_start.elapsed() < Duration::from_secs(config.duration_secs) {
                // 尝试处理事件（应该失败）
                match self.process_test_event().await {
                    Ok(_) => events_processed += 1,
                    Err(_) => errors_detected += 1,
                }
                
                sleep(Duration::from_millis(100)).await;
            }

            result.data_loss_count = events_processed; // 在实际中这些可能会丢失
            result.error_log_count = errors_detected;

            // 模拟故障恢复
            if config.auto_recovery {
                println!("🔄 开始 Socket 连接恢复...");
                let recovery_start = Instant::now();
                
                // 模拟重连过程
                sleep(Duration::from_secs(2)).await;
                
                result.recovery_success = true;
                result.response_time_ms = recovery_start.elapsed().as_millis() as u64;
                result.end_time = Some(Instant::now());
                
                println!("✅ Socket 连接已恢复，恢复时间: {}ms", result.response_time_ms);
            }
        }
*/


        Ok(result)
    }

    /// 注入 Containerd 重启故障
    async fn inject_containerd_restart_fault(&self, config: FaultConfig) -> Result<FaultResult, Box<dyn std::error::Error>> {
        println!("🔄 注入 Containerd 重启故障...");
        
        let start_time = Instant::now();
        let mut result = FaultResult {
            fault_type: FaultType::ContainerdRestart,
            start_time,
            end_time: None,
            injection_success: false,
            recovery_success: false,
            response_time_ms: 0,
            data_loss_count: 0,
            error_log_count: 0,
        };

        // 等待故障注入延迟
        if config.delay_secs > 0 {
            sleep(Duration::from_secs(config.delay_secs)).await;
        }

        // 模拟 Containerd 重启
        println!("⚠️  Containerd 正在重启...");
        result.injection_success = true;

        // 记录重启前的状态
        let pre_restart_pods = self.nri_table.list_all_pods().len();
        
        // 模拟重启过程（清空映射表）
        let restart_start = Instant::now();
        
        // 在实际中，这里会清空 NRI 映射表
        // 为了测试，我们模拟这个过程
        sleep(Duration::from_secs(3)).await;
        
        // 模拟重启后的状态恢复
        if config.auto_recovery {
            println!("🔄 Containerd 重启完成，开始状态恢复...");
            
            // 重新添加测试 Pod
            self.setup_test_pods().await?;
            
            let post_restart_pods = self.nri_table.list_all_pods().len();
            
            result.recovery_success = true;
            result.response_time_ms = restart_start.elapsed().as_millis() as u64;
            result.end_time = Some(Instant::now());
            result.data_loss_count = (pre_restart_pods - post_restart_pods) as u64;
            
            println!("✅ Containerd 状态已恢复，恢复时间: {}ms", result.response_time_ms);
            println!("   - 重启前 Pod 数: {}", pre_restart_pods);
            println!("   - 重启后 Pod 数: {}", post_restart_pods);
            println!("   - 数据丢失: {}", result.data_loss_count);
        }

        Ok(result)
    }

    /// 注入网络分区故障
    async fn inject_network_partition_fault(&self, config: FaultConfig) -> Result<FaultResult, Box<dyn std::error::Error>> {
        println!("🌐 注入网络分区故障...");
        
        let start_time = Instant::now();
        let mut result = FaultResult {
            fault_type: FaultType::NetworkPartition,
            start_time,
            end_time: None,
            injection_success: false,
            recovery_success: false,
            response_time_ms: 0,
            data_loss_count: 0,
            error_log_count: 0,
        };

        // 等待故障注入延迟
        if config.delay_secs > 0 {
            sleep(Duration::from_secs(config.delay_secs)).await;
        }

        // 模拟网络分区
        println!("⚠️  网络分区已启用");
        result.injection_success = true;

        // 模拟网络分区期间的行为
        let partition_start = Instant::now();
        let mut failed_requests = 0u64;
        let mut total_requests = 0u64;

        while partition_start.elapsed() < Duration::from_secs(config.duration_secs) {
            total_requests += 1;
            
            // 模拟网络请求（应该失败）
            match self.simulate_network_request().await {
                Ok(_) => {
                    // 在网络分区期间，偶尔可能有成功的请求
                }
                Err(_) => {
                    failed_requests += 1;
                }
            }
            
            sleep(Duration::from_millis(200)).await;
        }

        result.error_log_count = failed_requests;

        // 模拟网络恢复
        if config.auto_recovery {
            println!("🔄 网络分区已解除，开始恢复...");
            let recovery_start = Instant::now();
            
            // 模拟网络恢复
            sleep(Duration::from_secs(1)).await;
            
            result.recovery_success = true;
            result.response_time_ms = recovery_start.elapsed().as_millis() as u64;
            result.end_time = Some(Instant::now());
            
            println!("✅ 网络已恢复，恢复时间: {}ms", result.response_time_ms);
            println!("   - 故障期间失败请求: {}/{}", failed_requests, total_requests);
        }

        Ok(result)
    }

    /// 注入磁盘 I/O 故障
    async fn inject_disk_io_fault(&self, config: FaultConfig) -> Result<FaultResult, Box<dyn std::error::Error>> {
        println!("💾 注入磁盘 I/O 故障...");
        
        let start_time = Instant::now();
        let mut result = FaultResult {
            fault_type: FaultType::DiskIOFailure,
            start_time,
            end_time: None,
            injection_success: false,
            recovery_success: false,
            response_time_ms: 0,
            data_loss_count: 0,
            error_log_count: 0,
        };

        // 等待故障注入延迟
        if config.delay_secs > 0 {
            sleep(Duration::from_secs(config.delay_secs)).await;
        }

        // 模拟磁盘 I/O 故障
        println!("⚠️  磁盘 I/O 故障已启用");
        result.injection_success = true;

        // 模拟磁盘 I/O 故障期间的行为
        let fault_start = Instant::now();
        let mut io_errors = 0u64;
        let mut total_io_operations = 0u64;

        while fault_start.elapsed() < Duration::from_secs(config.duration_secs) {
            total_io_operations += 1;
            
            // 模拟磁盘 I/O 操作
            match self.simulate_disk_io_operation().await {
                Ok(_) => {
                    // 偶尔成功的 I/O 操作
                }
                Err(_) => {
                    io_errors += 1;
                }
            }
            
            sleep(Duration::from_millis(100)).await;
        }

        result.error_log_count = io_errors;

        // 模拟磁盘恢复
        if config.auto_recovery {
            println!("🔄 磁盘 I/O 已恢复...");
            let recovery_start = Instant::now();
            
            // 模拟磁盘恢复
            sleep(Duration::from_secs(2)).await;
            
            result.recovery_success = true;
            result.response_time_ms = recovery_start.elapsed().as_millis() as u64;
            result.end_time = Some(Instant::now());
            
            println!("✅ 磁盘 I/O 已恢复，恢复时间: {}ms", result.response_time_ms);
            println!("   - 故障期间 I/O 错误: {}/{}", io_errors, total_io_operations);
        }

        Ok(result)
    }

    /// 处理测试事件
    async fn process_test_event(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 模拟事件处理
        let evidence = self.create_test_evidence();
        let _json = serde_json::to_string(&evidence)?;
        Ok(())
    }

    /// 模拟网络请求
    async fn simulate_network_request(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 模拟网络请求
        sleep(Duration::from_millis(50)).await;
        
        // 在故障注入期间，模拟请求失败
        // TODO: rand crate not in dependencies - use deterministic fallback
        // if rand::random::<f64>() < 0.8 {
        if false { // deterministic fallback
            Err("Network request failed due to partition".into())
        } else {
            Ok(())
        }
    }

    /// 模拟磁盘 I/O 操作
    async fn simulate_disk_io_operation(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 模拟磁盘 I/O
        sleep(Duration::from_millis(20)).await;
        
        // 在故障注入期间，模拟 I/O 失败
        // TODO: rand crate not in dependencies - use deterministic fallback
        // if rand::random::<f64>() < 0.7 {
        if false { // deterministic fallback
            Err("Disk I/O operation failed".into())
        } else {
            Ok(())
        }
    }

    /// 创建测试证据
    fn create_test_evidence(&self) -> Evidence {
        let mut metric_summary = HashMap::new();
        metric_summary.insert("cpu_usage_percent".to_string(), 75.0);
        metric_summary.insert("memory_usage_percent".to_string(), 60.0);

        Evidence {
            schema_version: "evidence.v0.2".to_string(),
            evidence_id: "fault-test-evidence".to_string(),
            evidence_type: "cgroup_contention".to_string(),
            task_id: "fault-test-task".to_string(),
            collection: nuts_observer::types::evidence::CollectionMeta {
                collection_id: "fault-test-collection".to_string(),
                collection_status: "success".to_string(),
                probe_id: "fault-test-probe".to_string(),
                errors: vec![],
            },
            time_window: nuts_observer::types::evidence::TimeWindow {
                start_time_ms: chrono::Utc::now().timestamp_millis() - 60000,
                end_time_ms: chrono::Utc::now().timestamp_millis(),
                collection_interval_ms: Some(1000),
            },
            scope: nuts_observer::types::evidence::Scope {
                pod: Some(nuts_observer::types::evidence::PodInfo {
                    uid: Some("fault-test-pod".to_string()),
                    name: Some("fault-test-app".to_string()),
                    namespace: Some("fault-test-ns".to_string()),
                }),
                container_id: Some("fault-test-container".to_string()),
                cgroup_id: Some("fault-test-cgroup".to_string()),
                pid_scope: None,
                scope_key: "fault-test-scope".to_string(),
                network_target: None,
            },
            selection: None,
            metric_summary,
            events_topology: vec![],
            top_calls: None,
            attribution: nuts_observer::types::evidence::Attribution {
                status: "success".to_string(),
                confidence: Some(0.95),
                source: Some("fault-test".to_string()),
                mapping_version: Some("v1".to_string()),
            },
        }
    }

    /// 运行所有故障注入测试
    pub async fn run_all_fault_tests(&mut self) -> Result<Vec<FaultResult>, Box<dyn std::error::Error>> {
        println!("🚀 开始故障注入测试套件...");
        
        // 设置测试环境
        self.setup_test_environment().await?;
        
        let mut all_results = Vec::new();

        // 测试 1: Socket 断开故障
        let socket_config = FaultConfig {
            fault_type: FaultType::SocketDisconnect,
            duration_secs: 10,
            intensity: 1.0,
            delay_secs: 2,
            auto_recovery: true,
        };
        
        let socket_result = self.inject_socket_disconnect_fault(socket_config).await?;
        all_results.push(socket_result);

        // 等待系统稳定
        sleep(Duration::from_secs(5)).await;

        // 测试 2: Containerd 重启故障
        let containerd_config = FaultConfig {
            fault_type: FaultType::ContainerdRestart,
            duration_secs: 5,
            intensity: 1.0,
            delay_secs: 1,
            auto_recovery: true,
        };
        
        let containerd_result = self.inject_containerd_restart_fault(containerd_config).await?;
        all_results.push(containerd_result);

        // 等待系统稳定
        sleep(Duration::from_secs(5)).await;

        // 测试 3: 网络分区故障
        let network_config = FaultConfig {
            fault_type: FaultType::NetworkPartition,
            duration_secs: 8,
            intensity: 0.8,
            delay_secs: 1,
            auto_recovery: true,
        };
        
        let network_result = self.inject_network_partition_fault(network_config).await?;
        all_results.push(network_result);

        // 等待系统稳定
        sleep(Duration::from_secs(5)).await;

        // 测试 4: 磁盘 I/O 故障
        let disk_config = FaultConfig {
            fault_type: FaultType::DiskIOFailure,
            duration_secs: 6,
            intensity: 0.9,
            delay_secs: 1,
            auto_recovery: true,
        };
        
        let disk_result = self.inject_disk_io_fault(disk_config).await?;
        all_results.push(disk_result);

        // 打印测试结果
        self.print_fault_test_results(&all_results);

        println!("✅ 故障注入测试套件完成");
        Ok(all_results)
    }

    /// 打印故障测试结果
    fn print_fault_test_results(&self, results: &[FaultResult]) {
        println!("\n📊 故障注入测试结果:");
        
        for (i, result) in results.iter().enumerate() {
            println!("\n{}. {:?} 故障:", i + 1, result.fault_type);
            println!("   - 注入成功: {}", if result.injection_success { "✅" } else { "❌" });
            println!("   - 恢复成功: {}", if result.recovery_success { "✅" } else { "❌" });
            println!("   - 响应时间: {}ms", result.response_time_ms);
            println!("   - 数据丢失: {}", result.data_loss_count);
            println!("   - 错误日志: {}", result.error_log_count);
            
            if let Some(end_time) = result.end_time {
                let total_duration = end_time.duration_since(result.start_time);
                println!("   - 总持续时间: {:?}", total_duration);
            }
        }

        // 验证故障恢复能力
        let recovery_success_rate = results.iter()
            .filter(|r| r.recovery_success)
            .count() as f64 / results.len() as f64;
        
        println!("\n🔍 故障恢复能力评估:");
        println!("   - 恢复成功率: {:.1}%", recovery_success_rate * 100.0);
        
        if recovery_success_rate >= 0.8 {
            println!("   ✅ 系统故障恢复能力良好");
        } else {
            println!("   ❌ 系统故障恢复能力需要改进");
        }

        // 验证数据完整性
        let total_data_loss: u64 = results.iter().map(|r| r.data_loss_count).sum();
        println!("   - 总数据丢失: {}", total_data_loss);
        
        if total_data_loss == 0 {
            println!("   ✅ 数据完整性保持良好");
        } else {
            println!("   ⚠️  检测到数据丢失，需要改进");
        }
    }
}

#[cfg(test)]
mod fault_injection_tests {
    use super::*;

    /// Socket 断开故障测试
    #[tokio::test]
    async fn test_socket_disconnect_fault() {
        let mut test_suite = FaultInjectionTestSuite::new();
        test_suite.setup_test_environment().await.expect("Setup failed");

        let config = FaultConfig {
            fault_type: FaultType::SocketDisconnect,
            duration_secs: 5,
            intensity: 1.0,
            delay_secs: 0,
            auto_recovery: true,
        };

        let result = test_suite.inject_socket_disconnect_fault(config).await
            .expect("Socket disconnect test failed");

        assert!(result.injection_success);
        assert!(result.recovery_success);
        assert!(result.response_time_ms < 10000); // 恢复时间应小于10秒
    }

    /// Containerd 重启故障测试
    #[tokio::test]
    async fn test_containerd_restart_fault() {
        let mut test_suite = FaultInjectionTestSuite::new();
        test_suite.setup_test_environment().await.expect("Setup failed");

        let config = FaultConfig {
            fault_type: FaultType::ContainerdRestart,
            duration_secs: 3,
            intensity: 1.0,
            delay_secs: 0,
            auto_recovery: true,
        };

        let result = test_suite.inject_containerd_restart_fault(config).await
            .expect("Containerd restart test failed");

        assert!(result.injection_success);
        assert!(result.recovery_success);
        assert!(result.response_time_ms < 15000); // 恢复时间应小于15秒
    }

    /// 网络分区故障测试
    #[tokio::test]
    async fn test_network_partition_fault() {
        let mut test_suite = FaultInjectionTestSuite::new();
        test_suite.setup_test_environment().await.expect("Setup failed");

        let config = FaultConfig {
            fault_type: FaultType::NetworkPartition,
            duration_secs: 4,
            intensity: 0.8,
            delay_secs: 0,
            auto_recovery: true,
        };

        let result = test_suite.inject_network_partition_fault(config).await
            .expect("Network partition test failed");

        assert!(result.injection_success);
        assert!(result.recovery_success);
        assert!(result.error_log_count > 0); // 应该有错误日志
    }

    /// 磁盘 I/O 故障测试
    #[tokio::test]
    async fn test_disk_io_fault() {
        let mut test_suite = FaultInjectionTestSuite::new();
        test_suite.setup_test_environment().await.expect("Setup failed");

        let config = FaultConfig {
            fault_type: FaultType::DiskIOFailure,
            duration_secs: 3,
            intensity: 0.9,
            delay_secs: 0,
            auto_recovery: true,
        };

        let result = test_suite.inject_disk_io_fault(config).await
            .expect("Disk I/O test failed");

        assert!(result.injection_success);
        assert!(result.recovery_success);
        assert!(result.error_log_count > 0); // 应该有 I/O 错误
    }

    /// 完整故障注入测试套件
    #[tokio::test]
    #[ignore] // 默认忽略，避免CI时间过长
    async fn test_full_fault_injection_suite() {
        let mut test_suite = FaultInjectionTestSuite::new();
        let results = test_suite.run_all_fault_tests().await
            .expect("Full fault injection suite failed");

        // 验证所有故障都成功注入和恢复
        assert!(!results.is_empty());
        
        let recovery_success_rate = results.iter()
            .filter(|r| r.recovery_success)
            .count() as f64 / results.len() as f64;
        
        assert!(recovery_success_rate >= 0.8, "Recovery success rate should be >= 80%");
    }
}
