//! NRI V3 模块单元测试
//!
//! 测试覆盖:
//! - NriMappingTableV2 并发操作
//! - EventVersionManager 版本控制
//! - NriBatchProcessor 批量处理
//! - NriV3 集成流程

#[cfg(test)]
mod tests {
    use crate::collector::nri_mapping_v2::NriMappingTableV2;
    use crate::collector::nri_mapping_v2::{NriEvent, NriPodEvent, NriContainerInfo};
    use crate::collector::nri_v3::{NriV3, NriV3Config, CapacityConfig};
    use crate::collector::nri_persist::PersistConfig;
    use crate::collector::nri_batch::BatchProcessorConfig;
    use crate::collector::nri_version::EventVersionManager;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    /// 创建测试用的 Pod 事件
    fn create_add_pod_event(pod_uid: &str, pod_name: &str) -> NriEvent {
        NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: pod_uid.to_string(),
            pod_name: pod_name.to_string(),
            namespace: "default".to_string(),
            containers: vec![],
        })
    }

    /// 创建带容器的 Pod 事件
    fn create_add_pod_with_container(pod_uid: &str, container_id: &str) -> NriEvent {
        NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: pod_uid.to_string(),
            pod_name: format!("pod-{}", pod_uid),
            namespace: "default".to_string(),
            containers: vec![NriContainerInfo {
                container_id: container_id.to_string(),
                cgroup_ids: vec![format!("/kubepods/{}", container_id)],
                pids: vec![1001],
            }],
        })
    }

    /// 测试 NriMappingTableV2 基本 CRUD
    #[tokio::test]
    async fn test_mapping_table_v2_basic() {
        let table = Arc::new(NriMappingTableV2::new());

        // 通过事件接口添加 Pod
        let pod_uid = "test-pod-123".to_string();
        let event = create_add_pod_event(&pod_uid, "test-pod");
        table.update_from_nri(event).unwrap();

        // 验证查询
        let found = table.get_pod_details(&pod_uid);
        assert!(found.is_some());
        let (pod, containers) = found.unwrap();
        assert_eq!(pod.pod_name, "test-pod");
        assert!(containers.is_empty());

        // 测试更新（添加容器）
        let event = create_add_pod_with_container(&pod_uid, "container-1");
        table.update_from_nri(event).unwrap();

        let found = table.get_pod_details(&pod_uid);
        let (_pod, containers) = found.unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].container_id, "container-1");

        // 测试删除
        let delete_event = NriEvent::Delete { pod_uid: pod_uid.clone() };
        table.update_from_nri(delete_event).unwrap();
        assert!(table.get_pod_details(&pod_uid).is_none());
    }

    /// 测试并发写入性能
    #[tokio::test]
    async fn test_mapping_table_v2_concurrent() {
        let table = Arc::new(NriMappingTableV2::new());
        let mut handles = vec![];

        // 10 个并发任务，每个写入 100 个 Pod
        for i in 0..10 {
            let table_clone = Arc::clone(&table);
            let handle = tokio::spawn(async move {
                for j in 0..100 {
                    let pod_uid = format!("pod-{}-{}", i, j);
                    let event = create_add_pod_event(&pod_uid, &format!("pod-{}", j));
                    table_clone.update_from_nri(event).unwrap();
                }
            });
            handles.push(handle);
        }

        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap();
        }

        // 验证总数
        assert_eq!(table.pod_count(), 1000);
    }

    /// 测试 EventVersionManager 版本操作
    #[tokio::test]
    async fn test_version_manager_basic() {
        let vm = Arc::new(EventVersionManager::new());

        // 生成版本
        let v1 = vm.generate_version();
        assert!(v1 > 0);

        // 更新版本
        let pod_uid = "test-pod-version";
        let result = vm.try_update(pod_uid, v1);
        assert!(result.is_ok());

        // 获取版本
        let stored = vm.get_version(pod_uid);
        assert_eq!(stored, v1);

        // 生成另一个版本
        let v2 = vm.generate_version();
        assert_ne!(v1, v2);
    }

    /// 测试并发版本生成
    #[tokio::test]
    async fn test_version_manager_concurrent() {
        let vm = Arc::new(EventVersionManager::new());
        let mut handles = vec![];

        // 100 个并发任务生成版本
        for _ in 0..100 {
            let vm_clone = Arc::clone(&vm);
            let handle = tokio::spawn(async move {
                vm_clone.generate_version()
            });
            handles.push(handle);
        }

        let mut versions = vec![];
        for handle in handles {
            versions.push(handle.await.unwrap());
        }

        // 验证版本唯一（无重复）
        versions.sort();
        versions.dedup();
        assert_eq!(versions.len(), 100);
    }

    /// 测试 NriV3 基本功能（替代直接的批量处理器测试）
    #[tokio::test]
    async fn test_nri_v3_basic_functionality() {
        let config = NriV3Config {
            persistence: PersistConfig::default(),
            batch: BatchProcessorConfig {
                worker_threads: 1,
                max_queue_depth: 100,
                batch_size: 10,
                max_buffer_ms: 100,
                enable_priority: false,
                delete_priority_boost: 10,
            },
            enable_persistence: false,
            enable_metrics: true,
            capacity: CapacityConfig::default(),
        };

        // 创建 NRI V3 实例
        let nri_v3 = NriV3::new(config).await;
        assert!(nri_v3.is_ok());

        let v3 = nri_v3.unwrap();

        // 提交事件
        let event = create_add_pod_event("batch-test-pod", "batch-test");
        let result = v3.try_submit_event(event);
        assert!(result.is_ok());

        // 等待处理
        sleep(Duration::from_millis(200)).await;

        // 刷新
        v3.flush().await;

        // 关闭
        v3.shutdown().await;
    }

    /// 测试批量事件处理
    #[tokio::test]
    async fn test_nri_v3_batch_processing() {
        let config = NriV3Config {
            persistence: PersistConfig::default(),
            batch: BatchProcessorConfig {
                worker_threads: 2,
                max_queue_depth: 1000,
                batch_size: 50,
                max_buffer_ms: 100,
                enable_priority: true,
                delete_priority_boost: 10,
            },
            enable_persistence: false,
            enable_metrics: true,
            capacity: CapacityConfig::default(),
        };

        // 创建 NRI V3 实例
        let nri_v3 = NriV3::new(config).await;
        assert!(nri_v3.is_ok());

        let v3 = nri_v3.unwrap();

        // 批量提交 200 个事件
        for i in 0..200 {
            let event = create_add_pod_event(&format!("batch-pod-{}", i), &format!("pod-{}", i));
            v3.try_submit_event(event).unwrap();
        }

        // 等待批处理完成
        sleep(Duration::from_millis(500)).await;

        // 验证处理结果
        let table = v3.table();
        assert!(table.pod_count() > 0);

        // 清理
        v3.flush().await;
        v3.shutdown().await;
    }

    /// 测试 NriV3 完整集成
    #[tokio::test]
    async fn test_nri_v3_integration() {
        let config = NriV3Config {
            persistence: PersistConfig::default(),
            batch: BatchProcessorConfig {
                worker_threads: 2,
                max_queue_depth: 100,
                batch_size: 10,
                max_buffer_ms: 100,
                enable_priority: true,
                delete_priority_boost: 10,
            },
            enable_persistence: false,
            enable_metrics: true,
            capacity: CapacityConfig::default(),
        };

        // 创建 NRI V3 实例
        let nri_v3 = NriV3::new(config).await;
        assert!(nri_v3.is_ok());

        let v3 = nri_v3.unwrap();

        // 提交 Pod 创建事件
        let event = create_add_pod_with_container("integration-pod", "integration-container");

        let result = v3.submit_event(event).await;
        assert!(result.is_ok());

        // 等待事件处理
        sleep(Duration::from_millis(300)).await;

        // 验证映射表已更新
        let table = v3.table();
        assert!(table.pod_count() > 0);

        // 验证指标
        let metrics = v3.metrics();
        let export = metrics.export_prometheus();
        assert!(export.contains("nri_events_total"));

        // 关闭
        v3.shutdown().await;
    }

    /// 测试删除事件（RemoveContainer 语义）
    #[tokio::test]
    async fn test_remove_container_event() {
        let table = Arc::new(NriMappingTableV2::new());

        // 先添加一个多容器 Pod
        let add_event = NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: "multi-ctr-pod".to_string(),
            pod_name: "multi-ctr".to_string(),
            namespace: "default".to_string(),
            containers: vec![
                NriContainerInfo {
                    container_id: "ctr-1".to_string(),
                    cgroup_ids: vec!["/kubepods/ctr-1".to_string()],
                    pids: vec![1001],
                },
                NriContainerInfo {
                    container_id: "ctr-2".to_string(),
                    cgroup_ids: vec!["/kubepods/ctr-2".to_string()],
                    pids: vec![1002],
                },
            ],
        });
        table.update_from_nri(add_event).unwrap();
        assert_eq!(table.container_count(), 2);

        // 移除单个容器（不是整个 Pod）
        let remove_event = NriEvent::RemoveContainer {
            pod_uid: "multi-ctr-pod".to_string(),
            container_id: "ctr-1".to_string(),
        };
        table.update_from_nri(remove_event).unwrap();

        // Pod 仍在，但容器减少
        assert!(table.get_pod_details("multi-ctr-pod").is_some());
        assert_eq!(table.container_count(), 1);
    }

    /// 测试归属查询
    #[tokio::test]
    async fn test_attribution_query() {
        let table = Arc::new(NriMappingTableV2::new());

        // 添加带 cgroup 和 PID 的 Pod
        let event = NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: "attribution-pod".to_string(),
            pod_name: "attr-test".to_string(),
            namespace: "production".to_string(),
            containers: vec![NriContainerInfo {
                container_id: "attr-ctr".to_string(),
                cgroup_ids: vec!["/kubepods/attribution-cgroup".to_string()],
                pids: vec![12345],
            }],
        });
        table.update_from_nri(event).unwrap();

        // 通过 pod_uid 查询
        let info = table.resolve_attribution(Some("attribution-pod"), None, None);
        assert!(info.is_ok());
        assert_eq!(info.unwrap().pod_uid, Some("attribution-pod".to_string()));

        // 通过 cgroup_id 查询
        let info = table.resolve_attribution(None, Some("/kubepods/attribution-cgroup"), None);
        assert!(info.is_ok());
        assert_eq!(info.unwrap().pod_uid, Some("attribution-pod".to_string()));

        // 通过 PID 查询
        let info = table.resolve_attribution(None, None, Some(12345));
        assert!(info.is_ok());
        assert_eq!(info.unwrap().pod_uid, Some("attribution-pod".to_string()));
    }
}
