//! 认证和授权相关类型
//!
//! 提供 UID 验证和认证流的核心类型定义

use std::io;
use std::os::unix::io::AsRawFd;
use tokio::net::UnixStream;

/// Peer UID 包装类型（用于请求扩展）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerUid(pub u32);

/// 认证流包装类型
///
/// 包装 UnixStream 并验证对等端的 UID 是否在允许列表中
#[derive(Debug)]
pub struct AuthenticatedStream {
    stream: UnixStream,
    peer_uid: u32,
    allowed_uids: Vec<u32>,
}

impl AuthenticatedStream {
    /// 创建一个新的认证流
    ///
    /// # Arguments
    /// * `stream` - Unix socket 流
    /// * `allowed_uids` - 允许的 UID 列表
    ///
    /// # Returns
    /// 如果 UID 验证成功，返回 AuthenticatedStream；否则返回错误
    pub fn new(stream: UnixStream, allowed_uids: Vec<u32>) -> io::Result<Self> {
        let peer_uid = get_peer_uid(&stream)?;

        if !check_uid_permission(peer_uid, &allowed_uids) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("UID {} not in allowed list", peer_uid),
            ));
        }

        Ok(AuthenticatedStream {
            stream,
            peer_uid,
            allowed_uids,
        })
    }

    /// 获取对等端的 UID
    pub fn peer_uid(&self) -> u32 {
        self.peer_uid
    }

    /// 获取允许的 UID 列表
    pub fn allowed_uids(&self) -> &[u32] {
        &self.allowed_uids
    }

    /// 获取底层的 UnixStream
    pub fn stream(&self) -> &UnixStream {
        &self.stream
    }

    /// 获取可变的底层 UnixStream
    pub fn stream_mut(&mut self) -> &mut UnixStream {
        &mut self.stream
    }

    /// 消费 AuthenticatedStream 并返回底层的 UnixStream
    pub fn into_inner(self) -> UnixStream {
        self.stream
    }
}

/// 获取对等端的 UID
///
/// 这个函数从 UnixStream 中提取对等端的 UID，
/// 用于验证连接的权限
pub fn get_peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let fd = stream.as_raw_fd();
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };

    let mut credentials_size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    unsafe {
        if libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut credentials as *mut _ as *mut libc::c_void,
            &mut credentials_size,
        ) == -1 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(credentials.uid)
}

/// 检查 UID 是否在允许列表中
pub fn check_uid_permission(uid: u32, allowed_uids: &[u32]) -> bool {
    !allowed_uids.is_empty() && allowed_uids.contains(&uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_uid_equality() {
        let uid1 = PeerUid(1000);
        let uid2 = PeerUid(1000);
        let uid3 = PeerUid(2000);

        assert_eq!(uid1, uid2);
        assert_ne!(uid1, uid3);
    }

    #[test]
    fn test_check_uid_permission() {
        let allowed_uids = vec![1000, 2000];

        assert!(check_uid_permission(1000, &allowed_uids));
        assert!(check_uid_permission(2000, &allowed_uids));
        assert!(!check_uid_permission(3000, &allowed_uids));

        // 空列表应该拒绝所有 UID
        assert!(!check_uid_permission(1000, &[]));
        assert!(!check_uid_permission(3000, &[]));
    }

    #[tokio::test]
    async fn test_authenticated_stream_creation() {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair().unwrap();
        let tokio_stream = tokio::net::UnixStream::from_std(sock1).unwrap();

        // 获取当前进程的 UID
        let current_uid = unsafe { libc::getuid() };

        // 创建认证流（应该成功）
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![current_uid]);
        assert!(auth_stream.is_ok(), "应该能够创建认证流");

        let auth_stream = auth_stream.unwrap();
        assert_eq!(auth_stream.peer_uid(), current_uid, "peer_uid 应该匹配");
        assert_eq!(auth_stream.allowed_uids(), &[current_uid], "allowed_uids 应该正确");
    }

    #[tokio::test]
    async fn test_authenticated_stream_permission_denied() {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair().unwrap();
        let tokio_stream = tokio::net::UnixStream::from_std(sock1).unwrap();

        // 使用不同的 UID（应该失败）
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![9999]);
        assert!(auth_stream.is_err(), "应该拒绝不在允许列表中的 UID");

        let err = auth_stream.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "错误类型应该是 PermissionDenied");
    }

    #[tokio::test]
    async fn test_authenticated_stream_empty_allowed_list() {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair().unwrap();
        let tokio_stream = tokio::net::UnixStream::from_std(sock1).unwrap();

        // 空允许列表应该拒绝所有 UID
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![]);
        assert!(auth_stream.is_err(), "空允许列表应该拒绝所有连接");
    }
}
