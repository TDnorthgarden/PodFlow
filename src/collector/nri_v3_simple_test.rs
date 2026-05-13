//! Simple V3 API test to verify type compatibility

#[cfg(test)]
mod tests {
    use crate::collector::nri_mapping_v2::{NriMappingTableV2, NriEvent, NriPodEvent};
    use crate::collector::nri_v3::{NriV3, NriV3Config, CapacityConfig};
    use crate::collector::nri_persist::PersistConfig;
    use crate::collector::nri_batch::BatchProcessorConfig;

    #[tokio::test]
    async fn test_v3_api_types() {
        // Test that V3 API types are compatible
        let config = NriV3Config {
            persistence: PersistConfig::default(),
            batch: BatchProcessorConfig::default(),
            enable_persistence: false,
            enable_metrics: false,
            capacity: CapacityConfig::default(),
        };

        // This should compile without type errors
        let nri_v3_result = NriV3::new(config).await;
        
        match nri_v3_result {
            Ok(v3) => {
                // Test that we can create events and submit them
                let event = NriEvent::AddOrUpdate(NriPodEvent {
                    pod_uid: "test-pod".to_string(),
                    pod_name: "test-pod".to_string(),
                    namespace: "default".to_string(),
                    containers: vec![],
                });

                let submit_result = v3.try_submit_event(event);
                assert!(submit_result.is_ok(), "Event submission should succeed");

                // Test table access
                let table = v3.table();
                assert_eq!(table.pod_count(), 0, "Should have no pods initially");

                v3.flush().await;
                v3.shutdown().await;
            }
            Err(e) => {
                panic!("Failed to create NriV3: {:?}", e);
            }
        }
    }
}
