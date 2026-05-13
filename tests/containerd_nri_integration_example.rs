//! Containerd NRI 集成测试示例
//!
//! 这个文件展示了如何编写 NRI 集成测试
//! 可以复制到 tests/ 目录中使用

#[cfg(test)]
mod containerd_nri_integration_tests {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    // 假设这些模块已导入
    // use nuts_observer::collector::nri_mapping_v2::{NriMappingTableV2, PodInfo, ContainerInfo};
    // use nuts_observer::collector::nri_v3::{NriV3, NriV3Config};
    // use nuts_observer::types::error::NutsError;

    /// 测试 1: 基础 NRI 事件提交
    ///
    /// 验证:
    /// - Pod 事件可以成功提交
    /// - 映射表正确更新
    /// - 查询接口返回正确数据
    #[tokio::test]
    async fn test_nri_basic_pod_submission() {
        // 1. 创建映射表
        // let table = Arc::new(NriMappingTableV2::new());

        // 2. 提交 Pod 事件
        // let pod_info = PodInfo {
        //     pod_uid: "test-pod-001".to_string(),
        //     pod_name: "test-app".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![ContainerInfo {
        //         container_id: "container-001".to_string(),
        //         name: "main".to_string(),
        //         image: "nginx:latest".to_string(),
        //     }],
        // };
        // table.insert_pod(pod_info);

        // 3. 验证查询
        // let result = table.get_pod("test-pod-001");
        // assert!(result.is_some());
        // let pod = result.unwrap();
        // assert_eq!(pod.pod_name, "test-app");
        // assert_eq!(pod.containers.len(), 1);
    }

    /// 测试 2: 容器生命周期事件
    ///
    /// 验证:
    /// - 容器创建事件处理
    /// - 容器更新事件处理
    /// - 容器删除事件处理
    /// - 映射表一致性
    #[tokio::test]
    async fn test_container_lifecycle_events() {
        // 1. 创建 Pod
        // let table = Arc::new(NriMappingTableV2::new());
        // let pod_uid = "lifecycle-test-pod".to_string();

        // 2. 添加容器
        // let pod_info = PodInfo {
        //     pod_uid: pod_uid.clone(),
        //     pod_name: "lifecycle-app".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![ContainerInfo {
        //         container_id: "container-1".to_string(),
        //         name: "main".to_string(),
        //         image: "nginx:latest".to_string(),
        //     }],
        // };
        // table.insert_pod(pod_info);

        // 3. 验证容器已添加
        // let pod = table.get_pod(&pod_uid).unwrap();
        // assert_eq!(pod.containers.len(), 1);

        // 4. 更新容器（添加新容器）
        // let updated_pod = PodInfo {
        //     pod_uid: pod_uid.clone(),
        //     pod_name: "lifecycle-app".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![
        //         ContainerInfo {
        //             container_id: "container-1".to_string(),
        //             name: "main".to_string(),
        //             image: "nginx:latest".to_string(),
        //         },
        //         ContainerInfo {
        //             container_id: "container-2".to_string(),
        //             name: "sidecar".to_string(),
        //             image: "sidecar:latest".to_string(),
        //         },
        //     ],
        // };
        // table.insert_pod(updated_pod);

        // 5. 验证容器已更新
        // let pod = table.get_pod(&pod_uid).unwrap();
        // assert_eq!(pod.containers.len(), 2);

        // 6. 删除 Pod
        // table.remove_pod(&pod_uid);

        // 7. 验证 Pod 已删除
        // assert!(table.get_pod(&pod_uid).is_none());
    }

    /// 测试 3: 并发事件处理
    ///
    /// 验证:
    /// - 多个 Pod 并发提交
    /// - 数据一致性
    /// - 性能指标
    #[tokio::test]
    async fn test_concurrent_pod_events() {
        // let table = Arc::new(NriMappingTableV2::new());
        // let mut handles = vec![];

        // // 创建 10 个并发任务，每个提交 100 个 Pod
        // for i in 0..10 {
        //     let table_clone = Arc::clone(&table);
        //     let handle = tokio::spawn(async move {
        //         for j in 0..100 {
        //             let pod_uid = format!("concurrent-pod-{}-{}", i, j);
        //             let pod_info = PodInfo {
        //                 pod_uid: pod_uid.clone(),
        //                 pod_name: format!("app-{}", j),
        //                 namespace: "default".to_string(),
        //                 containers: vec![ContainerInfo {
        //                     container_id: format!("container-{}-{}", i, j),
        //                     name: "main".to_string(),
        //                     image: "nginx:latest".to_string(),
        //                 }],
        //             };
        //             table_clone.insert_pod(pod_info);
        //         }
        //     });
        //     handles.push(handle);
        // }

        // // 等待所有任务完成
        // for handle in handles {
        //     handle.await.unwrap();
        // }

        // // 验证总数
        // let stats = table.stats();
        // assert_eq!(stats.pod_count, 1000);
    }

    /// 测试 4: 批量事件处理
    ///
    /// 验证:
    /// - 批量提交事件
    /// - 事件聚合
    /// - 处理延迟
    #[tokio::test]
    async fn test_batch_event_processing() {
        // let table = Arc::new(NriMappingTableV2::new());

        // // 创建批量事件
        // let mut pods = vec![];
        // for i in 0..100 {
        //     let pod_info = PodInfo {
        //         pod_uid: format!("batch-pod-{}", i),
        //         pod_name: format!("app-{}", i),
        //         namespace: "default".to_string(),
        //         containers: vec![ContainerInfo {
        //             container_id: format!("container-{}", i),
        //             name: "main".to_string(),
        //             image: "nginx:latest".to_string(),
        //         }],
        //     };
        //     pods.push(pod_info);
        // }

        // // 批量插入
        // let start = std::time::Instant::now();
        // for pod in pods {
        //     table.insert_pod(pod);
        // }
        // let elapsed = start.elapsed();

        // // 验证
        // let stats = table.stats();
        // assert_eq!(stats.pod_count, 100);

        // // 性能检查：100 个 Pod 应该在 100ms 内完成
        // assert!(elapsed < Duration::from_millis(100),
        //     "Batch processing took {:?}, expected < 100ms", elapsed);
    }

    /// 测试 5: 映射表查询性能
    ///
    /// 验证:
    /// - 查询延迟
    /// - 查询准确性
    /// - 大规模数据集性能
    #[tokio::test]
    async fn test_mapping_table_query_performance() {
        // let table = Arc::new(NriMappingTableV2::new());

        // // 插入 1000 个 Pod
        // for i in 0..1000 {
        //     let pod_info = PodInfo {
        //         pod_uid: format!("perf-pod-{}", i),
        //         pod_name: format!("app-{}", i),
        //         namespace: "default".to_string(),
        //         containers: vec![],
        //     };
        //     table.insert_pod(pod_info);
        // }

        // // 测试查询性能
        // let start = std::time::Instant::now();
        // for i in 0..1000 {
        //     let pod_uid = format!("perf-pod-{}", i);
        //     let result = table.get_pod(&pod_uid);
        //     assert!(result.is_some());
        // }
        // let elapsed = start.elapsed();

        // // 性能检查：1000 次查询应该在 10ms 内完成
        // assert!(elapsed < Duration::from_millis(10),
        //     "Query performance: {:?}, expected < 10ms", elapsed);
    }

    /// 测试 6: 错误处理和恢复
    ///
    /// 验证:
    /// - 重复提交处理
    /// - 无效数据处理
    /// - 恢复机制
    #[tokio::test]
    async fn test_error_handling_and_recovery() {
        // let table = Arc::new(NriMappingTableV2::new());

        // // 1. 提交有效 Pod
        // let pod_info = PodInfo {
        //     pod_uid: "error-test-pod".to_string(),
        //     pod_name: "error-app".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![],
        // };
        // table.insert_pod(pod_info.clone());

        // // 2. 重复提交相同 Pod（应该覆盖）
        // table.insert_pod(pod_info);

        // // 3. 验证只有一个 Pod
        // let stats = table.stats();
        // assert_eq!(stats.pod_count, 1);

        // // 4. 删除 Pod
        // table.remove_pod("error-test-pod");

        // // 5. 验证删除成功
        // assert!(table.get_pod("error-test-pod").is_none());

        // // 6. 重新添加 Pod（恢复）
        // let recovered_pod = PodInfo {
        //     pod_uid: "error-test-pod".to_string(),
        //     pod_name: "recovered-app".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![],
        // };
        // table.insert_pod(recovered_pod);

        // // 7. 验证恢复成功
        // let pod = table.get_pod("error-test-pod").unwrap();
        // assert_eq!(pod.pod_name, "recovered-app");
    }

    /// 测试 7: 内存使用和泄漏检测
    ///
    /// 验证:
    /// - 内存使用合理
    /// - 无内存泄漏
    /// - 删除操作释放内存
    #[tokio::test]
    async fn test_memory_usage_and_leak_detection() {
        // let table = Arc::new(NriMappingTableV2::new());

        // // 1. 获取初始内存使用
        // let initial_stats = table.stats();

        // // 2. 插入大量 Pod
        // for i in 0..10000 {
        //     let pod_info = PodInfo {
        //         pod_uid: format!("memory-test-pod-{}", i),
        //         pod_name: format!("app-{}", i),
        //         namespace: "default".to_string(),
        //         containers: vec![],
        //     };
        //     table.insert_pod(pod_info);
        // }

        // // 3. 获取插入后的统计
        // let after_insert = table.stats();
        // assert_eq!(after_insert.pod_count, 10000);

        // // 4. 删除所有 Pod
        // for i in 0..10000 {
        //     table.remove_pod(&format!("memory-test-pod-{}", i));
        // }

        // // 5. 获取删除后的统计
        // let after_delete = table.stats();
        // assert_eq!(after_delete.pod_count, 0);

        // // 6. 验证内存已释放（Pod 数量为 0）
        // // 注意：实际内存释放可能需要垃圾回收
    }

    /// 测试 8: 事件顺序和一致性
    ///
    /// 验证:
    /// - 事件处理顺序
    /// - 数据一致性
    /// - 版本控制
    #[tokio::test]
    async fn test_event_ordering_and_consistency() {
        // let table = Arc::new(NriMappingTableV2::new());

        // // 1. 创建 Pod
        // let pod_uid = "order-test-pod".to_string();
        // let pod_v1 = PodInfo {
        //     pod_uid: pod_uid.clone(),
        //     pod_name: "app-v1".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![],
        // };
        // table.insert_pod(pod_v1);

        // // 2. 更新 Pod（版本 2）
        // let pod_v2 = PodInfo {
        //     pod_uid: pod_uid.clone(),
        //     pod_name: "app-v2".to_string(),
        //     namespace: "default".to_string(),
        //     containers: vec![ContainerInfo {
        //         container_id: "container-1".to_string(),
        //         name: "main".to_string(),
        //         image: "nginx:latest".to_string(),
        //     }],
        // };
        // table.insert_pod(pod_v2);

        // // 3. 验证最新版本
        // let pod = table.get_pod(&pod_uid).unwrap();
        // assert_eq!(pod.pod_name, "app-v2");
        // assert_eq!(pod.containers.len(), 1);
    }

    /// 测试 9: 与 API 端点的集成
    ///
    /// 验证:
    /// - HTTP API 正确调用映射表
    /// - 请求/响应格式正确
    /// - 错误处理正确
    #[tokio::test]
    async fn test_api_integration() {
        // 这个测试需要启动 HTTP 服务器
        // 可以使用 axum 的测试工具

        // 1. 创建测试应用
        // let app = create_test_app();

        // 2. 发送 POST 请求提交 Pod
        // let request = Request::builder()
        //     .method("POST")
        //     .uri("/api/v3/nri/batch")
        //     .header("content-type", "application/json")
        //     .body(Body::from(r#"{"events":[...]}"#))
        //     .unwrap();

        // 3. 验证响应
        // let response = app.oneshot(request).await.unwrap();
        // assert_eq!(response.status(), StatusCode::OK);
    }

    /// 测试 10: 压力测试
    ///
    /// 验证:
    /// - 系统在高负载下的表现
    /// - 响应时间
    /// - 资源使用
    #[tokio::test]
    async fn test_stress_high_volume_events() {
        // let table = Arc::new(NriMappingTableV2::new());
        // let mut handles = vec![];

        // // 创建 100 个并发任务
        // for task_id in 0..100 {
        //     let table_clone = Arc::clone(&table);
        //     let handle = tokio::spawn(async move {
        //         // 每个任务提交 1000 个 Pod
        //         for i in 0..1000 {
        //             let pod_uid = format!("stress-pod-{}-{}", task_id, i);
        //             let pod_info = PodInfo {
        //                 pod_uid: pod_uid.clone(),
        //                 pod_name: format!("app-{}", i),
        //                 namespace: "default".to_string(),
        //                 containers: vec![],
        //             };
        //             table_clone.insert_pod(pod_info);
        //         }
        //     });
        //     handles.push(handle);
        // }

        // // 等待所有任务完成
        // for handle in handles {
        //     handle.await.unwrap();
        // }

        // // 验证总数
        // let stats = table.stats();
        // assert_eq!(stats.pod_count, 100000);
    }
}

// 使用说明：
// 1. 将此文件复制到 tests/ 目录
// 2. 取消注释代码中的 use 语句和测试代码
// 3. 运行: cargo test --test containerd_nri_integration_tests
// 4. 查看测试结果和性能指标
