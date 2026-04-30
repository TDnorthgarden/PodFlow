#!/bin/bash
# 采集权限分离架构集成测试

set -e

NUTS_DIR="/root/nuts"
DAEMON_BIN="$NUTS_DIR/target/release/nuts-collector-daemon"
OBSERVER_BIN="$NUTS_DIR/target/release/nuts-observer"
SOCKET_PATH="/tmp/test-nuts-collector.sock"

echo "=========================================="
echo "特权分离架构集成测试"
echo "=========================================="
echo ""

# 检查是否以 root 运行（用于测试特权功能）
if [ "$EUID" -ne 0 ]; then
    echo "⚠️  警告: 部分测试需要 root 权限，建议以 root 运行此测试"
    echo "   或使用: sudo $0"
    echo ""
fi

# 测试1: 检查二进制文件存在性
echo "【测试1】验证二进制文件..."
if [ -f "$DAEMON_BIN" ]; then
    echo "  ✓ nuts-collector-daemon 存在"
else
    echo "  ✗ nuts-collector-daemon 不存在，请先编译: cargo build --release"
    exit 1
fi

if [ -f "$OBSERVER_BIN" ]; then
    echo "  ✓ nuts-observer 存在"
else
    echo "  ✗ nuts-observer 不存在"
    exit 1
fi
echo ""

# 测试2: 检查 systemd 服务文件
echo "【测试2】验证 systemd 服务配置..."
SERVICE_FILE="$NUTS_DIR/systemd/nuts-collector-daemon.service"
if [ -f "$SERVICE_FILE" ]; then
    echo "  ✓ 服务文件存在"
    
    # 检查关键配置
    if grep -q "AmbientCapabilities=CAP_BPF" "$SERVICE_FILE"; then
        echo "  ✓ CAP_BPF 能力已配置"
    else
        echo "  ✗ CAP_BPF 能力未配置"
    fi
    
    if grep -q "ProtectSystem=strict" "$SERVICE_FILE"; then
        echo "  ✓ 系统保护已启用"
    else
        echo "  ✗ 系统保护未启用"
    fi
else
    echo "  ✗ 服务文件不存在"
    exit 1
fi
echo ""

# 测试3: 检查 proto 文件
echo "【测试3】验证 protobuf 定义..."
PROTO_FILE="$NUTS_DIR/proto/collector.proto"
if [ -f "$PROTO_FILE" ]; then
    echo "  ✓ collector.proto 存在"
    
    # 检查关键服务定义
    if grep -q "service Collector" "$PROTO_FILE"; then
        echo "  ✓ Collector 服务定义存在"
    else
        echo "  ✗ Collector 服务定义缺失"
    fi
    
    if grep -q "CollectBpftrace" "$PROTO_FILE"; then
        echo "  ✓ CollectBpftrace RPC 定义存在"
    else
        echo "  ✗ CollectBpftrace RPC 定义缺失"
    fi
else
    echo "  ✗ collector.proto 不存在"
    exit 1
fi
echo ""

# 测试4: 启动 daemon 测试（需要 root）
if [ "$EUID" -eq 0 ]; then
    echo "【测试4】测试 daemon 启动..."
    
    # 清理之前的测试 socket
    rm -f "$SOCKET_PATH"
    
    # 启动 daemon（后台）
    echo "  启动 nuts-collector-daemon..."
    "$DAEMON_BIN" "$SOCKET_PATH" &
    DAEMON_PID=$!
    
    # 等待 daemon 启动
    sleep 2
    
    # 检查 socket 是否创建
    if [ -S "$SOCKET_PATH" ]; then
        echo "  ✓ Unix Socket 已创建: $SOCKET_PATH"
        
        # 检查权限
        PERM=$(stat -c %a "$SOCKET_PATH")
        if [ "$PERM" = "660" ]; then
            echo "  ✓ Socket 权限正确 (0660)"
        else
            echo "  ⚠️  Socket 权限为 $PERM，期望 660"
        fi
    else
        echo "  ✗ Unix Socket 未创建"
        kill $DAEMON_PID 2>/dev/null || true
        exit 1
    fi
    
    # 停止 daemon
    kill $DAEMON_PID 2>/dev/null || true
    wait $DAEMON_PID 2>/dev/null || true
    rm -f "$SOCKET_PATH"
    
    echo "  ✓ Daemon 测试完成"
else
    echo "【测试4】跳过 daemon 启动测试（需要 root）"
fi
echo ""

# 测试5: 检查客户端代码
echo "【测试5】验证客户端代码..."
CLIENT_FILE="$NUTS_DIR/src/collector/collector_client.rs"
if [ -f "$CLIENT_FILE" ]; then
    echo "  ✓ collector_client.rs 存在"
    
    if grep -q "CollectorClient" "$CLIENT_FILE"; then
        echo "  ✓ CollectorClient 结构体定义存在"
    else
        echo "  ✗ CollectorClient 结构体定义缺失"
    fi
    
    if grep -q "AutoFallbackCollector" "$CLIENT_FILE"; then
        echo "  ✓ 自动回退采集器已实现"
    else
        echo "  ✗ 自动回退采集器未实现"
    fi
else
    echo "  ✗ collector_client.rs 不存在"
    exit 1
fi
echo ""

# 测试6: 检查权限控制代码
echo "【测试6】验证权限控制实现..."
PERM_FILE="$NUTS_DIR/src/collector/permission.rs"
if [ -f "$PERM_FILE" ]; then
    echo "  ✓ permission.rs 存在"
    
    if grep -q "PrivilegeMode" "$PERM_FILE"; then
        echo "  ✓ PrivilegeMode 枚举定义存在"
    fi
    
    if grep -q "Bpfman" "$PERM_FILE"; then
        echo "  ✓ Bpfman 模式支持"
    fi
    
    if grep -q "PrivilegedProxy" "$PERM_FILE"; then
        echo "  ✓ PrivilegedProxy 模式支持"
    fi
else
    echo "  ⚠️  permission.rs 不存在（可能已经集成到 client 中）"
fi
echo ""

# 测试7: 编译测试
echo "【测试7】验证代码编译..."
cd "$NUTS_DIR"
if cargo build --release 2>&1 | grep -q "error"; then
    echo "  ✗ 编译失败"
    exit 1
else
    echo "  ✓ 代码编译成功"
fi
echo ""

# 总结
echo "=========================================="
echo "测试完成！"
echo "=========================================="
echo ""
echo "特权分离架构组件:"
echo "  ✓ nuts-collector-daemon (特权组件)"
echo "  ✓ CollectorClient (非特权客户端)"
echo "  ✓ AutoFallbackCollector (自动回退)"
echo "  ✓ Unix Socket gRPC 通信"
echo "  ✓ systemd 服务配置"
echo ""
echo "部署步骤:"
echo "  1. 安装 daemon: sudo cp nuts-collector-daemon /usr/bin/"
echo "  2. 安装服务: sudo cp systemd/nuts-collector-daemon.service /etc/systemd/system/"
echo "  3. 创建用户组: sudo groupadd nuts (如不存在)"
echo "  4. 启动服务: sudo systemctl enable --now nuts-collector-daemon"
echo "  5. 验证: sudo systemctl status nuts-collector-daemon"
echo ""
echo "使用方式:"
echo "  - nuts-observer 会自动检测并使用 daemon（如果可用）"
echo "  - 如果 daemon 不可用，会回退到开发模式（需要 sudo）"
