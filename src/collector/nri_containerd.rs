//! Containerd NRI 官方协议 gRPC 适配器
//!
//! 实现 containerd NRI (Node Resource Interface) 官方 gRPC 协议
//! 参考: https://github.com/containerd/nri
//!
//! 这个模块实现了 Plugin 服务接口，接收来自 containerd 的事件：
//! - Configure: 运行时配置插件
//! - Synchronize: 同步运行时状态
//! - CreateContainer: 容器创建事件
//! - UpdateContainer: 容器更新事件
//! - StopContainer: 容器停止事件
//!
//! 同时实现了 Runtime 客户端接口，用于向 containerd 注册插件。

use crate::types::error::NutsError;
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn, instrument};

// 从 protobuf 生成的代码
pub mod nri_proto {
    tonic::include_proto!("nri.plugin.v1");
}

use nri_proto::{
    ConfigureRequest, ConfigureResponse,
    CreateContainerRequest, CreateContainerResponse,
    UpdateContainerRequest, UpdateContainerResponse,
    StopContainerRequest, StopContainerResponse,
    SynchronizeRequest, SynchronizeResponse,
    RegisterPluginRequest, RegisterPluginResponse,
    ContainerUpdate, LinuxResources,
    plugin_server::{Plugin, PluginServer},
    runtime_client::RuntimeClient,
};

use super::nri_mapping::{NriContainerInfo, NriEvent, NriPodEvent};
use super::nri_mapping_v2::NriMappingTableV2;

/// Containerd NRI Plugin 配置
#[derive(Debug, Clone)]
pub struct ContainerdNriConfig {
    /// Unix Socket 路径（containerd NRI 标准路径）
    pub socket_path: String,
    /// 插件名称
    pub plugin_name: String,
    /// 插件索引
    pub plugin_idx: String,
    /// 支持的 NRI 版本
    pub nri_version: String,
    /// 是否向 containerd 注册
    pub auto_register: bool,
    /// containerd NRI 套接字地址
    pub runtime_socket_path: String,
    /// 重试配置
    pub retry_config: RetryConfig,
    /// 熔断器配置
    pub circuit_breaker_config: CircuitBreakerConfig,
}

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_retries: u32,
    /// 初始重试延迟
    pub initial_delay: Duration,
    /// 最大重试延迟
    pub max_delay: Duration,
    /// 重试延迟倍数
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

/// 熔断器配置
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// 失败阈值
    pub failure_threshold: u32,
    /// 重置超时时间
    pub reset_timeout: Duration,
    /// 半开状态测试请求数
    pub half_open_max_calls: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            half_open_max_calls: 3,
        }
    }
}

impl ContainerdNriConfig {
    /// 验证配置有效性
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        // 验证 socket 路径
        if self.socket_path.is_empty() {
            return Err(ConfigValidationError::InvalidSocketPath(
                "socket_path cannot be empty".to_string()
            ));
        }
        
        if !self.socket_path.ends_with(".sock") {
            warn!("[ContainerdNri] socket_path does not end with .sock: {}", self.socket_path);
        }
        
        // 验证插件名称
        if self.plugin_name.is_empty() {
            return Err(ConfigValidationError::InvalidPluginName(
                "plugin_name cannot be empty".to_string()
            ));
        }
        
        // 验证插件索引
        if self.plugin_idx.len() != 2 || !self.plugin_idx.chars().all(|c| c.is_ascii_digit()) {
            return Err(ConfigValidationError::InvalidPluginIdx(
                format!("plugin_idx must be 2 digits, got: {}", self.plugin_idx)
            ));
        }
        
        // 验证运行时 socket 路径（如果启用自动注册）
        if self.auto_register && self.runtime_socket_path.is_empty() {
            return Err(ConfigValidationError::InvalidRuntimeSocket(
                "runtime_socket_path cannot be empty when auto_register is enabled".to_string()
            ));
        }
        
        info!("[ContainerdNri] Config validation passed");
        Ok(())
    }
}

impl Default for ContainerdNriConfig {
    fn default() -> Self {
        Self {
            socket_path: "/var/run/nri/nuts-observer.sock".to_string(),
            plugin_name: "nuts-observer".to_string(),
            plugin_idx: "00".to_string(),
            nri_version: "1.0.0".to_string(),
            auto_register: true,
            runtime_socket_path: "/var/run/nri/nri.sock".to_string(),
            retry_config: RetryConfig::default(),
            circuit_breaker_config: CircuitBreakerConfig::default(),
        }
    }
}

/// 配置验证错误
#[derive(Debug, thiserror::Error)]
pub enum ConfigValidationError {
    #[error("Invalid socket path: {0}")]
    InvalidSocketPath(String),
    #[error("Invalid plugin name: {0}")]
    InvalidPluginName(String),
    #[error("Invalid plugin index: {0}")]
    InvalidPluginIdx(String),
    #[error("Invalid runtime socket: {0}")]
    InvalidRuntimeSocket(String),
}

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitBreakerState {
    Closed,      // 正常状态
    Open,        // 熔断状态，拒绝请求
    HalfOpen,    // 半开状态，测试恢复
}

/// 熔断器
pub struct CircuitBreaker {
    state: RwLock<CircuitBreakerState>,
    failure_count: std::sync::atomic::AtomicU32,
    success_count: std::sync::atomic::AtomicU32,
    last_failure_time: RwLock<Option<Instant>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: RwLock::new(CircuitBreakerState::Closed),
            failure_count: std::sync::atomic::AtomicU32::new(0),
            success_count: std::sync::atomic::AtomicU32::new(0),
            last_failure_time: RwLock::new(None),
            config,
        }
    }

    /// 记录成功
    pub async fn record_success(&self) {
        let state = *self.state.read().await;
        match state {
            CircuitBreakerState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if successes >= self.config.half_open_max_calls {
                    let mut state = self.state.write().await;
                    *state = CircuitBreakerState::Closed;
                    self.failure_count.store(0, std::sync::atomic::Ordering::SeqCst);
                    self.success_count.store(0, std::sync::atomic::Ordering::SeqCst);
                    info!("[CircuitBreaker] State changed to Closed");
                }
            }
            CircuitBreakerState::Closed => {
                self.failure_count.store(0, std::sync::atomic::Ordering::SeqCst);
            }
            _ => {}
        }
    }

    /// 记录失败
    pub async fn record_failure(&self) -> CircuitBreakerState {
        let state = *self.state.read().await;
        let mut new_state = state;

        match state {
            CircuitBreakerState::Closed => {
                let failures = self.failure_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold {
                    let mut state = self.state.write().await;
                    *state = CircuitBreakerState::Open;
                    *self.last_failure_time.write().await = Some(Instant::now());
                    new_state = CircuitBreakerState::Open;
                    warn!("[CircuitBreaker] State changed to Open after {} failures", failures);
                }
            }
            CircuitBreakerState::HalfOpen => {
                let mut state = self.state.write().await;
                *state = CircuitBreakerState::Open;
                *self.last_failure_time.write().await = Some(Instant::now());
                new_state = CircuitBreakerState::Open;
                warn!("[CircuitBreaker] State changed back to Open from HalfOpen");
            }
            _ => {}
        }

        new_state
    }

    /// 检查是否可以执行请求
    pub async fn can_execute(&self) -> bool {
        let state = *self.state.read().await;

        match state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                let last_failure = *self.last_failure_time.read().await;
                if let Some(last) = last_failure {
                    if last.elapsed() >= self.config.reset_timeout {
                        let mut state = self.state.write().await;
                        *state = CircuitBreakerState::HalfOpen;
                        self.success_count.store(0, std::sync::atomic::Ordering::SeqCst);
                        info!("[CircuitBreaker] State changed to HalfOpen");
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => true,
        }
    }

    pub async fn current_state(&self) -> CircuitBreakerState {
        *self.state.read().await
    }
}

/// NRI 插件指标
#[derive(Debug, Default)]
pub struct NriMetrics {
    /// 总事件数
    pub events_total: std::sync::atomic::AtomicU64,
    /// 成功处理的事件数
    pub events_success: std::sync::atomic::AtomicU64,
    /// 失败的事件数
    pub events_failed: std::sync::atomic::AtomicU64,
    /// 重试次数
    pub retry_count: std::sync::atomic::AtomicU64,
    /// 熔断器打开次数
    pub circuit_breaker_opened: std::sync::atomic::AtomicU64,
    /// 当前连接状态 (1=connected, 0=disconnected)
    pub connected: std::sync::atomic::AtomicU32,
}

impl NriMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_event(&self, success: bool) {
        self.events_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if success {
            self.events_success.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        } else {
            self.events_failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn record_retry(&self) {
        self.retry_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn record_circuit_breaker_opened(&self) {
        self.circuit_breaker_opened.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_connected(&self, connected: bool) {
        self.connected.store(if connected { 1 } else { 0 }, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Containerd NRI Plugin 服务实现
pub struct ContainerdNriPlugin {
    config: ContainerdNriConfig,
    table: Arc<NriMappingTableV2>,
    event_tx: mpsc::Sender<NriEvent>,
    configured: Arc<RwLock<bool>>,
    circuit_breaker: Arc<CircuitBreaker>,
    metrics: Arc<NriMetrics>,
}

impl ContainerdNriPlugin {
    /// 创建新的 NRI Plugin
    pub fn new(
        config: ContainerdNriConfig,
        table: Arc<NriMappingTableV2>,
        event_tx: mpsc::Sender<NriEvent>,
    ) -> Self {
        let circuit_breaker = Arc::new(CircuitBreaker::new(config.circuit_breaker_config.clone()));
        let metrics = Arc::new(NriMetrics::new());

        Self {
            config,
            table,
            event_tx,
            configured: Arc::new(RwLock::new(false)),
            circuit_breaker,
            metrics,
        }
    }

    /// 获取指标
    pub fn metrics(&self) -> Arc<NriMetrics> {
        Arc::clone(&self.metrics)
    }

    /// 获取熔断器状态
    pub async fn circuit_breaker_state(&self) -> CircuitBreakerState {
        self.circuit_breaker.current_state().await
    }

    /// 启动 gRPC 服务
    pub async fn start(&self) -> Result<(), ContainerdNriError> {
        let path = Path::new(&self.config.socket_path);

        // 清理旧 socket 文件
        if path.exists() {
            tracing::info!("[ContainerdNri] Removing old socket file: {}", self.config.socket_path);
            tokio::fs::remove_file(path).await.map_err(|e| {
                ContainerdNriError::SocketError(format!("Failed to remove old socket: {}", e))
            })?;
        }

        // 确保目录存在
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ContainerdNriError::SocketError(format!("Failed to create directory: {}", e))
            })?;
        }

        // 创建 Unix Socket
        let listener = UnixListener::bind(&self.config.socket_path).map_err(|e| {
            ContainerdNriError::SocketError(format!("Failed to bind socket: {}", e))
        })?;

        // 设置权限（containerd 需要访问）
        let perms = std::fs::Permissions::from_mode(0o666);
        std::fs::set_permissions(&self.config.socket_path, perms).map_err(|e| {
            ContainerdNriError::SocketError(format!("Failed to set permissions: {}", e))
        })?;

        tracing::info!(
            "[ContainerdNri] NRI Plugin listening on {} (plugin_name={}, idx={})",
            self.config.socket_path,
            self.config.plugin_name,
            self.config.plugin_idx
        );

        // 如果需要，向 containerd 注册（带重试机制）
        if self.config.auto_register {
            let plugin_name = self.config.plugin_name.clone();
            let plugin_idx = self.config.plugin_idx.clone();
            let runtime_socket = self.config.runtime_socket_path.clone();
            let socket_path = self.config.socket_path.clone();
            let retry_config = self.config.retry_config.clone();
            let metrics = Arc::clone(&self.metrics);

            tokio::spawn(async move {
                let start_time = Instant::now();
                let mut attempts = 0;

                loop {
                    attempts += 1;
                    debug!(
                        "[ContainerdNri] Registration attempt {} to {}",
                        attempts, runtime_socket
                    );

                    match Self::register_with_runtime(
                        &runtime_socket,
                        &socket_path,
                        &plugin_name,
                        &plugin_idx,
                    ).await {
                        Ok(_) => {
                            metrics.set_connected(true);
                            info!(
                                "[ContainerdNri] Successfully registered with runtime after {} attempts (took {:?})",
                                attempts,
                                start_time.elapsed()
                            );
                            break;
                        }
                        Err(e) => {
                            metrics.record_retry();
                            if attempts >= retry_config.max_retries {
                                error!(
                                    "[ContainerdNri] Failed to register after {} attempts: {}. Will retry in background.",
                                    attempts, e
                                );
                                // 继续重试，但不阻塞服务启动
                                tokio::spawn(async move {
                                    sleep(Duration::from_secs(30)).await;
                                });
                                break;
                            }

                            let delay = std::cmp::min(
                                retry_config.initial_delay.mul_f64(
                                    retry_config.backoff_multiplier.powi(attempts as i32 - 1)
                                ),
                                retry_config.max_delay,
                            );

                            warn!(
                                "[ContainerdNri] Registration attempt {} failed: {}. Retrying in {:?}...",
                                attempts, e, delay
                            );

                            sleep(delay).await;
                        }
                    }
                }
            });
        }

        // 启动 gRPC 服务
        let plugin = ContainerdNriPlugin {
            config: self.config.clone(),
            table: Arc::clone(&self.table),
            event_tx: self.event_tx.clone(),
            configured: Arc::clone(&self.configured),
            circuit_breaker: Arc::clone(&self.circuit_breaker),
            metrics: Arc::clone(&self.metrics),
        };

        let service = PluginServer::new(plugin);
        let stream = UnixListenerStream::new(listener);

        info!("[ContainerdNri] Starting gRPC service with circuit breaker and metrics");

        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming(stream)
            .await
            .map_err(|e| ContainerdNriError::GrpcError(e.to_string()))?;

        Ok(())
    }

    /// 向 containerd 运行时注册插件
    #[instrument(skip(runtime_socket, plugin_socket, plugin_name, plugin_idx))]
    async fn register_with_runtime(
        runtime_socket: &str,
        plugin_socket: &str,
        plugin_name: &str,
        plugin_idx: &str,
    ) -> Result<(), ContainerdNriError> {
        debug!(
            "[ContainerdNri] Connecting to runtime at {} for plugin {}.{}",
            runtime_socket, plugin_name, plugin_idx
        );

        // 检查运行时 socket 是否存在
        if !Path::new(runtime_socket).exists() {
            return Err(ContainerdNriError::ConnectionError(
                format!("Runtime socket does not exist: {}", runtime_socket)
            ));
        }

        // 使用 tonic 连接 Unix Socket
        let channel = tonic::transport::Endpoint::try_from(format!("unix:{}", runtime_socket))
            .map_err(|e| {
                error!("[ContainerdNri] Invalid socket path '{}': {}", runtime_socket, e);
                ContainerdNriError::ConnectionError(format!("Invalid socket path: {}", e))
            })?
            .connect()
            .await
            .map_err(|e| {
                error!("[ContainerdNri] Failed to connect to runtime '{}': {}", runtime_socket, e);
                ContainerdNriError::ConnectionError(format!("Failed to connect: {}", e))
            })?;

        debug!("[ContainerdNri] Connected to runtime, creating client");
        let mut client = RuntimeClient::new(channel);

        let request = tonic::Request::new(RegisterPluginRequest {
            plugin_name: plugin_name.to_string(),
            plugin_idx: plugin_idx.to_string(),
            capabilities: vec![
                nri_proto::EventCapability::PodEvents as i32,
                nri_proto::EventCapability::ContainerEvents as i32,
            ],
        });

        debug!("[ContainerdNri] Sending registration request");
        let response = client.register_plugin(request).await.map_err(|e| {
            error!("[ContainerdNri] Registration RPC failed: {}", e);
            ContainerdNriError::RegistrationError(format!("Registration RPC failed: {}", e))
        })?;

        let resp = response.into_inner();
        if resp.success {
            info!(
                "[ContainerdNri] Successfully registered with runtime: plugin_name={}, idx={}, response={}",
                plugin_name,
                plugin_idx,
                resp.message
            );
            Ok(())
        } else {
            let error_msg = resp.message.clone();
            error!(
                "[ContainerdNri] Runtime rejected registration: plugin_name={}, idx={}, error={}",
                plugin_name,
                plugin_idx,
                error_msg
            );
            Err(ContainerdNriError::RegistrationError(
                format!("Runtime rejected: {}", error_msg)
            ))
        }
    }

    /// 转换 containerd Pod 为内部事件
    fn convert_pod(&self, pod: &nri_proto::PodSandbox) -> NriPodEvent {
        let containers = vec![]; // 会在后续事件中填充

        NriPodEvent {
            pod_uid: pod.pod_uid.clone(),
            pod_name: pod.name.clone(),
            namespace: pod.namespace.clone(),
            containers,
            cgroup_ids: vec![],
            pids: vec![],
        }
    }

    /// 转换 containerd Container 为内部事件
    fn convert_container(&self, container: &nri_proto::Container) -> NriContainerInfo {
        let cgroup_ids = container.linux.as_ref()
            .map(|linux| linux.cgroups.iter().map(|&id| id.to_string()).collect())
            .unwrap_or_default();

        let pids = container.linux.as_ref()
            .map(|linux| linux.pids.iter().map(|&pid| pid as u32).collect())
            .unwrap_or_default();

        NriContainerInfo {
            container_id: container.container_id.clone(),
            cgroup_ids,
            pids,
        }
    }

    /// 从 Pod 和 Container 创建完整事件
    fn create_event(&self, _event_type: &str, pod: &nri_proto::PodSandbox, container: &nri_proto::Container) -> NriEvent {
        let container_info = self.convert_container(container);

        let pod_event = NriPodEvent {
            pod_uid: pod.pod_uid.clone(),
            pod_name: pod.name.clone(),
            namespace: pod.namespace.clone(),
            containers: vec![container_info],
        };

        NriEvent::AddOrUpdate(pod_event)
    }
}

#[tonic::async_trait]
impl Plugin for ContainerdNriPlugin {
    /// Configure 是运行时向插件发送的第一个请求
    async fn configure(
        &self,
        request: Request<ConfigureRequest>,
    ) -> Result<Response<ConfigureResponse>, Status> {
        let req = request.into_inner();

        tracing::info!(
            "[ContainerdNri] Configure received: runtime={}/{}, plugin={}/{}, config_len={}",
            req.runtime_name,
            req.runtime_version,
            req.plugin_name,
            req.plugin_idx,
            req.plugin_config.len()
        );

        // 标记为已配置
        let mut configured = self.configured.write().await;
        *configured = true;

        // 返回支持的配置
        let response = ConfigureResponse {
            success: true,
            error: "".to_string(),
            events: vec![
                nri_proto::EventCapability::PodEvents as i32,
                nri_proto::EventCapability::ContainerEvents as i32,
            ],
        };

        Ok(Response::new(response))
    }

    /// Synchronize 用于同步运行时的当前状态
    async fn synchronize(
        &self,
        request: Request<SynchronizeRequest>,
    ) -> Result<Response<SynchronizeResponse>, Status> {
        let req = request.into_inner();

        tracing::info!(
            "[ContainerdNri] Synchronize received: {} pods, {} containers",
            req.pods.len(),
            req.containers.len()
        );

        // 处理所有现有的 Pod 和 Container
        for pod in &req.pods {
            let pod_event = NriPodEvent {
                pod_uid: pod.pod_uid.clone(),
                pod_name: pod.name.clone(),
                namespace: pod.namespace.clone(),
                containers: vec![],
            };
            let event = NriEvent::AddOrUpdate(pod_event);

            if let Err(e) = self.event_tx.try_send(event) {
                warn!("[ContainerdNri] Failed to send sync pod event: {}", e);
            }
        }

        for container in &req.containers {
            // 查找对应的 Pod
            let pod = req.pods.iter().find(|p| p.pod_uid == container.pod_uid);

            let event = if let Some(pod) = pod {
                let container_info = self.convert_container(container);
                let pod_event = NriPodEvent {
                    pod_uid: pod.pod_uid.clone(),
                    pod_name: pod.name.clone(),
                    namespace: pod.namespace.clone(),
                    containers: vec![container_info],
                };
                NriEvent::AddOrUpdate(pod_event)
            } else {
                let container_info = self.convert_container(container);
                let pod_event = NriPodEvent {
                    pod_uid: container.pod_uid.clone(),
                    pod_name: "".to_string(),
                    namespace: "".to_string(),
                    containers: vec![container_info],
                };
                NriEvent::AddOrUpdate(pod_event)
            };

            if let Err(e) = self.event_tx.try_send(event) {
                warn!("[ContainerdNri] Failed to send sync container event: {}", e);
            }
        }

        // 返回空的更新列表（暂不需要修改容器）
        let response = SynchronizeResponse {
            updates: vec![],
        };

        Ok(Response::new(response))
    }

    /// CreateContainer 在容器创建时调用
    #[instrument(skip(self, request))]
    async fn create_container(
        &self,
        request: Request<CreateContainerRequest>,
    ) -> Result<Response<CreateContainerResponse>, Status> {
        // 检查熔断器状态
        if !self.circuit_breaker.can_execute().await {
            warn!("[ContainerdNri] Circuit breaker is open, rejecting CreateContainer request");
            return Err(Status::unavailable("Service temporarily unavailable"));
        }

        let req = request.into_inner();
        let pod = req.pod.ok_or_else(|| {
            error!("[ContainerdNri] CreateContainer missing pod");
            Status::invalid_argument("Pod is required")
        })?;
        let container = req.container.ok_or_else(|| {
            error!("[ContainerdNri] CreateContainer missing container");
            Status::invalid_argument("Container is required")
        })?;

        info!(
            "[ContainerdNri] CreateContainer: pod_uid={}, pod_name={}, container_id={}",
            pod.pod_uid,
            pod.name,
            container.container_id
        );

        // 发送 ADD 事件
        let event = self.create_event("ADD", &pod, &container);
        match self.event_tx.try_send(event) {
            Ok(_) => {
                self.circuit_breaker.record_success().await;
                self.metrics.record_event(true);
                debug!("[ContainerdNri] CreateContainer event sent successfully");
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to send create event: {}", e);
                self.circuit_breaker.record_failure().await;
                self.metrics.record_event(false);
            }
        }

        // 返回成功，暂不需要更新容器
        let response = CreateContainerResponse {
            success: true,
            error: "".to_string(),
            update: None,
        };

        Ok(Response::new(response))
    }

    /// UpdateContainer 在容器更新时调用
    #[instrument(skip(self, request))]
    async fn update_container(
        &self,
        request: Request<UpdateContainerRequest>,
    ) -> Result<Response<UpdateContainerResponse>, Status> {
        // 检查熔断器状态
        if !self.circuit_breaker.can_execute().await {
            warn!("[ContainerdNri] Circuit breaker is open, rejecting UpdateContainer request");
            return Err(Status::unavailable("Service temporarily unavailable"));
        }

        let req = request.into_inner();
        let pod = req.pod.ok_or_else(|| {
            error!("[ContainerdNri] UpdateContainer missing pod");
            Status::invalid_argument("Pod is required")
        })?;
        let container = req.container.ok_or_else(|| {
            error!("[ContainerdNri] UpdateContainer missing container");
            Status::invalid_argument("Container is required")
        })?;

        info!(
            "[ContainerdNri] UpdateContainer: pod_uid={}, container_id={}, state={}",
            pod.pod_uid,
            container.container_id,
            container.state
        );

        // 发送 UPDATE 事件
        let event = self.create_event("UPDATE", &pod, &container);
        match self.event_tx.try_send(event) {
            Ok(_) => {
                self.circuit_breaker.record_success().await;
                self.metrics.record_event(true);
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to send update event: {}", e);
                self.circuit_breaker.record_failure().await;
                self.metrics.record_event(false);
            }
        }

        let response = UpdateContainerResponse {
            success: true,
            error: "".to_string(),
            update: None,
        };

        Ok(Response::new(response))
    }

    /// StopContainer 在容器停止时调用
    #[instrument(skip(self, request))]
    async fn stop_container(
        &self,
        request: Request<StopContainerRequest>,
    ) -> Result<Response<StopContainerResponse>, Status> {
        // 检查熔断器状态
        if !self.circuit_breaker.can_execute().await {
            warn!("[ContainerdNri] Circuit breaker is open, rejecting StopContainer request");
            return Err(Status::unavailable("Service temporarily unavailable"));
        }

        let req = request.into_inner();
        let pod = req.pod.ok_or_else(|| {
            error!("[ContainerdNri] StopContainer missing pod");
            Status::invalid_argument("Pod is required")
        })?;
        let container = req.container.ok_or_else(|| {
            error!("[ContainerdNri] StopContainer missing container");
            Status::invalid_argument("Container is required")
        })?;

        info!(
            "[ContainerdNri] StopContainer: pod_uid={}, container_id={}",
            pod.pod_uid,
            container.container_id
        );

        // 发送 DELETE 事件
        let event = NriEvent::Delete { pod_uid: pod.pod_uid.clone() };
        match self.event_tx.try_send(event) {
            Ok(_) => {
                self.circuit_breaker.record_success().await;
                self.metrics.record_event(true);
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to send stop event: {}", e);
                self.circuit_breaker.record_failure().await;
                self.metrics.record_event(false);
            }
        }

        let response = StopContainerResponse {
            success: true,
            error: "".to_string(),
            update: None,
        };

        Ok(Response::new(response))
    }
}

/// Containerd NRI 错误类型
#[derive(Debug, thiserror::Error)]
pub enum ContainerdNriError {
    #[error("Socket error: {0}")]
    SocketError(String),
    #[error("gRPC error: {0}")]
    GrpcError(String),
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Registration error: {0}")]
    RegistrationError(String),
    #[error("Not configured")]
    NotConfigured,
}

impl From<ContainerdNriError> for NutsError {
    fn from(e: ContainerdNriError) -> Self {
        NutsError::internal(format!("Containerd NRI error: {}", e))
    }
}
