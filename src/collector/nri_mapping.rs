use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// NRI 归属映射表 - 维护 Pod/容器/cgroup/pid 之间的归属关系
/// 
/// 支持从 NRI 事件实时更新映射，并为证据采集提供归属查询服务
#[derive(Debug, Clone)]
pub struct NriMappingTable {
    /// Pod 映射表: key = pod_uid
    pod_map: Arc<RwLock<HashMap<String, PodInfo>>>,
    /// 容器映射表: key = container_id
    container_map: Arc<RwLock<HashMap<String, ContainerMapping>>>,
    /// cgroup 映射表: key = cgroup_id
    cgroup_map: Arc<RwLock<HashMap<String, CgroupMapping>>>,
    /// PID 映射表: key = pid，用于兜底查询
    pid_map: Arc<RwLock<HashMap<u32, PidMapping>>>,
    /// 最后更新时间戳 (epoch ms)
    last_update_ms: Arc<RwLock<i64>>,
}

/// Pod 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodInfo {
    pub pod_uid: String,
    pub pod_name: String,
    pub namespace: String,
    pub containers: Vec<ContainerMapping>,
}

/// 容器映射信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerMapping {
    pub container_id: String,
    pub pod_uid: String,
    pub cgroup_ids: Vec<String>,
}

/// cgroup 映射信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgroupMapping {
    pub cgroup_id: String,
    pub pod_uid: Option<String>,
    pub container_id: Option<String>,
    pub pids: Vec<u32>,
}

/// PID 映射信息（兜底查询用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidMapping {
    pub pid: u32,
    pub cgroup_id: String,
}

/// 归属查询结果
#[derive(Debug, Clone)]
pub struct AttributionInfo {
    /// Pod UID（可能为 None）
    pub pod_uid: Option<String>,
    /// 容器 ID（可能为 None）
    pub container_id: Option<String>,
    /// cgroup ID
    pub cgroup_id: String,
    /// 归属状态
    pub status: AttributionStatus,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f64,
    /// 归属来源
    pub source: AttributionSource,
    /// 映射版本/时戳
    pub mapping_version: String,
}

/// 归属状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionStatus {
    /// NRI 映射确认
    NriMapped,
    /// PID->cgroup 回退归属
    PidCgroupFallback,
    /// 归属不确定
    Unknown,
}

impl std::fmt::Display for AttributionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributionStatus::NriMapped => write!(f, "nri_mapped"),
            AttributionStatus::PidCgroupFallback => write!(f, "pid_cgroup_fallback"),
            AttributionStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// 归属来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionSource {
    /// NRI 直接提供
    Nri,
    /// PID 回退查询
    PidMap,
    /// 无法确定
    Uncertain,
}

/// NRI 事件类型
#[derive(Debug, Clone)]
pub enum NriEvent {
    /// Pod/容器新增或更新
    AddOrUpdate(NriPodEvent),
    /// Pod/容器删除
    Delete { pod_uid: String },
}

/// NRI Pod 事件详情
#[derive(Debug, Clone)]
pub struct NriPodEvent {
    pub pod_uid: String,
    pub pod_name: String,
    pub namespace: String,
    pub containers: Vec<NriContainerInfo>,
}

/// NRI 容器信息
#[derive(Debug, Clone)]
pub struct NriContainerInfo {
    pub container_id: String,
    pub cgroup_ids: Vec<String>,
    pub pids: Vec<u32>,
}

/// 归属错误类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributionError {
    /// NRI 服务不可用
    NriUnavailable,
    /// 映射缺失
    MappingMissing,
    /// 映射过期
    MappingStale,
    /// 采集期间 Pod 被删除
    PodDeletedDuringWindow,
    /// 归属不确定
    AttributionUncertain,
}

impl std::fmt::Display for AttributionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributionError::NriUnavailable => write!(f, "NRI_UNAVAILABLE"),
            AttributionError::MappingMissing => write!(f, "MAPPING_MISSING"),
            AttributionError::MappingStale => write!(f, "MAPPING_STALE"),
            AttributionError::PodDeletedDuringWindow => write!(f, "POD_DELETED_DURING_WINDOW"),
            AttributionError::AttributionUncertain => write!(f, "ATTRIBUTION_UNCERTAIN"),
        }
    }
}

impl std::error::Error for AttributionError {}

impl Default for NriMappingTable {
    fn default() -> Self {
        Self::new()
    }
}

impl NriMappingTable {
    /// 创建新的映射表
    pub fn new() -> Self {
        Self {
            pod_map: Arc::new(RwLock::new(HashMap::new())),
            container_map: Arc::new(RwLock::new(HashMap::new())),
            cgroup_map: Arc::new(RwLock::new(HashMap::new())),
            pid_map: Arc::new(RwLock::new(HashMap::new())),
            last_update_ms: Arc::new(RwLock::new(0)),
        }
    }

    /// 从 NRI 事件更新映射表
    pub fn update_from_nri(&self, event: NriEvent) -> Result<(), AttributionError> {
        match event {
            NriEvent::AddOrUpdate(pod_event) => {
                self.handle_add_or_update(pod_event)
            }
            NriEvent::Delete { pod_uid } => {
                self.handle_delete(&pod_uid)
            }
        }
    }

    /// 处理 Add/Update 事件
    fn handle_add_or_update(&self, event: NriPodEvent) -> Result<(), AttributionError> {
        let now = chrono::Utc::now().timestamp_millis();

        // 构建 PodInfo
        let containers: Vec<ContainerMapping> = event
            .containers
            .iter()
            .map(|c| ContainerMapping {
                container_id: c.container_id.clone(),
                pod_uid: event.pod_uid.clone(),
                cgroup_ids: c.cgroup_ids.clone(),
            })
            .collect();

        let pod_info = PodInfo {
            pod_uid: event.pod_uid.clone(),
            pod_name: event.pod_name.clone(),
            namespace: event.namespace.clone(),
            containers: containers.clone(),
        };

        // 更新 pod_map
        {
            let mut pod_guard = self.pod_map.write().map_err(|_| AttributionError::NriUnavailable)?;
            pod_guard.insert(event.pod_uid.clone(), pod_info);
        }

        // 更新 container_map 和 cgroup_map
        for container in &event.containers {
            // 更新 container_map
            {
                let mut container_guard = self.container_map.write()
                    .map_err(|_| AttributionError::NriUnavailable)?;
                container_guard.insert(
                    container.container_id.clone(),
                    ContainerMapping {
                        container_id: container.container_id.clone(),
                        pod_uid: event.pod_uid.clone(),
                        cgroup_ids: container.cgroup_ids.clone(),
                    },
                );
            }

            // 更新 cgroup_map 和 pid_map
            for cgroup_id in &container.cgroup_ids {
                {
                    let mut cgroup_guard = self.cgroup_map.write()
                        .map_err(|_| AttributionError::NriUnavailable)?;
                    cgroup_guard.insert(
                        cgroup_id.clone(),
                        CgroupMapping {
                            cgroup_id: cgroup_id.clone(),
                            pod_uid: Some(event.pod_uid.clone()),
                            container_id: Some(container.container_id.clone()),
                            pids: container.pids.clone(),
                        },
                    );
                }

                // 更新 pid_map
                for pid in &container.pids {
                    let mut pid_guard = self.pid_map.write()
                        .map_err(|_| AttributionError::NriUnavailable)?;
                    pid_guard.insert(
                        *pid,
                        PidMapping {
                            pid: *pid,
                            cgroup_id: cgroup_id.clone(),
                        },
                    );
                }
            }
        }

        // 更新最后更新时间
        {
            let mut last_update = self.last_update_ms.write()
                .map_err(|_| AttributionError::NriUnavailable)?;
            *last_update = now;
        }

        Ok(())
    }

    /// 处理 Delete 事件
    fn handle_delete(&self, pod_uid: &str) -> Result<(), AttributionError> {
        // 获取 Pod 信息以清理关联映射
        let pod_info = {
            let pod_guard = self.pod_map.read()
                .map_err(|_| AttributionError::NriUnavailable)?;
            pod_guard.get(pod_uid).cloned()
        };

        if let Some(pod) = pod_info {
            // 清理 container_map, cgroup_map, pid_map
            for container in &pod.containers {
                // 删除 container_map 条目
                {
                    let mut container_guard = self.container_map.write()
                        .map_err(|_| AttributionError::NriUnavailable)?;
                    container_guard.remove(&container.container_id);
                }

                // 删除 cgroup_map 条目
                for cgroup_id in &container.cgroup_ids {
                    {
                        let mut cgroup_guard = self.cgroup_map.write()
                            .map_err(|_| AttributionError::NriUnavailable)?;
                        // 获取 cgroup 信息以清理 pid_map
                        if let Some(cgroup) = cgroup_guard.get(cgroup_id) {
                            let pids_to_remove = cgroup.pids.clone();
                            // 删除关联的 pid_map 条目
                            let mut pid_guard = self.pid_map.write()
                                .map_err(|_| AttributionError::NriUnavailable)?;
                            for pid in &pids_to_remove {
                                pid_guard.remove(pid);
                            }
                        }
                        cgroup_guard.remove(cgroup_id);
                    }
                }
            }
        }

        // 删除 pod_map 条目
        {
            let mut pod_guard = self.pod_map.write()
                .map_err(|_| AttributionError::NriUnavailable)?;
            pod_guard.remove(pod_uid);
        }

        // 更新最后更新时间
        {
            let now = chrono::Utc::now().timestamp_millis();
            let mut last_update = self.last_update_ms.write()
                .map_err(|_| AttributionError::NriUnavailable)?;
            *last_update = now;
        }

        Ok(())
    }

    /// 查询归属信息
    /// 
    /// 优先级：
    /// 1. 如果提供了 pod_uid，直接查询 pod_map
    /// 2. 如果提供了 cgroup_id，查询 cgroup_map -> 反查 pod
    /// 3. 如果提供了 pid，查询 pid_map -> 获取 cgroup -> 反查 pod（兜底）
    pub fn resolve_attribution(
        &self,
        pod_uid: Option<&str>,
        cgroup_id: Option<&str>,
        pid: Option<u32>,
    ) -> Result<AttributionInfo, AttributionError> {
        // 优先级 1: 直接通过 pod_uid 查询
        if let Some(uid) = pod_uid {
            let pod_guard = self.pod_map.read()
                .map_err(|_| AttributionError::NriUnavailable)?;
            
            if let Some(pod) = pod_guard.get(uid) {
                // 获取该 Pod 的第一个 cgroup_id 作为默认 cgroup
                let default_cgroup = pod.containers.first()
                    .and_then(|c| c.cgroup_ids.first())
                    .cloned()
                    .unwrap_or_default();

                return Ok(AttributionInfo {
                    pod_uid: Some(uid.to_string()),
                    container_id: pod.containers.first()
                        .map(|c| c.container_id.clone()),
                    cgroup_id: default_cgroup,
                    status: AttributionStatus::NriMapped,
                    confidence: 0.9,
                    source: AttributionSource::Nri,
                    mapping_version: self.get_last_update().to_string(),
                });
            } else {
                // Pod UID 提供但映射不存在 -> Pod 可能已被删除
                return Err(AttributionError::PodDeletedDuringWindow);
            }
        }

        // 优先级 2: 通过 cgroup_id 查询
        if let Some(cg_id) = cgroup_id {
            let cgroup_guard = self.cgroup_map.read()
                .map_err(|_| AttributionError::NriUnavailable)?;
            
            if let Some(cgroup) = cgroup_guard.get(cg_id) {
                return Ok(AttributionInfo {
                    pod_uid: cgroup.pod_uid.clone(),
                    container_id: cgroup.container_id.clone(),
                    cgroup_id: cg_id.to_string(),
                    status: if cgroup.pod_uid.is_some() {
                        AttributionStatus::NriMapped
                    } else {
                        AttributionStatus::Unknown
                    },
                    confidence: if cgroup.pod_uid.is_some() { 0.9 } else { 0.5 },
                    source: if cgroup.pod_uid.is_some() {
                        AttributionSource::Nri
                    } else {
                        AttributionSource::Uncertain
                    },
                    mapping_version: self.get_last_update().to_string(),
                });
            }
        }

        // 优先级 3: 通过 pid 兜底查询
        if let Some(p) = pid {
            let pid_guard = self.pid_map.read()
                .map_err(|_| AttributionError::NriUnavailable)?;
            
            if let Some(pid_mapping) = pid_guard.get(&p) {
                // 获取到 cgroup_id，进一步查询 pod 信息
                let cgroup_guard = self.cgroup_map.read()
                    .map_err(|_| AttributionError::NriUnavailable)?;
                
                if let Some(cgroup) = cgroup_guard.get(&pid_mapping.cgroup_id) {
                    return Ok(AttributionInfo {
                        pod_uid: cgroup.pod_uid.clone(),
                        container_id: cgroup.container_id.clone(),
                        cgroup_id: pid_mapping.cgroup_id.clone(),
                        status: AttributionStatus::PidCgroupFallback,
                        confidence: 0.6,
                        source: AttributionSource::PidMap,
                        mapping_version: self.get_last_update().to_string(),
                    });
                } else {
                    // 只有 pid->cgroup 映射，没有 cgroup->pod 映射
                    return Ok(AttributionInfo {
                        pod_uid: None,
                        container_id: None,
                        cgroup_id: pid_mapping.cgroup_id.clone(),
                        status: AttributionStatus::PidCgroupFallback,
                        confidence: 0.5,
                        source: AttributionSource::PidMap,
                        mapping_version: self.get_last_update().to_string(),
                    });
                }
            }
        }

        // 所有查询都失败
        Err(AttributionError::MappingMissing)
    }

    /// 生成 scope_key
    /// 
    /// 规则: sha256_hex(pod_uid + "|" + cgroup_id)
    /// 任一字段缺失时用空字符串代替
    pub fn make_scope_key(pod_uid: Option<&str>, cgroup_id: Option<&str>) -> String {
        use sha2::{Digest, Sha256};
        
        let u = pod_uid.unwrap_or("");
        let c = cgroup_id.unwrap_or("");
        
        let mut hasher = Sha256::new();
        hasher.update(format!("{}|{}", u, c));
        format!("{:x}", hasher.finalize())
    }

    /// 获取最后更新时间
    pub fn get_last_update(&self) -> i64 {
        self.last_update_ms.read()
            .map(|v| *v)
            .unwrap_or(0)
    }

    /// 检查映射是否过期 (TTL 默认 30 秒)
    pub fn is_stale(&self, ttl_ms: i64) -> bool {
        let last = self.get_last_update();
        if last == 0 {
            return true; // 从未更新视为过期
        }
        let now = chrono::Utc::now().timestamp_millis();
        (now - last) > ttl_ms
    }

    /// 获取 Pod 数量（用于调试/监控）
    pub fn pod_count(&self) -> usize {
        self.pod_map.read().map(|m| m.len()).unwrap_or(0)
    }

    /// 获取容器数量
    pub fn container_count(&self) -> usize {
        self.container_map.read().map(|m| m.len()).unwrap_or(0)
    }

    /// 获取 cgroup 数量
    pub fn cgroup_count(&self) -> usize {
        self.cgroup_map.read().map(|m| m.len()).unwrap_or(0)
    }

    /// 获取 PID 数量
    pub fn pid_count(&self) -> usize {
        self.pid_map.read().map(|m| m.len()).unwrap_or(0)
    }

    /// 通过 Pod 名称模糊查询（支持前缀匹配）
    /// 
    /// 返回匹配的 Pod 列表，按名称精确度排序
    pub fn find_pods_by_name(&self, name_prefix: &str) -> Vec<PodInfo> {
        let pod_guard = self.pod_map.read().unwrap_or_else(|_| {
            // 如果读锁被污染，创建一个新的空map
            std::panic::resume_unwind(Box::new("RwLock poisoned"))
        });
        
        // 收集匹配的 Pod
        let mut matches: Vec<PodInfo> = pod_guard
            .values()
            .filter(|pod| pod.pod_name.starts_with(name_prefix))
            .cloned()
            .collect();
        
        // 按名称长度排序（更精确的匹配优先）
        matches.sort_by_key(|pod| pod.pod_name.len());
        
        matches
    }

    /// 通过 Pod 名称和命名空间查询（精确匹配）
    pub fn find_pod_by_name_namespace(&self, name: &str, namespace: &str) -> Option<PodInfo> {
        let pod_guard = self.pod_map.read().ok()?;
        
        pod_guard
            .values()
            .find(|pod| pod.pod_name == name && pod.namespace == namespace)
            .cloned()
    }

    /// 获取所有 Pod 列表
    pub fn list_all_pods(&self) -> Vec<PodInfo> {
        let pod_guard = self.pod_map.read().unwrap_or_else(|_| {
            std::panic::resume_unwind(Box::new("RwLock poisoned"))
        });
        
        pod_guard.values().cloned().collect()
    }

    /// 获取 Pod 详细信息（包括容器信息）
    pub fn get_pod_details(&self, pod_uid: &str) -> Option<(PodInfo, Vec<ContainerMapping>)> {
        let pod_guard = self.pod_map.read().ok()?;
        let pod = pod_guard.get(pod_uid)?.clone();
        
        // 获取容器详细信息
        let container_guard = self.container_map.read().ok()?;
        let containers: Vec<ContainerMapping> = pod.containers
            .iter()
            .filter_map(|c| container_guard.get(&c.container_id).cloned())
            .collect();
        
        Some((pod, containers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_scope_key() {
        let key1 = NriMappingTable::make_scope_key(Some("pod-123"), Some("cgroup-456"));
        let key2 = NriMappingTable::make_scope_key(Some("pod-123"), Some("cgroup-456"));
        assert_eq!(key1, key2); // 确定性哈希

        // 缺失字段
        let key3 = NriMappingTable::make_scope_key(None, Some("cgroup-456"));
        let key4 = NriMappingTable::make_scope_key(Some(""), Some("cgroup-456"));
        assert_eq!(key3, key4); // None 和空字符串等价
    }

    #[test]
    fn test_add_update_and_query() {
        let table = NriMappingTable::new();

        // 模拟 NRI Add 事件
        let event = NriPodEvent {
            pod_uid: "pod-test-001".to_string(),
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

        table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();

        // 通过 pod_uid 查询
        let info = table.resolve_attribution(Some("pod-test-001"), None, None).unwrap();
        assert_eq!(info.pod_uid, Some("pod-test-001".to_string()));
        assert_eq!(info.cgroup_id, "cgroup-001");
        assert_eq!(info.status, AttributionStatus::NriMapped);
        assert!(info.confidence > 0.8);

        // 通过 cgroup_id 查询
        let info2 = table.resolve_attribution(None, Some("cgroup-001"), None).unwrap();
        assert_eq!(info2.pod_uid, Some("pod-test-001".to_string()));

        // 通过 pid 兜底查询
        let info3 = table.resolve_attribution(None, None, Some(1001)).unwrap();
        assert_eq!(info3.status, AttributionStatus::PidCgroupFallback);
        assert_eq!(info3.cgroup_id, "cgroup-001");

        // 统计检查
        assert_eq!(table.pod_count(), 1);
        assert_eq!(table.container_count(), 1);
        assert_eq!(table.cgroup_count(), 1);
        assert_eq!(table.pid_count(), 2);
    }

    #[test]
    fn test_delete() {
        let table = NriMappingTable::new();

        // 添加 Pod
        let event = NriPodEvent {
            pod_uid: "pod-delete-test".to_string(),
            pod_name: "delete-me".to_string(),
            namespace: "default".to_string(),
            containers: vec![
                NriContainerInfo {
                    container_id: "container-del".to_string(),
                    cgroup_ids: vec!["cgroup-del".to_string()],
                    pids: vec![2001],
                },
            ],
        };
        table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();

        assert_eq!(table.pod_count(), 1);

        // 删除 Pod
        table.update_from_nri(NriEvent::Delete { 
            pod_uid: "pod-delete-test".to_string() 
        }).unwrap();

        assert_eq!(table.pod_count(), 0);
        assert_eq!(table.container_count(), 0);
        assert_eq!(table.cgroup_count(), 0);
        assert_eq!(table.pid_count(), 0);

        // 删除后再查询应报错
        let result = table.resolve_attribution(Some("pod-delete-test"), None, None);
        assert!(matches!(result, Err(AttributionError::PodDeletedDuringWindow)));
    }

    #[test]
    fn test_mapping_missing() {
        let table = NriMappingTable::new();

        // 查询不存在的映射
        let result = table.resolve_attribution(None, None, None);
        assert!(matches!(result, Err(AttributionError::MappingMissing)));

        let result = table.resolve_attribution(Some("non-existent"), None, None);
        assert!(matches!(result, Err(AttributionError::PodDeletedDuringWindow)));
    }

    #[test]
    fn test_find_pods_by_name() {
        let table = NriMappingTable::new();

        // 添加多个Pod
        let pods = vec![
            ("pod-001", "nginx-app-frontend", "default"),
            ("pod-002", "nginx-app-backend", "default"),
            ("pod-003", "redis-cache", "default"),
            ("pod-004", "nginx-proxy", "kube-system"),
        ];

        for (uid, name, ns) in pods {
            let event = NriPodEvent {
                pod_uid: uid.to_string(),
                pod_name: name.to_string(),
                namespace: ns.to_string(),
                containers: vec![NriContainerInfo {
                    container_id: format!("container-{}", uid),
                    cgroup_ids: vec![format!("cgroup-{}", uid)],
                    pids: vec![1000],
                }],
            };
            table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();
        }

        // 测试前缀模糊匹配
        let matches = table.find_pods_by_name("nginx-app");
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().any(|p| p.pod_uid == "pod-001"));
        assert!(matches.iter().any(|p| p.pod_uid == "pod-002"));

        // 测试更精确的前缀
        let matches = table.find_pods_by_name("nginx-app-frontend");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].pod_uid, "pod-001");

        // 测试无匹配
        let matches = table.find_pods_by_name("postgres");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_pod_by_name_namespace() {
        let table = NriMappingTable::new();

        let event = NriPodEvent {
            pod_uid: "pod-001".to_string(),
            pod_name: "my-app".to_string(),
            namespace: "production".to_string(),
            containers: vec![NriContainerInfo {
                container_id: "container-001".to_string(),
                cgroup_ids: vec!["cgroup-001".to_string()],
                pids: vec![1000],
            }],
        };
        table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();

        // 精确匹配
        let pod = table.find_pod_by_name_namespace("my-app", "production");
        assert!(pod.is_some());
        assert_eq!(pod.unwrap().pod_uid, "pod-001");

        // 命名空间不匹配
        let pod = table.find_pod_by_name_namespace("my-app", "default");
        assert!(pod.is_none());

        // Pod名称不匹配
        let pod = table.find_pod_by_name_namespace("other-app", "production");
        assert!(pod.is_none());
    }

    #[test]
    fn test_list_all_pods() {
        let table = NriMappingTable::new();

        // 初始为空
        let pods = table.list_all_pods();
        assert!(pods.is_empty());

        // 添加Pod
        let event = NriPodEvent {
            pod_uid: "pod-001".to_string(),
            pod_name: "test-pod".to_string(),
            namespace: "default".to_string(),
            containers: vec![NriContainerInfo {
                container_id: "container-001".to_string(),
                cgroup_ids: vec!["cgroup-001".to_string()],
                pids: vec![1000],
            }],
        };
        table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();

        let pods = table.list_all_pods();
        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].pod_uid, "pod-001");
    }

    #[test]
    fn test_get_pod_details() {
        let table = NriMappingTable::new();

        let event = NriPodEvent {
            pod_uid: "pod-001".to_string(),
            pod_name: "test-pod".to_string(),
            namespace: "default".to_string(),
            containers: vec![
                NriContainerInfo {
                    container_id: "container-001".to_string(),
                    cgroup_ids: vec!["cgroup-001".to_string()],
                    pids: vec![1000, 1001],
                },
                NriContainerInfo {
                    container_id: "container-002".to_string(),
                    cgroup_ids: vec!["cgroup-002".to_string()],
                    pids: vec![2000],
                },
            ],
        };
        table.update_from_nri(NriEvent::AddOrUpdate(event)).unwrap();

        // 获取详细信息
        let (pod, containers) = table.get_pod_details("pod-001").unwrap();
        assert_eq!(pod.pod_uid, "pod-001");
        assert_eq!(containers.len(), 2);
        assert!(containers.iter().any(|c| c.container_id == "container-001"));
        assert!(containers.iter().any(|c| c.container_id == "container-002"));

        // 不存在的Pod
        let result = table.get_pod_details("non-existent");
        assert!(result.is_none());
    }
}
