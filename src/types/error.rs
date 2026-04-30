//! 统一的错误类型定义

use std::io;
use serde_json;
use serde_yaml;
use thiserror::Error;

/// 统一的错误类型
#[derive(Error, Debug)]
pub enum NutsError {
    /// IO错误
    #[error("IO错误: {0}")]
    Io(#[from] io::Error),
    
    /// JSON序列化/反序列化错误
    #[error("JSON错误: {0}")]
    Json(#[from] serde_json::Error),
    
    /// YAML序列化/反序列化错误
    #[error("YAML错误: {0}")]
    Yaml(#[from] serde_yaml::Error),
    
    /// 锁获取失败
    #[error("锁获取失败: {0}")]
    LockError(String),
    
    /// 网络错误
    #[error("网络错误: {0}")]
    Network(String),
    
    /// 配置错误
    #[error("配置错误: {0}")]
    Config(String),
    
    /// 验证错误
    #[error("验证错误: {0}")]
    Validation(String),
    
    /// 未找到资源
    #[error("未找到: {0}")]
    NotFound(String),
    
    /// 内部错误
    #[error("内部错误: {0}")]
    Internal(String),
    
    /// 自定义错误
    #[error("{0}")]
    Custom(String),
}

impl NutsError {
    /// 创建锁错误
    pub fn lock_error(msg: &str) -> Self {
        NutsError::LockError(msg.to_string())
    }
    
    /// 创建网络错误
    pub fn network(msg: &str) -> Self {
        NutsError::Network(msg.to_string())
    }
    
    /// 创建配置错误
    pub fn config(msg: &str) -> Self {
        NutsError::Config(msg.to_string())
    }
    
    /// 创建验证错误
    pub fn validation(msg: &str) -> Self {
        NutsError::Validation(msg.to_string())
    }
    
    /// 创建未找到错误
    pub fn not_found(msg: &str) -> Self {
        NutsError::NotFound(msg.to_string())
    }
    
    /// 创建内部错误
    pub fn internal(msg: &str) -> Self {
        NutsError::Internal(msg.to_string())
    }
    
    /// 创建自定义错误
    pub fn custom(msg: &str) -> Self {
        NutsError::Custom(msg.to_string())
    }
}

/// Result类型别名
pub type Result<T> = std::result::Result<T, NutsError>;