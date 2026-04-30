//! 采集权限控制模块
//!
//! 实现基于 capability 的权限分离，支持两种模式：
//! 1. **特权模式**（生产环境）：通过 bpfman 或特权代理执行 bpftrace
//! 2. **开发模式**（本地测试）：直接 sudo 运行
//!
//! 目标：最小化 nuts 主进程的权限，隔离特权操作到独立组件

use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::future::Future;
use tokio::sync::RwLock;
use tracing::{info, warn};

// Linux capabilities 常量（CAP_BPF在Linux 5.8+引入，libc可能未定义）
const CAP_SYS_ADMIN: u32 = 21;  // 系统管理权限
const CAP_BPF: u32 = 39;        // BPF权限（Linux 5.8+）

/// 权限控制配置
#[derive(Debug, Clone)]
pub struct PermissionConfig {
    /// 运行模式
    pub mode: PrivilegeMode,
    /// 特权代理路径（如 bpfman 套接字或代理二进制）
    pub privileged_proxy: Option<String>,
    /// 是否检查 capabilities
    pub check_capabilities: bool,
    /// 开发模式：允许直接 sudo
    pub allow_dev_mode: bool,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            mode: PrivilegeMode::AutoDetect,
            privileged_proxy: None,
            check_capabilities: true,
            allow_dev_mode: cfg!(debug_assertions), // 仅在debug模式默认允许
        }
    }
}

/// 权限运行模式
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegeMode {
    /// 自动检测最优模式
    AutoDetect,
    /// 通过 bpfman 执行（推荐生产环境）
    Bpfman,
    /// 通过特权代理进程执行
    PrivilegedProxy,
    /// 直接执行（需要root或sudo）
    Direct,
    /// 开发模式（sudo）
    DevSudo,
}

impl std::str::FromStr for PrivilegeMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" | "autodetect" => Ok(Self::AutoDetect),
            "bpfman" => Ok(Self::Bpfman),
            "proxy" | "privilegedproxy" => Ok(Self::PrivilegedProxy),
            "direct" => Ok(Self::Direct),
            "dev" | "devsudo" | "sudo" => Ok(Self::DevSudo),
            _ => Err(format!("Unknown privilege mode: {}", s)),
        }
    }
}

/// 权限检查结果
#[derive(Debug, Clone)]
pub struct PermissionCheck {
    /// 是否有 CAP_BPF 能力
    pub has_cap_bpf: bool,
    /// 是否有 CAP_SYS_ADMIN 能力
    pub has_cap_sys_admin: bool,
    /// 是否以 root 运行
    pub is_root: bool,
    /// 当前用户ID
    pub uid: u32,
    /// bpfman 是否可用
    pub bpfman_available: bool,
    /// 特权代理是否可用
    pub proxy_available: bool,
    /// 推荐的运行模式
    pub recommended_mode: PrivilegeMode,
    /// 检查结果消息
    pub messages: Vec<String>,
}

/// 权限控制器
pub struct PermissionController {
    config: PermissionConfig,
    state: Arc<RwLock<PermissionState>>,
}

#[derive(Debug, Clone)]
struct PermissionState {
    check_result: Option<PermissionCheck>,
    effective_mode: PrivilegeMode,
    bpfman_socket: Option<String>,
}

impl PermissionController {
    /// 创建新的权限控制器
    pub fn new(config: PermissionConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(PermissionState {
                check_result: None,
                effective_mode: PrivilegeMode::AutoDetect,
                bpfman_socket: None,
            })),
        }
    }

    /// 初始化权限检查
    pub async fn initialize(&self) -> Result<PermissionCheck, PermissionError> {
        let check = self.perform_check().await?;
        
        // 确定有效运行模式
        let effective_mode = if self.config.mode == PrivilegeMode::AutoDetect {
            check.recommended_mode.clone()
        } else {
            self.validate_requested_mode(&self.config.mode, &check)?;
            self.config.mode.clone()
        };

        // 更新状态
        {
            let mut state = self.state.write().await;
            state.check_result = Some(check.clone());
            state.effective_mode = effective_mode.clone();
        }

        info!(
            "Permission controller initialized: mode={:?}, cap_bpf={}, cap_sys_admin={}, root={}",
            effective_mode, check.has_cap_bpf, check.has_cap_sys_admin, check.is_root
        );

        Ok(check)
    }

    /// 执行权限检查
    async fn perform_check(&self) -> Result<PermissionCheck, PermissionError> {
        let mut check = PermissionCheck {
            has_cap_bpf: false,
            has_cap_sys_admin: false,
            is_root: false,
            uid: 0,
            bpfman_available: false,
            proxy_available: false,
            recommended_mode: PrivilegeMode::DevSudo,
            messages: Vec::new(),
        };

        // 检查当前用户
        check.uid = unsafe { libc::getuid() };
        check.is_root = check.uid == 0;

        // 检查 capabilities（Linux 5.8+ 支持 CAP_BPF）
        if self.config.check_capabilities {
            match Self::check_capabilities() {
                Ok(caps) => {
                    check.has_cap_bpf = caps.contains(&CAP_BPF);
                    check.has_cap_sys_admin = caps.contains(&CAP_SYS_ADMIN);
                    check.messages.push(format!(
                        "Capabilities: CAP_BPF={}, CAP_SYS_ADMIN={}",
                        check.has_cap_bpf, check.has_cap_sys_admin
                    ));
                }
                Err(e) => {
                    check.messages.push(format!("Failed to check capabilities: {}", e));
                }
            }
        }

        // 检查 bpfman 可用性
        check.bpfman_available = Self::check_bpfman().await;
        if check.bpfman_available {
            check.messages.push("bpfman detected and available".to_string());
        }

        // 检查特权代理可用性
        if let Some(ref proxy) = self.config.privileged_proxy {
            check.proxy_available = Self::check_proxy(proxy).await;
            if check.proxy_available {
                check.messages.push(format!("Privileged proxy available: {}", proxy));
            }
        }

        // 确定推荐模式
        check.recommended_mode = Self::determine_best_mode(&check, self.config.allow_dev_mode);
        check.messages.push(format!(
            "Recommended privilege mode: {:?}",
            check.recommended_mode
        ));

        Ok(check)
    }

    /// 检查当前进程的 capabilities
    fn check_capabilities() -> Result<Vec<u32>, String> {
        // 读取 /proc/self/status 中的 CapEff 行
        let status = std::fs::read_to_string("/proc/self/status")
            .map_err(|e| format!("Failed to read /proc/self/status: {}", e))?;

        for line in status.lines() {
            if line.starts_with("CapEff:") {
                let caps_hex = line.split(':').nth(1)
                    .map(|s| s.trim())
                    .ok_or("Failed to parse CapEff")?;
                let caps = u64::from_str_radix(caps_hex, 16)
                    .map_err(|e| format!("Failed to parse capabilities: {}", e))?;
                
                // 解析有效的 capability 位
                let mut result = Vec::new();
                for i in 0..64 {
                    if (caps >> i) & 1 == 1 {
                        result.push(i);
                    }
                }
                return Ok(result);
            }
        }

        Err("CapEff not found in /proc/self/status".to_string())
    }

    /// 检查 bpfman 是否可用
    async fn check_bpfman() -> bool {
        // 检查 bpfman 命令是否存在
        match Command::new("bpfman").arg("--version").output() {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 检查特权代理是否可用
    async fn check_proxy(proxy_path: &str) -> bool {
        // 检查代理是否可执行（简化检查）
        std::fs::metadata(proxy_path)
            .map(|m| m.is_file())
            .unwrap_or(false)
    }

    /// 确定最佳运行模式
    fn determine_best_mode(check: &PermissionCheck, allow_dev: bool) -> PrivilegeMode {
        if check.bpfman_available {
            // bpfman 是最安全的选项，优先使用
            return PrivilegeMode::Bpfman;
        }

        if check.proxy_available {
            // 其次是特权代理
            return PrivilegeMode::PrivilegedProxy;
        }

        if check.is_root || check.has_cap_bpf {
            // 有 CAP_BPF 或直接 root，可以直接运行
            return PrivilegeMode::Direct;
        }

        if allow_dev {
            // 开发环境允许 sudo
            return PrivilegeMode::DevSudo;
        }

        // 默认降级到开发模式（但会发出警告）
        PrivilegeMode::DevSudo
    }

    /// 验证请求的模式是否可行
    fn validate_requested_mode(
        &self,
        mode: &PrivilegeMode,
        check: &PermissionCheck,
    ) -> Result<(), PermissionError> {
        match mode {
            PrivilegeMode::Bpfman if !check.bpfman_available => {
                Err(PermissionError::BpfmanNotAvailable)
            }
            PrivilegeMode::PrivilegedProxy if !check.proxy_available => {
                Err(PermissionError::ProxyNotAvailable)
            }
            PrivilegeMode::Direct if !check.is_root && !check.has_cap_bpf => {
                Err(PermissionError::InsufficientPermissions(
                    "Direct mode requires root or CAP_BPF".to_string()
                ))
            }
            PrivilegeMode::DevSudo if !self.config.allow_dev_mode => {
                Err(PermissionError::DevModeNotAllowed)
            }
            _ => Ok(()),
        }
    }

    /// 获取当前有效模式
    pub async fn effective_mode(&self) -> PrivilegeMode {
        let state = self.state.read().await;
        state.effective_mode.clone()
    }

    /// 构建 bpftrace 执行命令
    pub async fn build_bpftrace_command(
        &self,
        script_path: &str,
        args: &[String],
    ) -> Result<BpftraceExecutor, PermissionError> {
        let mode = self.effective_mode().await;
        
        let executor = match mode {
            PrivilegeMode::Bpfman => {
                // 通过 bpfman 执行
                BpftraceExecutor::Bpfman {
                    socket: self.state.read().await.bpfman_socket.clone(),
                    script_path: script_path.to_string(),
                    args: args.to_vec(),
                }
            }
            PrivilegeMode::PrivilegedProxy => {
                // 通过特权代理执行
                let proxy = self.config.privileged_proxy.clone()
                    .ok_or(PermissionError::ProxyNotAvailable)?;
                BpftraceExecutor::Proxy {
                    proxy_path: proxy,
                    script_path: script_path.to_string(),
                    args: args.to_vec(),
                }
            }
            PrivilegeMode::Direct => {
                // 直接执行（已有权限）
                BpftraceExecutor::Direct {
                    script_path: script_path.to_string(),
                    args: args.to_vec(),
                }
            }
            PrivilegeMode::DevSudo => {
                // 开发模式：sudo
                warn!("Using dev mode with sudo - not recommended for production");
                BpftraceExecutor::Sudo {
                    script_path: script_path.to_string(),
                    args: args.to_vec(),
                }
            }
            PrivilegeMode::AutoDetect => {
                return Err(PermissionError::NotInitialized);
            }
        };

        Ok(executor)
    }

    /// 获取权限状态报告
    pub async fn status_report(&self) -> PermissionStatusReport {
        let state = self.state.read().await;
        PermissionStatusReport {
            mode: state.effective_mode.clone(),
            initialized: state.check_result.is_some(),
            check_result: state.check_result.clone(),
        }
    }
}

/// bpftrace 执行器抽象
#[derive(Debug, Clone)]
pub enum BpftraceExecutor {
    /// 通过 bpfman 执行
    Bpfman {
        socket: Option<String>,
        script_path: String,
        args: Vec<String>,
    },
    /// 通过特权代理执行
    Proxy {
        proxy_path: String,
        script_path: String,
        args: Vec<String>,
    },
    /// 直接执行（已有权限）
    Direct {
        script_path: String,
        args: Vec<String>,
    },
    /// sudo 执行（开发模式）
    Sudo {
        script_path: String,
        args: Vec<String>,
    },
}

impl BpftraceExecutor {
    /// 转换为 Command
    pub fn to_command(&self) -> Command {
        match self {
            Self::Bpfman { socket, script_path, args } => {
                let mut cmd = Command::new("bpfman");
                cmd.arg("run").arg(script_path);
                if let Some(sock) = socket {
                    cmd.arg("--socket").arg(sock);
                }
                cmd.args(args);
                cmd
            }
            Self::Proxy { proxy_path, script_path, args } => {
                let mut cmd = Command::new(proxy_path);
                cmd.arg("run").arg(script_path).args(args);
                cmd
            }
            Self::Direct { script_path, args } => {
                let mut cmd = Command::new("bpftrace");
                cmd.arg(script_path).args(args);
                cmd
            }
            Self::Sudo { script_path, args } => {
                let mut cmd = Command::new("sudo");
                cmd.arg("bpftrace").arg(script_path).args(args);
                cmd
            }
        }
    }
}

/// 权限错误类型
#[derive(Debug, Clone)]
pub enum PermissionError {
    BpfmanNotAvailable,
    ProxyNotAvailable,
    InsufficientPermissions(String),
    DevModeNotAllowed,
    NotInitialized,
    CheckFailed(String),
}

impl std::fmt::Display for PermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BpfmanNotAvailable => write!(f, "bpfman not available"),
            Self::ProxyNotAvailable => write!(f, "privileged proxy not available"),
            Self::InsufficientPermissions(msg) => write!(f, "insufficient permissions: {}", msg),
            Self::DevModeNotAllowed => write!(f, "development mode (sudo) is not allowed in production"),
            Self::NotInitialized => write!(f, "permission controller not initialized"),
            Self::CheckFailed(msg) => write!(f, "permission check failed: {}", msg),
        }
    }
}

impl std::error::Error for PermissionError {}

/// 权限状态报告
#[derive(Debug, Clone)]
pub struct PermissionStatusReport {
    pub mode: PrivilegeMode,
    pub initialized: bool,
    pub check_result: Option<PermissionCheck>,
}

/// 全局权限控制器（可选单例模式）
static PERMISSION_CONTROLLER: OnceLock<PermissionController> = OnceLock::new();

/// 初始化全局权限控制器（必须在runtime内调用）
pub fn init_global_controller_blocking(config: PermissionConfig) -> Result<(), PermissionError> {
    let controller = PermissionController::new(config);
    
    PERMISSION_CONTROLLER
        .set(controller)
        .map_err(|_| PermissionError::CheckFailed("Global controller already initialized".to_string()))
}

/// 异步初始化全局权限控制器
pub async fn init_global_controller<F, Fut>(config: PermissionConfig, runtime_spawn: F) -> Result<(), PermissionError> 
where
    F: FnOnce(PermissionController) -> Fut,
    Fut: Future<Output = ()>,
{
    let controller = PermissionController::new(config);
    
    // 先存储控制器
    PERMISSION_CONTROLLER
        .set(controller)
        .map_err(|_| PermissionError::CheckFailed("Global controller already initialized".to_string()))?;
    
    // 然后异步初始化
    if let Some(ctrl) = PERMISSION_CONTROLLER.get() {
        runtime_spawn(ctrl.clone()).await;
    }
    
    Ok(())
}

/// 获取全局权限控制器
pub fn global_controller() -> Option<&'static PermissionController> {
    PERMISSION_CONTROLLER.get()
}

impl Clone for PermissionController {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_privilege_mode_from_str() {
        assert_eq!(
            PrivilegeMode::from_str("bpfman").unwrap(),
            PrivilegeMode::Bpfman
        );
        assert_eq!(
            PrivilegeMode::from_str("DEV").unwrap(),
            PrivilegeMode::DevSudo
        );
        assert!(PrivilegeMode::from_str("unknown").is_err());
    }

    #[test]
    fn test_bpftrace_executor_command() {
        let executor = BpftraceExecutor::Sudo {
            script_path: "/path/script.bt".to_string(),
            args: vec!["arg1".to_string()],
        };
        let cmd = executor.to_command();
        // 无法直接验证Command内部，但至少确保构建成功
        drop(cmd);
    }

    #[test]
    fn test_default_config() {
        let config = PermissionConfig::default();
        assert_eq!(config.mode, PrivilegeMode::AutoDetect);
        assert!(config.check_capabilities);
        // dev模式仅在debug断言开启时默认允许
        assert_eq!(config.allow_dev_mode, cfg!(debug_assertions));
    }
}
