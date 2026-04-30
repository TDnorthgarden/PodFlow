/// 错误处理辅助函数
use crate::types::error::{NutsError, Result};

/// 安全地获取 Mutex 锁
pub fn lock_mutex<T>(guard: std::sync::MutexGuard<'_, T>) -> Result<std::sync::MutexGuard<'_, T>> {
    Ok(guard)
}

/// 安全地获取 Mutex 锁，带错误信息
pub fn lock_mutex_with_error<'a, T>(
    result: std::sync::LockResult<std::sync::MutexGuard<'a, T>>,
    error_msg: &'a str,
) -> Result<std::sync::MutexGuard<'a, T>> {
    result.map_err(|_| NutsError::lock_error(error_msg))
}

/// 安全地获取 RwLock 读锁
pub fn read_rwlock<'a, T>(
    result: std::sync::LockResult<std::sync::RwLockReadGuard<'a, T>>,
    error_msg: &'a str,
) -> Result<std::sync::RwLockReadGuard<'a, T>> {
    result.map_err(|_| NutsError::lock_error(error_msg))
}

/// 安全地获取 RwLock 写锁
pub fn write_rwlock<'a, T>(
    result: std::sync::LockResult<std::sync::RwLockWriteGuard<'a, T>>,
    error_msg: &'a str,
) -> Result<std::sync::RwLockWriteGuard<'a, T>> {
    result.map_err(|_| NutsError::lock_error(error_msg))
}

/// 安全地序列化为 JSON
pub fn to_json_pretty<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(NutsError::Json)
}

/// 安全地从 JSON 反序列化
pub fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> Result<T> {
    serde_json::from_str(json).map_err(NutsError::Json)
}

/// 安全地序列化为 YAML
pub fn to_yaml<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_yaml::to_string(value).map_err(NutsError::Yaml)
}

/// 安全地从 YAML 反序列化
pub fn from_yaml<T: serde::de::DeserializeOwned>(yaml: &str) -> Result<T> {
    serde_yaml::from_str(yaml).map_err(NutsError::Yaml)
}

/// 安全地写入文件
pub fn write_file<P: AsRef<std::path::Path>, C: AsRef<[u8]>>(path: P, contents: C) -> Result<()> {
    std::fs::write(path, contents).map_err(NutsError::Io)
}

/// 安全地读取文件
pub fn read_file<P: AsRef<std::path::Path>>(path: P) -> Result<String> {
    std::fs::read_to_string(path).map_err(NutsError::Io)
}