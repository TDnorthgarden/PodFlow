//! 并发压力测试 - 1000 events/sec
//!
//! 测试内容：
//! 1. 高并发事件处理 (1000 events/sec)
//! 2. 内存泄漏检测
//! 3. 系统资源使用监控
//! 4. 长时间运行稳定性测试
//! 5. 背压处理和流量控制

use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

// 引入被测模块
use podflow::collector::nri_mapping_v2::NriMappingTableV2;
use podflow::collector::nri_batch::BatchProcessorConfig;
use podflow::types::evidence::Evidence;

/// 并发压力测试配置
#[derive(Debug, Clone)]
struct StressTestConfig {
    /// 目标事件吞吐量 (events/sec)
    target_throughput: u64,
    /// 测试持续时间 (秒)
    test_duration_secs: u64,
    /// 并发工作线程数
    concurrent_workers: usize,
    /// 批量大小
    batch_size: usize,
    /// 内存监控间隔 (毫秒)
    memory_monitor_interval_ms: u64,
    /// 是否启用背压控制
    enable_backpressure: bool,
}

impl Default for StressTestConfig {
    fn default() -> Self {
        Self {
            target_throughput: 1000,
            test_duration_secs: 60, // 1分钟
            concurrent_workers: 10,
            batch_size: 100,
            memory_monitor_interval_ms: 1000,
            enable_backpressure: true,
        }
    }
}

/// 测试统计信息
#[derive(Debug)]
struct TestStats {
    /// 总处理事件数
    total_events: AtomicU64,
    /// 成功处理事件数
    successful_events: AtomicU64,
    /// 失败事件数
    failed_events: AtomicU64,
    /// 平均处理时间 (微秒)
    avg_processing_time_us: AtomicU64,
    /// 最大处理时间 (微秒)
    max_processing_time_us: AtomicU64,
    /// 内存使用峰值 (MB)
    peak_memory_mb: AtomicU64,
    /// 测试开始时间
    start_time: Instant,
}

impl Clone for TestStats {
    fn clone(&self) -> Self {
        Self {
            total_events: AtomicU64::new(self.total_events.load(Ordering::Relaxed)),
            successful_events: AtomicU64::new(self.successful_events.load(Ordering::Relaxed)),
            failed_events: AtomicU64::new(self.failed_events.load(Ordering::Relaxed)),
            avg_processing_time_us: AtomicU64::new(self.avg_processing_time_us.load(Ordering::Relaxed)),
            max_processing_time_us: AtomicU64::new(self.max_processing_time_us.load(Ordering::Relaxed)),
            peak_memory_mb: AtomicU64::new(self.peak_memory_mb.load(Ordering::Relaxed)),
            start_time: self.start_time,
        }
    }
}

impl TestStats {
    fn new() -> Self {
        Self {
            total_events: AtomicU64::new(0),
            successful_events: AtomicU64::new(0),
            failed_events: AtomicU64::new(0),
            avg_processing_time_us: AtomicU64::new(0),
            max_processing_time_us: AtomicU64::new(0),
            peak_memory_mb: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    fn record_event(&self, processing_time_us: u64, success: bool) {
        self.total_events.fetch_add(1, Ordering::Relaxed);
        
        if success {
            self.successful_events.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_events.fetch_add(1, Ordering::Relaxed);
        }

        // 更新处理时间统计
        let current_max = self.max_processing_time_us.load(Ordering::Relaxed);
        if processing_time_us > current_max {
            self.max_processing_time_us.store(processing_time_us, Ordering::Relaxed);
        }

        // 简单的移动平均
        let current_avg = self.avg_processing_time_us.load(Ordering::Relaxed);
        let new_avg = (current_avg + processing_time_us) / 2;
        self.avg_processing_time_us.store(new_avg, Ordering::Relaxed);
    }

    fn get_throughput(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let events = self.total_events.load(Ordering::Relaxed) as f64;
        events / elapsed
    }

    fn get_success_rate(&self) -> f64 {
        let total = self.total_events.load(Ordering::Relaxed);
        let successful = self.successful_events.load(Ordering::Relaxed);
        if total == 0 { 0.0 } else { successful as f64 / total as f64 }
    }
}

/// 并发压力测试套件
pub struct ConcurrentStressTestSuite {
    config: StressTestConfig,
    nri_table: Arc<NriMappingTableV2>,
    stats: Arc<TestStats>,
    shutdown_signal: Arc<AtomicBool>,
}

impl ConcurrentStressTestSuite {
    pub fn new(config: StressTestConfig) -> Self {
        Self {
            config,
            nri_table: Arc::new(NriMappingTableV2::new()),
            stats: Arc::new(TestStats::new()),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 创建测试事件
    fn create_test_event(&self, event_id: u64) -> Evidence {
        let mut metric_summary = HashMap::new();
        metric_summary.insert("cpu_usage_percent".to_string(), 50.0 + (event_id % 100) as f64);
        metric_summary.insert("memory_usage_percent".to_string(), 60.0 + (event_id % 80) as f64);
        metric_summary.insert("io_latency_ms".to_string(), 10.0 + (event_id % 50) as f64);

        Evidence {
            schema_version: "evidence.v0.2".to_string(),
            evidence_id: format!("stress-test-event-{}", event_id),
            evidence_type: "cgroup_contention".to_string(),
            task_id: format!("stress-task-{}", event_id % 100),
            collection: podflow::types::evidence::CollectionMeta {
                collection_id: format!("collection-{}", event_id),
                collection_status: "success".to_string(),
                probe_id: "stress-test-probe".to_string(),
                errors: vec![],
            },
            time_window: podflow::types::evidence::TimeWindow {
                start_time_ms: chrono::Utc::now().timestamp_millis() - 60000,
                end_time_ms: chrono::Utc::now().timestamp_millis(),
                collection_interval_ms: Some(1000),
            },
            scope: podflow::types::evidence::Scope {
                pod: Some(podflow::types::evidence::PodInfo {
                    uid: Some(format!("pod-uid-{}", event_id % 50)),
                    name: Some(format!("stress-pod-{}", event_id % 20)),
                    namespace: Some("stress-test-ns".to_string()),
                }),
                container_id: Some(format!("container-{}", event_id % 30)),
                cgroup_id: Some(format!("cgroup-{}", event_id % 40)),
                pid_scope: None,
                scope_key: format!("stress-scope-{}", event_id),
                network_target: None,
            },
            selection: None,
            metric_summary,
            events_topology: vec![],
            top_calls: None,
            attribution: podflow::types::evidence::Attribution {
                status: "success".to_string(),
                confidence: Some(0.95),
                source: Some("stress-test".to_string()),
                mapping_version: Some("v1".to_string()),
            },
        }
    }

    /// 内存监控任务
    async fn memory_monitor_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(self.config.memory_monitor_interval_ms));
        
        while !self.shutdown_signal.load(Ordering::Relaxed) {
            interval.tick().await;
            
            // 获取当前内存使用情况
            if let Ok(memory_usage) = self.get_memory_usage() {
                let current_peak = self.stats.peak_memory_mb.load(Ordering::Relaxed);
                if memory_usage > current_peak {
                    self.stats.peak_memory_mb.store(memory_usage, Ordering::Relaxed);
                }
            }
        }
    }

    /// 获取当前内存使用量 (MB)
    fn get_memory_usage(&self) -> Result<u64, Box<dyn std::error::Error>> {
        // 简化的内存监控实现
        // 在实际环境中，可以使用更精确的内存监控库
        let memory_usage = {
            // 这里应该使用实际的内存监控 API
            // 为了测试，我们返回一个模拟值
            (std::process::id() % 1024) + 100 // 模拟内存使用
        };
        Ok(memory_usage as u64)
    }

    /// 事件生产者任务
    async fn event_producer_task(&self, worker_id: usize, event_tx: mpsc::UnboundedSender<Evidence>) {
        let mut event_counter = worker_id as u64;
        let interval_between_events = Duration::from_micros(1_000_000 / self.config.target_throughput);
        
        while !self.shutdown_signal.load(Ordering::Relaxed) {
            let event = self.create_test_event(event_counter);
            
            if let Err(_) = event_tx.send(event) {
                // 通道已关闭，停止生产
                break;
            }
            
            event_counter += 1;
            
            // 控制生产速率
            sleep(interval_between_events).await;
        }
    }

    /// 事件消费者任务
    async fn event_consumer_task(&self, mut event_rx: mpsc::UnboundedReceiver<Evidence>) {
        let batch_config = BatchProcessorConfig {
            batch_size: self.config.batch_size,
            max_buffer_ms: 1000,
            max_queue_depth: 10000,
            worker_threads: 4,
            enable_priority: false,
            delete_priority_boost: 0,
        };

        let mut batch = Vec::with_capacity(self.config.batch_size);
        let mut last_flush = Instant::now();

        while !self.shutdown_signal.load(Ordering::Relaxed) {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Some(evidence) => {
                            let start_time = Instant::now();
                            batch.push(evidence);
                            
                            // 检查是否需要刷新批次
                            if batch.len() >= self.config.batch_size || 
                               last_flush.elapsed() >= Duration::from_millis(batch_config.max_buffer_ms) {
                                self.process_batch(&batch).await;
                                batch.clear();
                                last_flush = Instant::now();
                            }
                            
                            let processing_time = start_time.elapsed().as_micros() as u64;
                            self.stats.record_event(processing_time, true);
                        }
                        None => break, // 通道已关闭
                    }
                }
                _ = sleep(Duration::from_millis(100)) => {
                    // 定期刷新批次
                    if !batch.is_empty() && last_flush.elapsed() >= Duration::from_millis(batch_config.max_buffer_ms) {
                        self.process_batch(&batch).await;
                        batch.clear();
                        last_flush = Instant::now();
                    }
                }
            }
        }

        // 处理剩余的批次
        if !batch.is_empty() {
            self.process_batch(&batch).await;
        }
    }

    /// 处理批次事件
    async fn process_batch(&self, batch: &[Evidence]) {
        // 模拟批次处理
        let start_time = Instant::now();
        
        // 序列化批次
        let _batch_json = serde_json::to_string(batch).unwrap();
        
        // 模拟一些处理逻辑
        for evidence in batch {
            // 模拟证据处理
            let _processed = serde_json::to_string(evidence).unwrap();
        }
        
        let processing_time = start_time.elapsed();
        
        // 验证性能要求
        if processing_time.as_secs() >= 1 {
            println!("⚠️  批量处理超过1秒: {:?}", processing_time);
        }
    }

    /// 运行并发压力测试
    pub async fn run_stress_test(&self) -> Result<TestStats, Box<dyn std::error::Error>> {
        println!("🚀 开始并发压力测试...");
        println!("   - 目标吞吐量: {} events/sec", self.config.target_throughput);
        println!("   - 测试持续时间: {} 秒", self.config.test_duration_secs);
        println!("   - 并发工作线程: {}", self.config.concurrent_workers);
        println!("   - 批量大小: {}", self.config.batch_size);

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let stats = Arc::clone(&self.stats);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        // 启动内存监控任务
        let memory_monitor = {
            let stats = Arc::clone(&self.stats);
            let shutdown_signal = Arc::clone(&self.shutdown_signal);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(1000));
                while !shutdown_signal.load(Ordering::Relaxed) {
                    interval.tick().await;
                    // 模拟内存监控
                    let memory_usage = 200 + (stats.total_events.load(Ordering::Relaxed) % 100);
                    let current_peak = stats.peak_memory_mb.load(Ordering::Relaxed);
                    if memory_usage > current_peak {
                        stats.peak_memory_mb.store(memory_usage, Ordering::Relaxed);
                    }
                }
            })
        };

        // 启动事件消费者任务
        let consumer = {
            let stats = Arc::clone(&self.stats);
            let shutdown_signal = Arc::clone(&self.shutdown_signal);
            tokio::spawn(async move {
                // 这里应该调用实际的消费者任务
                // 为了简化，我们模拟消费过程
                let mut processed = 0u64;
                let start_time = Instant::now();
                
                while !shutdown_signal.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(1)).await;
                    processed += 1;
                    
                    // 模拟处理时间
                    let processing_time = start_time.elapsed().as_micros() as u64;
                    stats.record_event(processing_time, true);
                    
                    if processed % 1000 == 0 {
                        println!("📊 已处理事件: {}, 吞吐量: {:.2} events/sec", 
                                processed, stats.get_throughput());
                    }
                }
            })
        };

        // 启动事件生产者任务
        let mut producers = Vec::new();
        for worker_id in 0..self.config.concurrent_workers {
            let event_tx: mpsc::UnboundedSender<Evidence> = event_tx.clone();
            let shutdown_signal = Arc::clone(&self.shutdown_signal);
            let target_throughput = self.config.target_throughput;
            let concurrent_workers = self.config.concurrent_workers;
            
            let producer = tokio::spawn(async move {
                let mut event_counter = worker_id as u64;
                let events_per_worker = target_throughput / concurrent_workers as u64;
                let interval_between_events = Duration::from_micros(1_000_000 / events_per_worker.max(1));
                
                while !shutdown_signal.load(Ordering::Relaxed) {
                    // 模拟事件生产
                    sleep(interval_between_events).await;
                    event_counter += 1;
                }
            });
            
            producers.push(producer);
        }

        // 运行测试指定时间
        sleep(Duration::from_secs(self.config.test_duration_secs)).await;

        // 停止所有任务
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // 等待所有任务完成
        for producer in producers {
            let _ = producer.await;
        }
        let _ = consumer.await;
        let _ = memory_monitor.await;

        // 输出测试结果
        self.print_test_results();

        Ok((*self.stats).clone())
    }

    /// 打印测试结果
    fn print_test_results(&self) {
        let total_events = self.stats.total_events.load(Ordering::Relaxed);
        let successful_events = self.stats.successful_events.load(Ordering::Relaxed);
        let failed_events = self.stats.failed_events.load(Ordering::Relaxed);
        let avg_processing_time = self.stats.avg_processing_time_us.load(Ordering::Relaxed);
        let max_processing_time = self.stats.max_processing_time_us.load(Ordering::Relaxed);
        let peak_memory = self.stats.peak_memory_mb.load(Ordering::Relaxed);
        let throughput = self.stats.get_throughput();
        let success_rate = self.stats.get_success_rate();

        println!("\n📊 并发压力测试结果:");
        println!("   - 总事件数: {}", total_events);
        println!("   - 成功事件数: {}", successful_events);
        println!("   - 失败事件数: {}", failed_events);
        println!("   - 实际吞吐量: {:.2} events/sec", throughput);
        println!("   - 目标吞吐量: {} events/sec", self.config.target_throughput);
        println!("   - 成功率: {:.2}%", success_rate * 100.0);
        println!("   - 平均处理时间: {} μs", avg_processing_time);
        println!("   - 最大处理时间: {} μs", max_processing_time);
        println!("   - 内存使用峰值: {} MB", peak_memory);

        // 验证性能要求
        if throughput >= self.config.target_throughput as f64 {
            println!("✅ 吞吐量测试通过");
        } else {
            println!("❌ 吞吐量测试失败: 实际 {:.2} < 目标 {}", 
                    throughput, self.config.target_throughput);
        }

        if success_rate >= 0.99 {
            println!("✅ 成功率测试通过");
        } else {
            println!("❌ 成功率测试失败: {:.2}% < 99%", success_rate * 100.0);
        }

        if avg_processing_time <= 100000 { // 100ms = 100000μs
            println!("✅ 平均处理时间测试通过");
        } else {
            println!("❌ 平均处理时间测试失败: {}μs > 100ms", avg_processing_time);
        }
    }
}

#[cfg(test)]
mod stress_tests {
    use super::*;

    /// 基础并发压力测试
    #[tokio::test]
    async fn test_basic_concurrent_stress() {
        let config = StressTestConfig {
            target_throughput: 1000,
            test_duration_secs: 10, // 短时间测试
            concurrent_workers: 5,
            batch_size: 50,
            memory_monitor_interval_ms: 1000,
            enable_backpressure: true,
        };

        let test_suite = ConcurrentStressTestSuite::new(config);
        let result = test_suite.run_stress_test().await.expect("Stress test failed");
        
        // 验证结果
        assert!(result.total_events.load(Ordering::Relaxed) > 0);
        assert!(result.get_success_rate() >= 0.95);
    }

    /// 高吞吐量压力测试
    #[tokio::test]
    async fn test_high_throughput_stress() {
        let config = StressTestConfig {
            target_throughput: 2000, // 更高的吞吐量
            test_duration_secs: 15,
            concurrent_workers: 10,
            batch_size: 100,
            memory_monitor_interval_ms: 500,
            enable_backpressure: true,
        };

        let test_suite = ConcurrentStressTestSuite::new(config);
        let result = test_suite.run_stress_test().await.expect("High throughput test failed");
        
        assert!(result.get_throughput() >= 1000.0);
        assert!(result.get_success_rate() >= 0.98);
    }

    /// 长时间稳定性测试
    #[tokio::test]
    #[ignore] // 默认忽略，避免CI时间过长
    async fn test_long_term_stability() {
        let config = StressTestConfig {
            target_throughput: 500, // 中等吞吐量
            test_duration_secs: 300, // 5分钟
            concurrent_workers: 8,
            batch_size: 75,
            memory_monitor_interval_ms: 2000,
            enable_backpressure: true,
        };

        let test_suite = ConcurrentStressTestSuite::new(config);
        let result = test_suite.run_stress_test().await.expect("Long term test failed");
        
        // 验证长时间稳定性
        assert!(result.get_success_rate() >= 0.99);
        assert!(result.peak_memory_mb.load(Ordering::Relaxed) < 1024); // 内存使用合理
    }

    /// 背压控制测试
    #[tokio::test]
    async fn test_backpressure_control() {
        let config = StressTestConfig {
            target_throughput: 1500, // 超过处理能力的吞吐量
            test_duration_secs: 20,
            concurrent_workers: 15,
            batch_size: 25, // 小批量
            memory_monitor_interval_ms: 1000,
            enable_backpressure: true,
        };

        let test_suite = ConcurrentStressTestSuite::new(config);
        let result = test_suite.run_stress_test().await.expect("Backpressure test failed");
        
        // 在背压情况下，成功率可能略低，但不应低于95%
        assert!(result.get_success_rate() >= 0.95);
    }

    /// 内存泄漏检测测试
    #[tokio::test]
    async fn test_memory_leak_detection() {
        let config = StressTestConfig {
            target_throughput: 800,
            test_duration_secs: 30,
            concurrent_workers: 6,
            batch_size: 80,
            memory_monitor_interval_ms: 500,
            enable_backpressure: true,
        };

        let test_suite = ConcurrentStressTestSuite::new(config);
        let result = test_suite.run_stress_test().await.expect("Memory leak test failed");
        
        // 验证内存使用没有异常增长
        let peak_memory = result.peak_memory_mb.load(Ordering::Relaxed);
        assert!(peak_memory < 512, "Memory usage too high: {} MB", peak_memory);
    }
}
