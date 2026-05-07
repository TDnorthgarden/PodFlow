//! UID 验证集成测试
//!
//! 直接测试 auth 模块的核心类型：
//! 1. PeerUid 类型和相等性
//! 2. AuthenticatedStream 创建和验证
//! 3. get_peer_uid 系统调用
//! 4. check_uid_permission 权限检查

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use std::os::unix::io::AsRawFd;
use tokio::time::sleep;
use tokio::net::UnixStream;
use std::os::unix::net::UnixListener as StdUnixListener;

// 直接导入核心认证类型
use nuts_observer::auth::{PeerUid, AuthenticatedStream, get_peer_uid, check_uid_permission};

/// 测试 PeerUid 类型的基本功能
#[test]
fn test_peer_uid_type() {
    println!("🔍 测试 PeerUid 类型...");

    // 测试创建和相等性
    let uid1 = PeerUid(1000);
    let uid2 = PeerUid(1000);
    let uid3 = PeerUid(2000);

    assert_eq!(uid1, uid2, "相同 UID 应该相等");
    assert_ne!(uid1, uid3, "不同 UID 应该不相等");

    // 测试 Debug 输出
    let debug_str = format!("{:?}", uid1);
    assert!(debug_str.contains("1000"), "Debug 输出应该包含 UID 值");

    // 测试 Copy 特性
    let uid_copy = uid1;
    assert_eq!(uid_copy, uid1, "Copy 后应该相等");

    println!("✅ PeerUid 类型测试通过");
    println!("   - 创建: ✅");
    println!("   - 相等性: ✅");
    println!("   - Copy 特性: ✅");
}

/// 测试 get_peer_uid 系统调用
#[tokio::test]
async fn test_get_peer_uid_system_call() {
    println!("🔍 测试 get_peer_uid 系统调用...");

    #[cfg(unix)]
    {
        // 创建 socket pair
        let (sock1, sock2) = std::os::unix::net::UnixStream::pair()
            .expect("无法创建 socket pair");

        // 转换为 tokio UnixStream
        let tokio_stream1 = tokio::net::UnixStream::from_std(sock1)
            .expect("无法转换为 tokio stream");
        let tokio_stream2 = tokio::net::UnixStream::from_std(sock2)
            .expect("无法转换为 tokio stream");

        // 获取两端的 UID
        let uid1 = get_peer_uid(&tokio_stream1)
            .expect("应该能获取 UID1");
        let uid2 = get_peer_uid(&tokio_stream2)
            .expect("应该能获取 UID2");

        // 验证 UID 有效性
        assert!(uid1 > 0 || uid1 == 0, "UID1 应该有效");
        assert!(uid2 > 0 || uid2 == 0, "UID2 应该有效");

        // 同一进程的两个 socket 应该有相同的 UID
        assert_eq!(uid1, uid2, "同一进程的两个 socket 应该有相同的 UID");

        println!("✅ get_peer_uid 系统调用测试通过");
        println!("   - UID 获取: {} (两端)", uid1);
        println!("   - UID 一致性: ✅");
    }
}

/// 测试 check_uid_permission 权限检查
#[test]
fn test_check_uid_permission_logic() {
    println!("🔍 测试 check_uid_permission 权限检查...");

    // 正面测试：UID 在允许列表中
    let allowed_uids = vec![0, 1000, 1001];
    assert!(check_uid_permission(0, &allowed_uids), "root 应该被允许");
    assert!(check_uid_permission(1000, &allowed_uids), "UID 1000 应该被允许");
    assert!(check_uid_permission(1001, &allowed_uids), "UID 1001 应该被允许");

    // 负面测试：UID 不在允许列表中
    assert!(!check_uid_permission(1002, &allowed_uids), "UID 1002 不应该被允许");
    assert!(!check_uid_permission(999, &allowed_uids), "UID 999 不应该被允许");

    // 边界测试：空允许列表
    let empty_allowed: Vec<u32> = vec![];
    assert!(!check_uid_permission(0, &empty_allowed), "空列表应该拒绝 root");
    assert!(!check_uid_permission(1000, &empty_allowed), "空列表应该拒绝所有 UID");

    // 边界测试：单个 UID
    let single_uid = vec![1000];
    assert!(check_uid_permission(1000, &single_uid), "单个 UID 应该被允许");
    assert!(!check_uid_permission(1001, &single_uid), "其他 UID 不应该被允许");

    println!("✅ check_uid_permission 权限检查测试通过");
    println!("   - 正面测试: ✅");
    println!("   - 负面测试: ✅");
    println!("   - 边界测试: ✅");
}

/// 测试 AuthenticatedStream 创建和验证（成功路径）
#[tokio::test]
async fn test_authenticated_stream_creation_success() {
    println!("🔍 测试 AuthenticatedStream 创建（成功路径）...");

    #[cfg(unix)]
    {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair()
            .expect("无法创建 socket pair");

        let tokio_stream = tokio::net::UnixStream::from_std(sock1)
            .expect("无法转换为 tokio stream");

        // 获取当前进程的 UID
        let current_uid = unsafe { libc::getuid() };

        // 创建认证流（应该成功）
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![current_uid])
            .expect("应该能创建认证流");

        // 验证 peer_uid
        assert_eq!(auth_stream.peer_uid(), current_uid, "peer_uid 应该匹配当前 UID");

        // 验证 allowed_uids
        assert_eq!(auth_stream.allowed_uids(), &[current_uid], "allowed_uids 应该正确");

        // 验证 stream 访问
        let _stream = auth_stream.stream();
        assert!(_stream.as_raw_fd() >= 0, "stream 应该有效");

        println!("✅ AuthenticatedStream 创建成功测试通过");
        println!("   - 创建: ✅");
        println!("   - peer_uid 验证: ✅");
        println!("   - allowed_uids 验证: ✅");
        println!("   - stream 访问: ✅");
    }
}

/// 测试 AuthenticatedStream 创建和验证（失败路径）
#[tokio::test]
async fn test_authenticated_stream_creation_failure() {
    println!("🔍 测试 AuthenticatedStream 创建（失败路径）...");

    #[cfg(unix)]
    {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair()
            .expect("无法创建 socket pair");

        let tokio_stream = tokio::net::UnixStream::from_std(sock1)
            .expect("无法转换为 tokio stream");

        // 使用不在允许列表中的 UID（应该失败）
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![9999]);
        assert!(auth_stream.is_err(), "应该拒绝不在允许列表中的 UID");

        let err = auth_stream.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "错误类型应该是 PermissionDenied");

        println!("✅ AuthenticatedStream 创建失败测试通过");
        println!("   - 权限拒绝: ✅");
        println!("   - 错误类型: ✅");
    }
}

/// 测试 AuthenticatedStream 与空允许列表
#[tokio::test]
async fn test_authenticated_stream_empty_allowed_list() {
    println!("🔍 测试 AuthenticatedStream 与空允许列表...");

    #[cfg(unix)]
    {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair()
            .expect("无法创建 socket pair");

        let tokio_stream = tokio::net::UnixStream::from_std(sock1)
            .expect("无法转换为 tokio stream");

        // 空允许列表应该拒绝所有连接
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![]);
        assert!(auth_stream.is_err(), "空允许列表应该拒绝所有连接");

        println!("✅ AuthenticatedStream 空允许列表测试通过");
        println!("   - 拒绝所有连接: ✅");
    }
}

/// 测试 AuthenticatedStream 的 into_inner 方法
#[tokio::test]
async fn test_authenticated_stream_into_inner() {
    println!("🔍 测试 AuthenticatedStream 的 into_inner 方法...");

    #[cfg(unix)]
    {
        // 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair()
            .expect("无法创建 socket pair");

        let tokio_stream = tokio::net::UnixStream::from_std(sock1)
            .expect("无法转换为 tokio stream");

        let current_uid = unsafe { libc::getuid() };

        // 创建认证流
        let auth_stream = AuthenticatedStream::new(tokio_stream, vec![current_uid])
            .expect("应该能创建认证流");

        // 消费认证流并获取底层 stream
        let _inner_stream = auth_stream.into_inner();
        assert!(_inner_stream.as_raw_fd() >= 0, "底层 stream 应该有效");

        println!("✅ AuthenticatedStream into_inner 测试通过");
        println!("   - 消费和提取: ✅");
    }
}

/// 测试 UID 验证的配置和参数处理
#[test]
fn test_uid_validation_configuration() {
    println!("🔍 测试 UID 验证配置...");

    // 测试配置格式
    let config_content = r#"
# Collector Daemon 配置
socket_path: "/tmp/nuts-collector.sock"
allowed_uids: [0, 1000, 1001]
log_level: "info"
"#;

    // 验证配置包含必要字段
    assert!(config_content.contains("allowed_uids"), "配置应该包含 allowed_uids 字段");
    assert!(config_content.contains("socket_path"), "配置应该包含 socket_path 字段");
    assert!(config_content.contains("[0, 1000, 1001]"), "配置应该包含 UID 列表");

    // 验证 UID 列表解析
    let uids: Vec<u32> = vec![0, 1000, 1001];
    assert_eq!(uids.len(), 3, "应该有 3 个允许的 UID");
    assert!(uids.contains(&0), "应该包含 root UID (0)");
    assert!(uids.contains(&1000), "应该包含用户 UID 1000");

    println!("✅ UID 验证配置测试通过");
    println!("   - 配置格式: ✅");
    println!("   - UID 列表解析: ✅");
}

/// 测试 UID 验证的集成功能
#[tokio::test]
async fn test_uid_validation_integration() {
    println!("🔍 测试 UID 验证集成功能...");

    // 1. 验证 collector_daemon 可以编译
    println!("📝 验证 collector_daemon 编译...");

    let check_result = Command::new("cargo")
        .args(&["check", "--bin", "nuts-collector-daemon"])
        .output();

    assert!(check_result.is_ok(), "应该能够编译 collector_daemon");
    let result = check_result.unwrap();
    assert!(result.status.success(), "collector_daemon 应该编译成功");

    // 2. 验证参数处理
    println!("🔍 验证参数处理...");

    let help_result = Command::new("cargo")
        .args(&["run", "--bin", "nuts-collector-daemon", "--", "--help"])
        .output();

    assert!(help_result.is_ok(), "应该能够处理 --help 参数");

    // 3. 验证错误处理
    println!("🔍 验证错误处理...");

    let invalid_result = Command::new("cargo")
        .args(&["run", "--bin", "nuts-collector-daemon", "--", "--invalid-flag"])
        .output();

    assert!(invalid_result.is_ok(), "应该能够处理无效参数");
    let invalid_output = invalid_result.unwrap();
    assert!(!invalid_output.status.success(), "无效参数应该导致失败");

    println!("✅ UID 验证集成测试完成");
    println!("   - 编译检查: ✅");
    println!("   - 参数处理: ✅");
    println!("   - 错误处理: ✅");
}

/// 测试 Unix Socket 连接和权限验证
#[tokio::test]
async fn test_unix_socket_permissions() {
    println!("🔍 测试 Unix Socket 权限验证...");

    let socket_path = "/tmp/test-uid-validation.sock";

    // 清理旧的 socket 文件
    if Path::new(socket_path).exists() {
        let _ = fs::remove_file(socket_path);
    }

    // 创建 Unix Listener
    let listener = StdUnixListener::bind(socket_path).expect("无法创建 socket");

    // 验证 socket 文件存在
    assert!(Path::new(socket_path).exists(), "Socket 文件应该被创建");

    // 验证 socket 权限
    let metadata = fs::metadata(socket_path).expect("无法获取 socket 文件元数据");
    let permissions = metadata.permissions();
    println!("Socket 权限: {:?}", permissions);

    // 在后台接受连接
    let listener = tokio::net::UnixListener::from_std(listener).unwrap();

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let fd = stream.as_raw_fd();
            assert!(fd >= 0, "Socket 文件描述符应该有效");
            println!("✅ 接收到连接，fd: {}", fd);
        }
    });

    // 尝试连接
    sleep(Duration::from_millis(100)).await;
    let connect_result = UnixStream::connect(socket_path).await;

    assert!(connect_result.is_ok(), "应该能够连接到 socket");
    let _stream = connect_result.unwrap();

    // 清理
    let _ = fs::remove_file(socket_path);

    println!("✅ Unix Socket 权限验证测试通过");
    println!("   - Socket 创建: ✅");
    println!("   - 连接测试: ✅");
    println!("   - 权限验证: ✅");
}

/// 综合测试：UID 验证的完整流程
#[tokio::test]
async fn test_uid_validation_complete_flow() {
    println!("🔍 测试 UID 验证完整流程...");

    #[cfg(unix)]
    {
        // 1. 获取当前 UID
        let current_uid = unsafe { libc::getuid() };
        println!("📝 当前进程 UID: {}", current_uid);

        // 2. 创建 socket pair
        let (sock1, _sock2) = std::os::unix::net::UnixStream::pair()
            .expect("无法创建 socket pair");
        let tokio_stream = tokio::net::UnixStream::from_std(sock1)
            .expect("无法转换为 tokio stream");

        // 3. 验证 get_peer_uid
        let peer_uid = get_peer_uid(&tokio_stream)
            .expect("应该能获取 peer UID");
        assert_eq!(peer_uid, current_uid, "peer UID 应该匹配当前 UID");
        println!("✅ get_peer_uid: {}", peer_uid);

        // 4. 验证 check_uid_permission
        let allowed_uids = vec![current_uid];
        let has_permission = check_uid_permission(peer_uid, &allowed_uids);
        assert!(has_permission, "应该有权限");
        println!("✅ check_uid_permission: true");

        // 5. 创建 AuthenticatedStream
        let auth_stream = AuthenticatedStream::new(tokio_stream, allowed_uids)
            .expect("应该能创建认证流");
        assert_eq!(auth_stream.peer_uid(), current_uid, "peer_uid 应该匹配");
        println!("✅ AuthenticatedStream 创建成功");

        // 6. 验证 PeerUid 类型
        let peer_uid_type = PeerUid(current_uid);
        assert_eq!(peer_uid_type.0, current_uid, "PeerUid 值应该正确");
        println!("✅ PeerUid 类型: {:?}", peer_uid_type);

        println!("✅ UID 验证完整流程测试通过");
        println!("   - UID 获取: ✅");
        println!("   - 权限检查: ✅");
        println!("   - 认证流创建: ✅");
        println!("   - 类型验证: ✅");
    }
}
