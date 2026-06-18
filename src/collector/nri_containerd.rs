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

use crate::types::error::PodflowError;
use crate::metrics::UnifiedMetrics;
use std::path::Path;
use std::sync::Arc;
use std::os::unix::fs::PermissionsExt;
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
    RegisterPluginRequest,
    UnregisterPluginRequest,
    plugin_server::{Plugin, PluginServer},
    runtime_client::RuntimeClient,
};

use super::nri_mapping_v2::{NriContainerInfo, NriEvent, NriPodEvent};
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
            socket_path: "/var/run/nri/podflow.sock".to_string(),
            plugin_name: "podflow".to_string(),
            plugin_idx: "00".to_string(),
            nri_version: "1.0.0".to_string(),
            auto_register: true,
            runtime_socket_path: "/var/run/nri/nri.sock".to_string(),
            retry_config: RetryConfig::default(),
            circuit_breaker_config: CircuitBreakerConfig::default(),
        }
    }
}

impl ContainerdNriConfig {
    /// 从环境变量创建配置
    /// 支持的环境变量：
    /// - NUTS_NRI_SOCKET_PATH: Unix Socket 路径
    /// - NUTS_NRI_PLUGIN_NAME: 插件名称
    /// - NUTS_NRI_PLUGIN_IDX: 插件索引 (00-99)
    /// - NUTS_NRI_VERSION: NRI 协议版本
    /// - NUTS_NRI_AUTO_REGISTER: 是否自动注册 (true/false)
    /// - NUTS_NRI_RUNTIME_SOCKET: containerd NRI 套接字路径
    pub fn from_env() -> Result<Self, ConfigValidationError> {
        use std::env;
        
        let mut config = Self::default();
        
        // 从环境变量读取基本配置
        if let Ok(val) = env::var("NUTS_NRI_SOCKET_PATH") {
            if !val.is_empty() {
                config.socket_path = val;
            }
        }
        
        if let Ok(val) = env::var("NUTS_NRI_PLUGIN_NAME") {
            if !val.is_empty() {
                config.plugin_name = val;
            }
        }
        
        if let Ok(val) = env::var("NUTS_NRI_PLUGIN_IDX") {
            if !val.is_empty() {
                config.plugin_idx = val;
            }
        }
        
        if let Ok(val) = env::var("NUTS_NRI_VERSION") {
            if !val.is_empty() {
                config.nri_version = val;
            }
        }
        
        if let Ok(val) = env::var("NUTS_NRI_AUTO_REGISTER") {
            config.auto_register = val.parse().unwrap_or(true);
        }
        
        if let Ok(val) = env::var("NUTS_NRI_RUNTIME_SOCKET") {
            if !val.is_empty() {
                config.runtime_socket_path = val;
            }
        }
        
        info!("[ContainerdNri] Configuration loaded from environment variables");
        
        // 验证配置
        config.validate()?;
        
        Ok(config)
    }
    
    /// 创建配置（优先从环境变量读取，失败则使用默认值）
    pub fn from_env_or_default() -> Self {
        match Self::from_env() {
            Ok(config) => {
                info!("[ContainerdNri] Using configuration from environment");
                config
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to load from environment ({}), using defaults", e);
                let config = Self::default();
                if let Err(e) = config.validate() {
                    error!("[ContainerdNri] Default config validation failed: {}", e);
                }
                config
            }
        }
    }

    /// 从主配置文件的 NRI 段创建，环境变量可覆盖
    pub fn from_nri_config(nri: &crate::config::NriConfig) -> Self {
        let mut config = Self {
            socket_path: nri.socket_path.clone(),
            plugin_name: nri.plugin_name.clone(),
            plugin_idx: nri.plugin_idx.clone(),
            nri_version: nri.nri_version.clone(),
            auto_register: nri.auto_register,
            runtime_socket_path: nri.runtime_socket_path.clone(),
            retry_config: RetryConfig {
                max_retries: nri.retry.max_retries,
                initial_delay: std::time::Duration::from_millis(nri.retry.initial_delay_ms),
                max_delay: std::time::Duration::from_millis(nri.retry.max_delay_ms),
                backoff_multiplier: nri.retry.backoff_multiplier,
            },
            circuit_breaker_config: CircuitBreakerConfig {
                failure_threshold: nri.circuit_breaker.failure_threshold,
                reset_timeout: std::time::Duration::from_secs(nri.circuit_breaker.reset_timeout_secs),
                half_open_max_calls: nri.circuit_breaker.half_open_max_calls,
            },
        };

        // 环境变量覆盖
        use std::env;
        if let Ok(val) = env::var("NUTS_NRI_SOCKET_PATH") {
            if !val.is_empty() { config.socket_path = val; }
        }
        if let Ok(val) = env::var("NUTS_NRI_PLUGIN_NAME") {
            if !val.is_empty() { config.plugin_name = val; }
        }
        if let Ok(val) = env::var("NUTS_NRI_RUNTIME_SOCKET") {
            if !val.is_empty() { config.runtime_socket_path = val; }
        }

        if let Err(e) = config.validate() {
            warn!("[ContainerdNri] Config from nri section validation failed: {}", e);
        }

        info!("[ContainerdNri] Configuration loaded from config.yaml nri section (with env overrides)");
        config
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

/// Containerd NRI Plugin 服务实现
pub struct ContainerdNriPlugin {
    config: ContainerdNriConfig,
    table: Arc<NriMappingTableV2>,
    event_tx: mpsc::Sender<NriEvent>,
    configured: Arc<RwLock<bool>>,
    circuit_breaker: Arc<CircuitBreaker>,
    metrics: Arc<UnifiedMetrics>,
}

impl ContainerdNriPlugin {
    /// 创建新的 NRI Plugin
    pub fn new(
        config: ContainerdNriConfig,
        table: Arc<NriMappingTableV2>,
        event_tx: mpsc::Sender<NriEvent>,
    ) -> Self {
        let circuit_breaker = Arc::new(CircuitBreaker::new(config.circuit_breaker_config.clone()));
        let metrics = crate::metrics::create_metrics();

        Self {
            config,
            table,
            event_tx,
            configured: Arc::new(RwLock::new(false)),
            circuit_breaker,
            metrics: metrics.into(),
        }
    }

    /// 获取指标
    pub fn metrics(&self) -> Arc<UnifiedMetrics> {
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
                // 等待插件自身的 socket 就绪（gRPC server 需要先启动）
                let socket_path_obj = Path::new(&socket_path);
                for wait in 0..50 {
                    if socket_path_obj.exists() {
                        debug!("[ContainerdNri] Plugin socket {} is ready", socket_path);
                        break;
                    }
                    if wait == 49 {
                        warn!(
                            "[ContainerdNri] Plugin socket {} not ready after 5s, proceeding anyway",
                            socket_path
                        );
                    }
                    sleep(Duration::from_millis(100)).await;
                }

                let start_time = Instant::now();
                let mut attempts = 0;
                let mut in_background_retry = false;

                loop {
                    attempts += 1;
                    debug!(
                        "[ContainerdNri] Registration attempt {} to {}{}",
                        attempts,
                        runtime_socket,
                        if in_background_retry { " (background)" } else { "" }
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

                            // 计算延迟：初始阶段用指数退避，后台阶段用固定 30s
                            let delay = if !in_background_retry && attempts < retry_config.max_retries {
                                std::cmp::min(
                                    retry_config.initial_delay.mul_f64(
                                        retry_config.backoff_multiplier.powi(attempts as i32 - 1)
                                    ),
                                    retry_config.max_delay,
                                )
                            } else if !in_background_retry {
                                // 达到最大重试次数，切换到后台持续重试
                                error!(
                                    "[ContainerdNri] Failed to register after {} attempts: {}. \
                                     Switching to background retry mode (every 30s).",
                                    attempts, e
                                );
                                in_background_retry = true;
                                Duration::from_secs(30)
                            } else {
                                warn!(
                                    "[ContainerdNri] Background registration attempt failed: {}. \
                                     Retrying in 30s...",
                                    e
                                );
                                Duration::from_secs(30)
                            };

                            if !in_background_retry {
                                warn!(
                                    "[ContainerdNri] Registration attempt {} failed: {}. Retrying in {:?}...",
                                    attempts, e, delay
                                );
                            }

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
    ///
    /// 使用 tonic 通过 Unix Socket 连接 containerd NRI Runtime，
    /// 调用 RegisterPlugin RPC 将自己注册为 NRI 插件。
    #[instrument(skip(runtime_socket, plugin_socket, plugin_name, plugin_idx))]
    async fn register_with_runtime(
        runtime_socket: &str,
        plugin_socket: &str,
        plugin_name: &str,
        plugin_idx: &str,
    ) -> Result<(), ContainerdNriError> {
        debug!(
            "[ContainerdNri] Connecting to runtime at {} for plugin {}.{} (socket={})",
            runtime_socket, plugin_name, plugin_idx, plugin_socket
        );

        // 检查运行时 socket 是否存在
        if !Path::new(runtime_socket).exists() {
            return Err(ContainerdNriError::ConnectionError(
                format!("Runtime socket does not exist: {}", runtime_socket)
            ));
        }

        // 检查插件 socket 是否存在（containerd 需要连回来）
        if !Path::new(plugin_socket).exists() {
            warn!(
                "[ContainerdNri] Plugin socket {} does not exist yet. \
                 The plugin gRPC server should be started before registration.",
                plugin_socket
            );
        }

        // 构造 Unix Socket URI (tonic 格式: unix://path)
        let runtime_uri = format!("unix://{}", runtime_socket);

        // 连接 containerd NRI Runtime
        let channel = tonic::transport::Endpoint::from_shared(runtime_uri.clone())
            .map_err(|e| ContainerdNriError::ConnectionError(
                format!("Failed to create endpoint for {}: {}", runtime_uri, e)
            ))?
            .connect()
            .await
            .map_err(|e| ContainerdNriError::ConnectionError(
                format!("Failed to connect to runtime at {}: {}", runtime_uri, e)
            ))?;

        let mut runtime_client = RuntimeClient::new(channel);

        // 构造注册请求
        let request = RegisterPluginRequest {
            plugin_name: plugin_name.to_string(),
            plugin_idx: plugin_idx.to_string(),
            capabilities: vec![
                nri_proto::EventCapability::RuntimeEvents as i32,
                nri_proto::EventCapability::PodEvents as i32,
                nri_proto::EventCapability::ContainerEvents as i32,
            ],
        };

        debug!(
            "[ContainerdNri] Sending RegisterPlugin request: name={}, idx={}, capabilities={:?}",
            request.plugin_name, request.plugin_idx, request.capabilities
        );

        // 发送注册请求
        let response = runtime_client
            .register_plugin(request)
            .await
            .map_err(|e| ContainerdNriError::RegistrationError(
                format!("RegisterPlugin RPC failed: {}", e)
            ))?;

        let resp = response.into_inner();

        if resp.success {
            info!(
                "[ContainerdNri] Successfully registered plugin {}.{} with runtime",
                plugin_name, plugin_idx
            );
            Ok(())
        } else {
            Err(ContainerdNriError::RegistrationError(
                format!(
                    "Runtime rejected plugin registration: {}",
                    if resp.error_message.is_empty() { "unknown error".to_string() } else { resp.error_message }
                )
            ))
        }
    }

    /// 从 containerd 运行时注销插件
    ///
    /// 使用 tonic 通过 Unix Socket 连接 containerd NRI Runtime，
    /// 调用 UnregisterPlugin RPC 将自己从 NRI 插件列表中移除。
    #[instrument(skip(runtime_socket, plugin_name, plugin_idx))]
    pub async fn unregister_from_runtime(
        runtime_socket: &str,
        plugin_name: &str,
        plugin_idx: &str,
    ) -> Result<(), ContainerdNriError> {
        debug!(
            "[ContainerdNri] Connecting to runtime at {} to unregister plugin {}.{}",
            runtime_socket, plugin_name, plugin_idx
        );

        // 检查运行时 socket 是否存在
        if !Path::new(runtime_socket).exists() {
            return Err(ContainerdNriError::ConnectionError(
                format!("Runtime socket does not exist: {}", runtime_socket)
            ));
        }

        // 构造 Unix Socket URI (tonic 格式: unix://path)
        let runtime_uri = format!("unix://{}", runtime_socket);

        // 连接 containerd NRI Runtime
        let channel = tonic::transport::Endpoint::from_shared(runtime_uri.clone())
            .map_err(|e| ContainerdNriError::ConnectionError(
                format!("Failed to create endpoint for {}: {}", runtime_uri, e)
            ))?
            .connect()
            .await
            .map_err(|e| ContainerdNriError::ConnectionError(
                format!("Failed to connect to runtime at {}: {}", runtime_uri, e)
            ))?;

        let mut runtime_client = RuntimeClient::new(channel);

        // 构造注销请求
        let request = UnregisterPluginRequest {
            plugin_name: plugin_name.to_string(),
            plugin_idx: plugin_idx.to_string(),
        };

        debug!(
            "[ContainerdNri] Sending UnregisterPlugin request: name={}, idx={}",
            request.plugin_name, request.plugin_idx
        );

        // 发送注销请求
        let response = runtime_client
            .unregister_plugin(request)
            .await
            .map_err(|e| ContainerdNriError::RegistrationError(
                format!("UnregisterPlugin RPC failed: {}", e)
            ))?;

        let resp = response.into_inner();

        if resp.success {
            info!(
                "[ContainerdNri] Successfully unregistered plugin {}.{} from runtime",
                plugin_name, plugin_idx
            );
            Ok(())
        } else {
            Err(ContainerdNriError::RegistrationError(
                format!(
                    "Runtime rejected plugin unregistration: {}",
                    if resp.error_message.is_empty() { "unknown error".to_string() } else { resp.error_message }
                )
            ))
        }
    }

    /// 转换 containerd Pod 为内部事件
    #[allow(dead_code)]
fn convert_pod(&self, pod: &nri_proto::PodSandbox) -> NriPodEvent {
        let containers = vec![]; // 会在后续事件中填充

        // 提取额外的CRI兼容信息
        let runtime_handler = pod.runtime_handler.clone();
        let ips = pod.ips.clone();
        
        // 提取namespace信息
        let namespaces = pod.linux.as_ref()
            .map(|linux| {
                linux.namespaces.iter()
                    .map(|ns| format!("{}:{}", ns.r#type, ns.path))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        debug!(
            "[ContainerdNri] Converting pod: runtime_handler={}, ips={:?}, namespaces={:?}",
            runtime_handler, ips, namespaces
        );

        NriPodEvent {
            pod_uid: pod.pod_uid.clone(),
            pod_name: pod.name.clone(),
            namespace: pod.namespace.clone(),
            containers,
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

        // 提取namespace信息
        let namespaces = container.linux.as_ref()
            .map(|linux| {
                linux.namespaces.iter()
                    .map(|ns| format!("{}:{}", ns.r#type, ns.path))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // 提取cgroup路径
        let cgroup_path = container.linux.as_ref()
            .map(|linux| linux.cgroup_path.clone())
            .unwrap_or_default();

        debug!(
            "[ContainerdNri] Converting container: id={}, cgroup_path={}, namespaces={:?}",
            container.container_id, cgroup_path, namespaces
        );

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

        // 返回支持的配置，包括Pod生命周期事件
        let response = ConfigureResponse {
            success: true,
            error: "".to_string(),
            events: vec![
                nri_proto::EventCapability::RuntimeEvents as i32,
                nri_proto::EventCapability::PodEvents as i32,
                nri_proto::EventCapability::ContainerEvents as i32,
            ],
        };

        debug!(
            "[ContainerdNri] Plugin configured with event capabilities: {:?}",
            response.events
        );

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

        // 按 pod_uid 分组容器
        let mut pod_containers: std::collections::HashMap<String, Vec<crate::collector::nri_mapping_v2::NriContainerInfo>> =
            std::collections::HashMap::new();
        for container in &req.containers {
            let container_info = self.convert_container(container);
            pod_containers
                .entry(container.pod_uid.clone())
                .or_default()
                .push(container_info);
        }

        // 每个 Pod 发送一次包含所有容器的事件
        for pod in &req.pods {
            let containers = pod_containers.remove(&pod.pod_uid).unwrap_or_default();
            let pod_event = NriPodEvent {
                pod_uid: pod.pod_uid.clone(),
                pod_name: pod.name.clone(),
                namespace: pod.namespace.clone(),
                containers,
            };
            if let Err(e) = self.event_tx.try_send(NriEvent::AddOrUpdate(pod_event)) {
                warn!("[ContainerdNri] Failed to send sync pod event: {}", e);
            }
        }

        // 处理没有对应 Pod 的容器（孤儿容器）
        for (pod_uid, containers) in pod_containers {
            let pod_event = NriPodEvent {
                pod_uid: pod_uid.clone(),
                pod_name: "".to_string(),
                namespace: "".to_string(),
                containers,
            };
            if let Err(e) = self.event_tx.try_send(NriEvent::AddOrUpdate(pod_event)) {
                warn!("[ContainerdNri] Failed to send sync orphan container event: {}", e);
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
                self.metrics.record_containerd_event(true);
                debug!("[ContainerdNri] CreateContainer event sent successfully");
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to send create event: {}", e);
                self.circuit_breaker.record_failure().await;
                self.metrics.record_containerd_event(false);
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
                self.metrics.record_containerd_event(true);
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to send update event: {}", e);
                self.circuit_breaker.record_failure().await;
                self.metrics.record_containerd_event(false);
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

        // 发送 RemoveContainer 事件（仅移除该容器，保留 Pod 的其他容器）
        let event = NriEvent::RemoveContainer {
            pod_uid: pod.pod_uid.clone(),
            container_id: container.container_id.clone(),
        };
        match self.event_tx.try_send(event) {
            Ok(_) => {
                self.circuit_breaker.record_success().await;
                self.metrics.record_containerd_event(true);
            }
            Err(e) => {
                warn!("[ContainerdNri] Failed to send stop event: {}", e);
                self.circuit_breaker.record_failure().await;
                self.metrics.record_containerd_event(false);
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

impl From<ContainerdNriError> for PodflowError {
    fn from(e: ContainerdNriError) -> Self {
        PodflowError::internal(&format!("Containerd NRI error: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn test_register_with_runtime_socket_not_exists() {
        let runtime_socket = "/nonexistent/runtime.sock";
        let plugin_socket = "/tmp/test-plugin.sock";
        let plugin_name = "test-plugin";
        let plugin_idx = "001";

        let result = ContainerdNriPlugin::register_with_runtime(
            runtime_socket,
            plugin_socket,
            plugin_name,
            plugin_idx,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ContainerdNriError::ConnectionError(msg) => {
                assert!(msg.contains("Runtime socket does not exist"));
            }
            _ => panic!("Expected ConnectionError for nonexistent socket"),
        }
    }

    #[tokio::test]
    async fn test_register_with_runtime_plugin_socket_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let runtime_socket = temp_dir.path().join("runtime.sock");
        
        // 创建运行时socket文件
        UnixListener::bind(&runtime_socket).unwrap();
        
        let plugin_socket = "/nonexistent/plugin.sock";
        let plugin_name = "test-plugin";
        let plugin_idx = "001";

        let result = ContainerdNriPlugin::register_with_runtime(
            runtime_socket.to_str().unwrap(),
            plugin_socket,
            plugin_name,
            plugin_idx,
        ).await;

        // 应该成功，因为插件socket不存在只是警告
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_register_with_runtime_invalid_runtime_socket() {
        let runtime_socket = "/invalid/runtime.sock";
        let plugin_socket = "/tmp/test-plugin.sock";
        let plugin_name = "test-plugin";
        let plugin_idx = "001";

        let result = ContainerdNriPlugin::register_with_runtime(
            runtime_socket,
            plugin_socket,
            plugin_name,
            plugin_idx,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ContainerdNriError::ConnectionError(_) | ContainerdNriError::GrpcError(_) => {
                // 预期连接错误或gRPC错误
            }
            _ => panic!("Expected ConnectionError or GrpcError for invalid socket"),
        }
    }

    #[tokio::test]
    async fn test_register_with_runtime_success_path() {
        let temp_dir = TempDir::new().unwrap();
        let runtime_socket = temp_dir.path().join("runtime.sock");
        let plugin_socket = temp_dir.path().join("plugin.sock");
        let plugin_name = "test-plugin";
        let plugin_idx = "001";

        // 模拟运行时socket存在
        UnixListener::bind(&runtime_socket).unwrap();

        let result = ContainerdNriPlugin::register_with_runtime(
            runtime_socket.to_str().unwrap(),
            plugin_socket.to_str().unwrap(),
            plugin_name,
            plugin_idx,
        ).await;

        // 由于没有真实的containerd运行时，预期连接失败，但应该能到达连接阶段
        assert!(result.is_err());
        match result.unwrap_err() {
            ContainerdNriError::ConnectionError(_) | ContainerdNriError::GrpcError(_) => {
                // 预期连接错误，这是正常的
            }
            _ => panic!("Expected ConnectionError or GrpcError"),
        }
    }
}
