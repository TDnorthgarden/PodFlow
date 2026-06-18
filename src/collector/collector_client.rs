//! Collector Client - 连接到特权采集守护进程
//!
//! 为非特权的 podflow 提供访问 collector daemon 的接口。
//! 使用 gRPC over Unix Socket 进行通信。

use std::path::Path;
use tonic::transport::Channel;
use tracing::{info, warn, debug, error};

// 引入生成的 protobuf 代码用于类型定义
use crate::collector::proto::collector_client::CollectorClient;
use crate::collector::proto::{
    CollectRequest, ReadProcRequest, CancelRequest, HealthRequest, PermissionCheckRequest,
    CollectResponse, ReadProcResponse, CancelResponse, HealthResponse, PermissionCheckResponse,
};

/// 采集器客户端错误
#[derive(Debug)]
pub enum CollectorClientError {
    /// 连接失败
    ConnectionError(String),
    /// 请求被拒绝（权限不足）
    PermissionDenied(String),
    /// 采集超时
    Timeout,
    /// 守护进程不可用
    DaemonUnavailable,
    /// 其他错误
    Other(String),
}

impl std::fmt::Display for CollectorClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            Self::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            Self::Timeout => write!(f, "Collection timed out"),
            Self::DaemonUnavailable => write!(f, "Collector daemon not available"),
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CollectorClientError {}

/// 采集器客户端
pub struct CollectorClientWrapper {
    socket_path: String,
    client: Option<CollectorClient<tonic::transport::Channel>>,
    #[allow(dead_code)]
    connected: bool,
}

impl CollectorClientWrapper {
    /// 连接到 collector daemon
    /// 
    /// # Arguments
    /// * `socket_path` - Unix Socket 路径，默认为 "/run/podflow/collector.sock"
    pub async fn connect(socket_path: &str) -> Result<Self, CollectorClientError> {
        // 检查 socket 文件是否存在
        if !Path::new(socket_path).exists() {
            return Err(CollectorClientError::DaemonUnavailable);
        }

        // 构建 Unix Socket 连接
        let endpoint = Channel::from_shared("unix://".to_string() + socket_path)
            .map_err(|e| CollectorClientError::ConnectionError(
                format!("Invalid socket endpoint: {}", e)
            ))?;

        let channel = endpoint.connect()
            .await
            .map_err(|e| CollectorClientError::ConnectionError(
                format!("Failed to connect to Unix socket: {}", e)
            ))?;

        let client = CollectorClient::new(channel);

        info!("Connected to collector daemon at {}", socket_path);
        
        Ok(Self {
            socket_path: socket_path.to_string(),
            client: Some(client),
            connected: true,
        })
    }

    /// 尝试连接，如果失败则返回 None（用于检测 daemon 是否可用）
    pub async fn try_connect(socket_path: &str) -> Option<Self> {
        match Self::connect(socket_path).await {
            Ok(client) => Some(client),
            Err(e) => {
                warn!("Failed to connect to collector daemon: {}", e);
                None
            }
        }
    }

    /// 执行 bpftrace 采集
    pub async fn collect_bpftrace(
        &mut self,
        task_id: &str,
        script_path: &str,
        duration_secs: u64,
        scope_pid: Option<u32>,
        evidence_type: &str,
    ) -> Result<CollectResponse, CollectorClientError> {
        let client = self.client.as_mut()
            .ok_or_else(|| CollectorClientError::ConnectionError("Not connected".to_string()))?;

        let request = tonic::Request::new(CollectRequest {
            task_id: task_id.to_string(),
            script_path: script_path.to_string(),
            script_content: String::new(),
            duration_secs,
            scope_pid,
            cgroup_id: None,
            evidence_type: evidence_type.to_string(),
            params: std::collections::HashMap::new(),
        });

        debug!("Sending collect_bpftrace request: task_id={}, script={}", task_id, script_path);

        match client.collect_bpftrace(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                info!("Collection completed: id={}, status={}", resp.collection_id, resp.status);
                Ok(resp)
            }
            Err(status) => {
                error!("Collection failed: {}", status);
                Err(CollectorClientError::Other(format!("gRPC error: {}", status)))
            }
        }
    }

    /// 读取 /proc 文件
    pub async fn read_proc(
        &mut self,
        task_id: &str,
        path: &str,
        pid: Option<u32>,
    ) -> Result<ReadProcResponse, CollectorClientError> {
        let client = self.client.as_mut()
            .ok_or_else(|| CollectorClientError::ConnectionError("Not connected".to_string()))?;

        let request = tonic::Request::new(ReadProcRequest {
            task_id: task_id.to_string(),
            path: path.to_string(),
            pid,
            follow_symlink: false,
        });

        debug!("Sending read_proc request: task_id={}, path={}", task_id, path);

        match client.read_proc(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                info!("Read proc completed: path={}, exists={}", path, resp.exists);
                Ok(resp)
            }
            Err(status) => {
                error!("Read proc failed: {}", status);
                Err(CollectorClientError::Other(format!("gRPC error: {}", status)))
            }
        }
    }

    /// 取消正在进行的采集
    pub async fn cancel_collection(
        &mut self,
        collection_id: &str,
        reason: &str,
    ) -> Result<CancelResponse, CollectorClientError> {
        let client = self.client.as_mut()
            .ok_or_else(|| CollectorClientError::ConnectionError("Not connected".to_string()))?;

        let request = tonic::Request::new(CancelRequest {
            collection_id: collection_id.to_string(),
            reason: reason.to_string(),
        });

        debug!("Sending cancel_collection request: collection_id={}", collection_id);

        match client.cancel_collection(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                info!("Collection cancelled: id={}, success={}", collection_id, resp.success);
                Ok(resp)
            }
            Err(status) => {
                error!("Cancel collection failed: {}", status);
                Err(CollectorClientError::Other(format!("gRPC error: {}", status)))
            }
        }
    }

    /// 健康检查
    pub async fn health(&mut self, include_stats: bool) -> Result<HealthResponse, CollectorClientError> {
        let client = self.client.as_mut()
            .ok_or_else(|| CollectorClientError::ConnectionError("Not connected".to_string()))?;

        let request = tonic::Request::new(HealthRequest {
            include_stats,
        });

        debug!("Sending health request: include_stats={}", include_stats);

        match client.health(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                info!("Health check completed: healthy={}", resp.healthy);
                Ok(resp)
            }
            Err(status) => {
                error!("Health check failed: {}", status);
                Err(CollectorClientError::Other(format!("gRPC error: {}", status)))
            }
        }
    }

    /// 检查当前 UID 的权限
    pub async fn check_permission(&mut self, uid: u32) -> Result<PermissionCheckResponse, CollectorClientError> {
        let client = self.client.as_mut()
            .ok_or_else(|| CollectorClientError::ConnectionError("Not connected".to_string()))?;

        let request = tonic::Request::new(PermissionCheckRequest {
            uid,
        });

        debug!("Sending check_permission request: uid={}", uid);

        match client.check_permission(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                info!("Permission check completed: allowed={}", resp.allowed);
                Ok(resp)
            }
            Err(status) => {
                error!("Permission check failed: {}", status);
                Err(CollectorClientError::Other(format!("gRPC error: {}", status)))
            }
        }
    }

    /// 获取 socket 路径
    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }
}

/// 自动回退的采集器
/// 
/// 优先使用 daemon，如果不可用则回退到开发模式（直接执行）
pub struct AutoFallbackCollector {
    client: Option<CollectorClientWrapper>,
    #[allow(dead_code)]
    socket_path: String,
    allow_dev_mode: bool,
}

impl AutoFallbackCollector {
    /// 创建新的自动回退采集器
    pub async fn new(socket_path: &str, allow_dev_mode: bool) -> Self {
        let client = CollectorClientWrapper::try_connect(socket_path).await;
        
        if client.is_none() && allow_dev_mode {
            warn!("Collector daemon not available, will use dev mode (direct execution)");
        }

        Self {
            client,
            socket_path: socket_path.to_string(),
            allow_dev_mode,
        }
    }

    /// 检查是否使用 daemon 模式
    pub fn is_daemon_mode(&self) -> bool {
        // 如果有客户端连接，则使用 daemon 模式
        self.client.is_some()
    }

    /// 执行采集
    pub async fn collect(
        &mut self,
        task_id: &str,
        script_path: &str,
        duration_secs: u64,
        scope_pid: Option<u32>,
        evidence_type: &str,
    ) -> Result<CollectResponse, CollectorClientError> {
        match &mut self.client {
            Some(client) => {
                // 使用 daemon 模式
                client.collect_bpftrace(task_id, script_path, duration_secs, scope_pid, evidence_type).await
            }
            None if self.allow_dev_mode => {
                // 回退到开发模式
                self.collect_dev_mode(task_id, script_path, duration_secs, scope_pid, evidence_type).await
            }
            None => {
                // 无 daemon 且不允许开发模式
                Err(CollectorClientError::DaemonUnavailable)
            }
        }
    }

    /// 开发模式采集（直接执行 bpftrace）
    async fn collect_dev_mode(
        &mut self,
        _task_id: &str,
        script_path: &str,
        duration_secs: u64,
        _scope_pid: Option<u32>,
        _evidence_type: &str,
    ) -> Result<CollectResponse, CollectorClientError> {
        use tokio::process::Command;
        use tokio::time::{timeout, Duration};

        warn!("Using dev mode: executing bpftrace directly with sudo");

        let output = timeout(
            Duration::from_secs(duration_secs + 5), // 额外5秒缓冲
            Command::new("sudo")
                .args(["bpftrace", script_path])
                .output()
        )
        .await
        .map_err(|_| CollectorClientError::Timeout)?
        .map_err(|e| CollectorClientError::Other(format!("Failed to execute: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let event_count = stdout.lines().count() as u32;

        Ok(CollectResponse {
            collection_id: format!("dev-mode-{}", uuid::Uuid::new_v4()),
            raw_output: stdout.as_bytes().to_vec(),
            duration_ms: duration_secs * 1000, // 估算
            status: if output.status.success() { "success".to_string() } else { "error".to_string() },
            error_msg: if output.status.success() {
                None
            } else {
                Some(String::from_utf8_lossy(&output.stderr).to_string())
            },
            event_count,
            bytes_collected: stdout.len() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_error_display() {
        let err = CollectorClientError::PermissionDenied("test".to_string());
        assert_eq!(err.to_string(), "Permission denied: test");
    }
}
