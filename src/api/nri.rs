//! NRI (Node Resource Interface) Webhook 接收模块
//!
//! 接收来自 NRI 的 Pod/容器元信息更新事件，维护归属映射表

use axum::{extract::{Json, Path, Query, State}, routing::{get, post}, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::collector::nri_mapping::{NriMappingTable, NriEvent, NriPodEvent, NriContainerInfo};

/// NRI Pod 事件请求体
#[derive(Debug, Deserialize)]
pub struct NriPodEventRequest {
    /// 事件类型：ADD, UPDATE, DELETE
    pub event_type: String,
    /// Pod UID
    pub pod_uid: String,
    /// Pod 名称
    pub pod_name: String,
    /// 命名空间
    pub namespace: String,
    /// 容器列表
    pub containers: Vec<NriContainerRequest>,
    /// 节点名称（可选）
    #[serde(default)]
    pub node_name: Option<String>,
    /// 标签（可选）
    #[serde(default)]
    pub labels: Option<serde_json::Map<String, serde_json::Value>>,
    /// 注解（可选）
    #[serde(default)]
    pub annotations: Option<serde_json::Map<String, serde_json::Value>>,
}

/// NRI 容器信息请求体
#[derive(Debug, Deserialize)]
pub struct NriContainerRequest {
    /// 容器 ID
    pub container_id: String,
    /// 容器名称
    #[serde(default)]
    pub container_name: Option<String>,
    /// cgroup ID 列表
    pub cgroup_ids: Vec<String>,
    /// 进程 ID 列表（可选）
    #[serde(default)]
    pub pids: Vec<u32>,
    /// 运行时类型（如：runc, crun）
    #[serde(default)]
    pub runtime: Option<String>,
}

/// NRI 事件响应
#[derive(Debug, serde::Serialize)]
pub struct NriEventResponse {
    /// 处理状态
    pub status: String,
    /// 处理消息
    pub message: String,
    /// 映射表统计
    pub stats: Option<NriStats>,
}

/// NRI 映射表统计
#[derive(Debug, serde::Serialize)]
pub struct NriStats {
    pub pod_count: usize,
    pub container_count: usize,
    pub cgroup_count: usize,
    pub pid_count: usize,
}

/// 创建 NRI Webhook 路由
/// 
/// 需要传入共享的 NriMappingTable
pub fn router(nri_table: Arc<NriMappingTable>) -> Router {
    Router::new()
        .route("/v1/nri/events", post(nri_event_handler))
        .route("/v1/nri/pods", get(list_pods_handler))
        .route("/v1/nri/pods/search", get(search_pods_handler))
        .route("/v1/nri/pods/:pod_uid", get(get_pod_handler))
        .with_state(nri_table)
}

/// NRI 事件处理器
async fn nri_event_handler(
    axum::extract::State(nri_table): axum::extract::State<Arc<NriMappingTable>>,
    Json(req): Json<NriPodEventRequest>,
) -> Json<NriEventResponse> {
    tracing::info!(
        "Received NRI event: {} for pod {} in namespace {}",
        req.event_type, req.pod_name, req.namespace
    );

    // 转换请求为内部事件格式
    let containers: Vec<NriContainerInfo> = req.containers
        .into_iter()
        .map(|c| NriContainerInfo {
            container_id: c.container_id,
            cgroup_ids: c.cgroup_ids,
            pids: c.pids,
        })
        .collect();

    let event = match req.event_type.as_str() {
        "ADD" | "UPDATE" | "Add" | "Update" => {
            NriEvent::AddOrUpdate(NriPodEvent {
                pod_uid: req.pod_uid.clone(),
                pod_name: req.pod_name,
                namespace: req.namespace,
                containers,
            })
        }
        "DELETE" | "Delete" => {
            NriEvent::Delete { pod_uid: req.pod_uid.clone() }
        }
        _ => {
            return Json(NriEventResponse {
                status: "error".to_string(),
                message: format!("Unknown event type: {}", req.event_type),
                stats: None,
            });
        }
    };

    // 更新映射表
    match nri_table.update_from_nri(event) {
        Ok(()) => {
            let stats = NriStats {
                pod_count: nri_table.pod_count(),
                container_count: nri_table.container_count(),
                cgroup_count: nri_table.cgroup_count(),
                pid_count: nri_table.pid_count(),
            };

            tracing::info!(
                "NRI event processed successfully. Stats: pods={}, containers={}, cgroups={}, pids={}",
                stats.pod_count, stats.container_count, stats.cgroup_count, stats.pid_count
            );

            Json(NriEventResponse {
                status: "success".to_string(),
                message: format!("Event {} for pod {} processed", req.event_type, req.pod_uid),
                stats: Some(stats),
            })
        }
        Err(e) => {
            tracing::error!("Failed to process NRI event: {:?}", e);
            
            Json(NriEventResponse {
                status: "error".to_string(),
                message: format!("Failed to update mapping table: {:?}", e),
                stats: None,
            })
        }
    }
}

/// 查询映射表状态
/// 
/// 用于健康检查和调试
pub async fn get_mapping_stats(
    axum::extract::State(nri_table): axum::extract::State<Arc<NriMappingTable>>,
) -> Json<NriStats> {
    Json(NriStats {
        pod_count: nri_table.pod_count(),
        container_count: nri_table.container_count(),
        cgroup_count: nri_table.cgroup_count(),
        pid_count: nri_table.pid_count(),
    })
}

/// Pod列表响应
#[derive(Debug, Serialize)]
pub struct PodListResponse {
    pub pods: Vec<PodSummary>,
    pub total: usize,
}

/// Pod摘要信息
#[derive(Debug, Serialize)]
pub struct PodSummary {
    pub pod_uid: String,
    pub pod_name: String,
    pub namespace: String,
    pub container_count: usize,
}

/// Pod详细信息响应
#[derive(Debug, Serialize)]
pub struct PodDetailResponse {
    pub pod_uid: String,
    pub pod_name: String,
    pub namespace: String,
    pub containers: Vec<ContainerDetail>,
}

/// 容器详细信息
#[derive(Debug, Serialize)]
pub struct ContainerDetail {
    pub container_id: String,
    pub cgroup_ids: Vec<String>,
}

/// Pod搜索查询参数
#[derive(Debug, Deserialize)]
pub struct PodSearchQuery {
    /// Pod名称前缀（模糊匹配）
    pub name_prefix: Option<String>,
    /// 精确匹配Pod名称
    pub name: Option<String>,
    /// 命名空间
    pub namespace: Option<String>,
}

/// 列出所有Pod
async fn list_pods_handler(
    State(nri_table): State<Arc<NriMappingTable>>,
) -> Json<PodListResponse> {
    let pods = nri_table.list_all_pods();
    let summaries: Vec<PodSummary> = pods
        .into_iter()
        .map(|pod| PodSummary {
            pod_uid: pod.pod_uid,
            pod_name: pod.pod_name,
            namespace: pod.namespace,
            container_count: pod.containers.len(),
        })
        .collect();
    
    Json(PodListResponse {
        total: summaries.len(),
        pods: summaries,
    })
}

/// 搜索Pod（支持模糊匹配）
async fn search_pods_handler(
    State(nri_table): State<Arc<NriMappingTable>>,
    Query(query): Query<PodSearchQuery>,
) -> Json<PodListResponse> {
    let pods = if let Some(name) = &query.name {
        // 精确匹配模式
        if let Some(pod) = nri_table.find_pod_by_name_namespace(name, query.namespace.as_deref().unwrap_or("default")) {
            vec![pod]
        } else {
            vec![]
        }
    } else if let Some(prefix) = &query.name_prefix {
        // 前缀模糊匹配模式
        nri_table.find_pods_by_name(prefix)
    } else {
        // 命名空间过滤模式
        nri_table.list_all_pods()
            .into_iter()
            .filter(|pod| {
                if let Some(ns) = &query.namespace {
                    pod.namespace == *ns
                } else {
                    true
                }
            })
            .collect()
    };
    
    let summaries: Vec<PodSummary> = pods
        .into_iter()
        .map(|pod| PodSummary {
            pod_uid: pod.pod_uid.clone(),
            pod_name: pod.pod_name,
            namespace: pod.namespace,
            container_count: pod.containers.len(),
        })
        .collect();
    
    Json(PodListResponse {
        total: summaries.len(),
        pods: summaries,
    })
}

/// 获取Pod详细信息
async fn get_pod_handler(
    State(nri_table): State<Arc<NriMappingTable>>,
    Path(pod_uid): Path<String>,
) -> Result<Json<PodDetailResponse>, axum::http::StatusCode> {
    let (pod, containers) = nri_table
        .get_pod_details(&pod_uid)
        .ok_or(axum::http::StatusCode::NOT_FOUND)?;
    
    let container_details: Vec<ContainerDetail> = containers
        .into_iter()
        .map(|c| ContainerDetail {
            container_id: c.container_id,
            cgroup_ids: c.cgroup_ids,
        })
        .collect();
    
    Ok(Json(PodDetailResponse {
        pod_uid: pod.pod_uid,
        pod_name: pod.pod_name,
        namespace: pod.namespace,
        containers: container_details,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nri_event_conversion() {
        let nri_table = Arc::new(NriMappingTable::new());
        
        // 模拟 ADD 事件
        let event = NriPodEvent {
            pod_uid: "test-pod-001".to_string(),
            pod_name: "test-pod".to_string(),
            namespace: "default".to_string(),
            containers: vec![
                NriContainerInfo {
                    container_id: "container-001".to_string(),
                    cgroup_ids: vec!["cgroup-001".to_string()],
                    pids: vec![1001, 1002],
                },
            ],
        };
        
        nri_table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();
        
        // 验证统计
        assert_eq!(nri_table.pod_count(), 1);
        assert_eq!(nri_table.container_count(), 1);
        assert_eq!(nri_table.cgroup_count(), 1);
        assert_eq!(nri_table.pid_count(), 2);
        
        // 验证查询
        let info = nri_table.resolve_attribution(Some("test-pod-001"), None, None).unwrap();
        assert_eq!(info.pod_uid, Some("test-pod-001".to_string()));
    }
}
