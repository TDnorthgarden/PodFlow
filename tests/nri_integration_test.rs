//! NRI Integration Tests
//!
//! Tests to verify NRI V3 interaction with containerd
//! These tests can be run in a containerd environment

#[cfg(test)]
mod nri_integration_tests {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;
    use tempfile::TempDir;

    use nuts_observer::collector::nri_mapping_v2::{NriEvent, NriPodEvent, NriContainerInfo};
    use nuts_observer::collector::nri_v3::{NriV3, NriV3Config, CapacityConfig};
    use nuts_observer::collector::nri_persist::PersistConfig;
    use nuts_observer::collector::nri_batch::BatchProcessorConfig;

    /// Test 1: Basic NRI V3 functionality
    #[tokio::test]
    async fn test_nri_v3_basic_functionality() {
        // Create temporary directory for persistence
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test_nri_v3.db");

        // Configure NRI V3 for testing
        let config = NriV3Config {
            persistence: PersistConfig {
                db_path: db_path.to_string_lossy().to_string(),
                snapshot_interval_secs: 60,
                flush_async: true,
                cache_capacity_mb: 128,
            },
            batch: BatchProcessorConfig {
                worker_threads: 1,
                max_queue_depth: 100,
                batch_size: 10,
                max_buffer_ms: 50,
                enable_priority: true,
                delete_priority_boost: 5,
            },
            enable_persistence: true,
            enable_metrics: true,
            capacity: CapacityConfig {
                pods: 100,
                containers: 200,
                cgroups: 200,
                pids: 1000,
            },
        };

        // Create NRI V3 instance
        let nri_v3_result = NriV3::new(config).await;
        assert!(nri_v3_result.is_ok(), "Failed to create NRI V3 instance: {:?}", nri_v3_result.err());

        let nri_v3 = Arc::new(nri_v3_result.unwrap());

        // Test event submission
        let test_pod_uid = "test-integration-pod-001";
        let test_container_id = "test-integration-container-001";

        let add_event = NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: test_pod_uid.to_string(),
            pod_name: "test-integration-app".to_string(),
            namespace: "test-integration".to_string(),
            containers: vec![NriContainerInfo {
                container_id: test_container_id.to_string(),
                cgroup_ids: vec![format!("/kubepods/{}", test_container_id)],
                pids: vec![12345, 12346],
            }],
        });

        let submit_result = nri_v3.try_submit_event(add_event);
        assert!(submit_result.is_ok(), "Failed to submit event: {:?}", submit_result.err());

        // Wait for event processing
        sleep(Duration::from_millis(200)).await;

        // Verify mapping table is updated
        let table = nri_v3.table();
        assert_eq!(table.pod_count(), 1, "Expected 1 pod in mapping table");
        assert_eq!(table.container_count(), 1, "Expected 1 container in mapping table");

        // Test pod retrieval
        let pod_details = table.get_pod_details(test_pod_uid);
        assert!(pod_details.is_some(), "Expected to find pod details");

        let (pod_info, containers) = pod_details.unwrap();
        assert_eq!(pod_info.pod_name, "test-integration-app");
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].container_id, test_container_id);

        // Test container removal
        let remove_event = NriEvent::RemoveContainer {
            pod_uid: test_pod_uid.to_string(),
            container_id: test_container_id.to_string(),
        };

        let remove_result = nri_v3.try_submit_event(remove_event);
        assert!(remove_result.is_ok(), "Failed to submit remove event: {:?}", remove_result.err());

        // Wait for processing
        sleep(Duration::from_millis(200)).await;

        // Verify container is removed but pod remains
        let updated_details = table.get_pod_details(test_pod_uid);
        assert!(updated_details.is_some(), "Pod should still exist after container removal");
        let (_, remaining_containers) = updated_details.unwrap();
        assert_eq!(remaining_containers.len(), 0, "Container should be removed");

        // Test pod deletion
        let delete_event = NriEvent::Delete { 
            pod_uid: test_pod_uid.to_string() 
        };

        let delete_result = nri_v3.try_submit_event(delete_event);
        assert!(delete_result.is_ok(), "Failed to submit delete event: {:?}", delete_result.err());

        // Wait for processing
        sleep(Duration::from_millis(200)).await;

        // Verify pod is completely removed
        let final_details = table.get_pod_details(test_pod_uid);
        assert!(final_details.is_none(), "Pod should be completely removed");

        // Test metrics
        let metrics = nri_v3.metrics();
        let metrics_export = metrics.export_prometheus();
        assert!(metrics_export.contains("nri_events_total"), "Metrics should contain event count");
        assert!(metrics_export.contains("nri_mapping_table_size"), "Metrics should contain table size");

        // Cleanup
        if let Ok(nri_v3) = Arc::try_unwrap(nri_v3) {
            nri_v3.shutdown().await;
        }
    }

    /// Test 2: Batch processing performance
    #[tokio::test]
    async fn test_nri_v3_batch_performance() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("batch_test.db");

        let config = NriV3Config {
            persistence: PersistConfig {
                db_path: db_path.to_string_lossy().to_string(),
                snapshot_interval_secs: 300,
                flush_async: true,
                cache_capacity_mb: 128,
            },
            batch: BatchProcessorConfig {
                worker_threads: 2,
                max_queue_depth: 1000,
                batch_size: 50,
                max_buffer_ms: 100,
                enable_priority: true,
                delete_priority_boost: 10,
            },
            enable_persistence: true,
            enable_metrics: true,
            capacity: CapacityConfig::default(),
        };

        let nri_v3 = Arc::new(NriV3::new(config).await.unwrap());

        // Submit many events rapidly
        let event_count = 200;
        let start_time = std::time::Instant::now();

        for i in 0..event_count {
            let event = NriEvent::AddOrUpdate(NriPodEvent {
                pod_uid: format!("batch-test-pod-{:03}", i),
                pod_name: format!("batch-app-{:03}", i),
                namespace: "batch-test".to_string(),
                containers: vec![NriContainerInfo {
                    container_id: format!("batch-container-{:03}", i),
                    cgroup_ids: vec![format!("/kubepods/batch-container-{:03}", i)],
                    pids: vec![(10000 + i) as u32],
                }],
            });

            nri_v3.try_submit_event(event).unwrap();
        }

        // Wait for batch processing
        sleep(Duration::from_millis(500)).await;

        let processing_time = start_time.elapsed();
        let table = nri_v3.table();

        // Verify all events were processed
        assert_eq!(table.pod_count(), event_count, 
                   "Expected {} pods, got {}", event_count, table.pod_count());
        assert_eq!(table.container_count(), event_count, 
                   "Expected {} containers, got {}", event_count, table.container_count());

        // Verify performance (should be fast due to batching)
        assert!(processing_time < Duration::from_secs(2), 
                  "Batch processing took too long: {:?}", processing_time);

        // Test metrics for batch processing
        let metrics = nri_v3.metrics();
        let metrics_export = metrics.export_prometheus();
        assert!(metrics_export.contains("nri_events_total"), "Should track total events");
        assert!(metrics_export.contains("nri_batch_queue_depth"), "Should track queue depth");

        if let Ok(nri_v3) = Arc::try_unwrap(nri_v3) {
            nri_v3.shutdown().await;
        }
    }

    /// Test 3: Persistence and recovery
    #[tokio::test]
    async fn test_nri_v3_persistence_recovery() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("persistence_test.db");

        let config = NriV3Config {
            persistence: PersistConfig {
                db_path: db_path.to_string_lossy().to_string(),
                snapshot_interval_secs: 1, // Very frequent for testing
                flush_async: true,
                cache_capacity_mb: 128,
            },
            batch: BatchProcessorConfig::default(),
            enable_persistence: true,
            enable_metrics: true,
            capacity: CapacityConfig::default(),
        };

        // Create first instance and add data
        let nri_v3_1 = NriV3::new(config.clone()).await.unwrap();
        let test_pod_uid = "persistence-test-pod";

        let event = NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: test_pod_uid.to_string(),
            pod_name: "persistence-test-app".to_string(),
            namespace: "persistence-test".to_string(),
            containers: vec![NriContainerInfo {
                container_id: "persistence-test-container".to_string(),
                cgroup_ids: vec!["/kubepods/persistence-test-container".to_string()],
                pids: vec![54321],
            }],
        });

        nri_v3_1.try_submit_event(event).unwrap();
        sleep(Duration::from_millis(150)).await; // Allow snapshot

        // Shutdown first instance
        nri_v3_1.shutdown().await;

        // Create second instance (should recover from persistence)
        let nri_v3_2 = NriV3::new(config).await.unwrap();
        let table = nri_v3_2.table();

        // Verify data was recovered
        assert_eq!(table.pod_count(), 1, "Should recover 1 pod from persistence");
        assert_eq!(table.container_count(), 1, "Should recover 1 container from persistence");

        let recovered_details = table.get_pod_details(test_pod_uid);
        assert!(recovered_details.is_some(), "Should recover pod details");

        let (pod_info, containers) = recovered_details.unwrap();
        assert_eq!(pod_info.pod_name, "persistence-test-app");
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].container_id, "persistence-test-container");

        nri_v3_2.shutdown().await;
    }

    /// Test 4: Attribution queries
    #[tokio::test]
    async fn test_nri_v3_attribution_queries() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("attribution_test.db");

        let config = NriV3Config {
            persistence: PersistConfig {
                db_path: db_path.to_string_lossy().to_string(),
                snapshot_interval_secs: 300,
                flush_async: true,
                cache_capacity_mb: 128,
            },
            batch: BatchProcessorConfig::default(),
            enable_persistence: false, // Disable for faster testing
            enable_metrics: true,
            capacity: CapacityConfig::default(),
        };

        let nri_v3 = Arc::new(NriV3::new(config).await.unwrap());
        let table = nri_v3.table();

        // Add test data
        let test_pod_uid = "attribution-test-pod";
        let test_container_id = "attribution-test-container";
        let test_cgroup_id = "/kubepods/attribution-test-container";
        let test_pid = 98765u32;

        let event = NriEvent::AddOrUpdate(NriPodEvent {
            pod_uid: test_pod_uid.to_string(),
            pod_name: "attribution-test-app".to_string(),
            namespace: "attribution-test".to_string(),
            containers: vec![NriContainerInfo {
                container_id: test_container_id.to_string(),
                cgroup_ids: vec![test_cgroup_id.to_string()],
                pids: vec![test_pid],
            }],
        });

        nri_v3.try_submit_event(event).unwrap();
        sleep(Duration::from_millis(100)).await;

        // Test attribution by pod UID
        let attribution_by_pod = table.resolve_attribution(Some(test_pod_uid), None, None);
        assert!(attribution_by_pod.is_ok(), "Should resolve attribution by pod UID");
        let pod_attribution = attribution_by_pod.unwrap();
        assert_eq!(pod_attribution.pod_uid, Some(test_pod_uid.to_string()));

        // Test attribution by cgroup
        let attribution_by_cgroup = table.resolve_attribution(None, Some(test_cgroup_id), None);
        assert!(attribution_by_cgroup.is_ok(), "Should resolve attribution by cgroup");
        let cgroup_attribution = attribution_by_cgroup.unwrap();
        assert_eq!(cgroup_attribution.pod_uid, Some(test_pod_uid.to_string()));

        // Test attribution by PID
        let attribution_by_pid = table.resolve_attribution(None, None, Some(test_pid));
        assert!(attribution_by_pid.is_ok(), "Should resolve attribution by PID");
        let pid_attribution = attribution_by_pid.unwrap();
        assert_eq!(pid_attribution.pod_uid, Some(test_pod_uid.to_string()));

        // Test non-existent attribution
        let missing_attribution = table.resolve_attribution(None, None, Some(99999));
        assert!(missing_attribution.is_ok(), "Should handle missing attribution gracefully");
        let missing_result = missing_attribution.unwrap();
        assert_eq!(missing_result.pod_uid, None);

        if let Ok(nri_v3) = Arc::try_unwrap(nri_v3) {
            nri_v3.shutdown().await;
        }
    }
}
