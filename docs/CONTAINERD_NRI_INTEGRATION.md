# Containerd NRI 官方协议集成指南

本文档描述了 nuts-observer 与 containerd NRI (Node Resource Interface) 官方协议的集成实现。

## 概述

nuts-observer 现在支持 containerd NRI 官方 gRPC 协议，可以直接作为 NRI Plugin 与 containerd 集成，接收容器生命周期事件。

## 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                       Kubernetes Node                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌──────────────┐      gRPC/Unix Socket      ┌──────────────┐ │
│   │  containerd  │  <──────────────────────>  │ nuts-observer│ │
│   │              │      NRI Protocol          │  NRI Plugin    │ │
│   └──────────────┘                            └──────────────┘ │
│          │                                           │          │
│          │ CRI                                       │ HTTP     │
│          │                                           │          │
│   ┌──────────────┐                            ┌──────────────┐ │
│   │   CRI Shim   │                            │   HTTP API   │ │
│   │  (containerd)│                            │   (Port 8080)│ │
│   └──────────────┘                            └──────────────┘ │
│                                                          │       │
│                                                   ┌──────────┐   │
│                                                   │  CLI/UI  │   │
│                                                   └──────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## 实现组件

### 1. Protobuf 定义

- **文件**: `proto/nri.proto`
- **描述**: containerd NRI 官方协议的 protobuf 定义
- **包含服务**:
  - `Runtime`: containerd 暴露给插件的服务（注册、更新容器）
  - `Plugin`: 插件暴露给 containerd 的服务（配置、同步、容器事件）

### 2. gRPC 服务实现

- **文件**: `src/collector/nri_containerd.rs`
- **描述**: containerd NRI Plugin 的完整实现
- **功能**:
  - Unix Socket 监听 (`/var/run/nri/nuts-observer.sock`)
  - 自动向 containerd 注册
  - 处理 Configure/Synchronize/CreateContainer/UpdateContainer/StopContainer 事件
  - 转换为内部 NRI 事件格式

### 3. 集成启动

- **文件**: `src/main.rs`
- **描述**: 自动启动 containerd NRI 服务（当启用 `nri-grpc` feature 时）

## 部署方式

### 方式 1: 二进制直接部署（适合单节点测试）

```bash
# 编译启用 nri-grpc 特性
cargo build --release --features nri-grpc

# 复制二进制文件
sudo cp target/release/nuts-observer /usr/bin/

# 创建 NRI 配置目录
sudo mkdir -p /etc/nri/conf.d

# 复制 NRI 插件配置
sudo cp deploy/nri/nuts-observer-nri.toml /etc/nri/conf.d/

# 启动服务
sudo nuts-observer
```

### 方式 2: Systemd 服务部署

```bash
# 复制 systemd 服务文件
sudo cp systemd/nuts-observer.service /etc/systemd/system/

# 重载 systemd
sudo systemctl daemon-reload

# 启动服务
sudo systemctl start nuts-observer
sudo systemctl enable nuts-observer
```

### 方式 3: Kubernetes DaemonSet 部署（推荐生产环境）

```bash
# 部署到 Kubernetes
kubectl apply -f deploy/kubernetes/nuts-observer-nri-daemonset.yaml

# 验证部署
kubectl get pods -n kube-system -l app=nuts-observer-nri
kubectl logs -n kube-system -l app=nuts-observer-nri
```

## Containerd 配置

### 启用 NRI 支持

编辑 `/etc/containerd/config.toml`:

```toml
version = 2

[plugins."io.containerd.nri.v1.nri"]
  # 启用 NRI
  disable = false
  
  # NRI 套接字目录
  socket_path = "/var/run/nri"
  
  # 插件配置目录
  plugin_config_path = "/etc/nri/conf.d"
  
  # 插件扫描目录
  plugin_path = "/var/run/nri"
```

### 重启 containerd

```bash
sudo systemctl restart containerd
```

## 验证集成

### 1. 检查 NRI Socket

```bash
# 检查套接字文件是否存在
ls -la /var/run/nri/nuts-observer.sock

# 检查权限
stat /var/run/nri/nuts-observer.sock
```

### 2. 检查 containerd 日志

```bash
# 查看 containerd 是否识别到插件
sudo journalctl -u containerd -f | grep -i nri
```

### 3. 检查 nuts-observer 日志

```bash
# 查看 NRI 相关日志
sudo journalctl -u nuts-observer -f | grep -i "ContainerdNri"
```

### 4. 创建测试 Pod

```bash
# 创建测试 Pod
kubectl run test-pod --image=nginx:alpine

# 检查 nuts-observer 是否收到 NRI 事件
kubectl logs -n kube-system -l app=nuts-observer-nri | grep -i "CreateContainer"

# 删除测试 Pod
kubectl delete pod test-pod
```

### 5. 运行 E2E 测试

```bash
# 运行端到端测试脚本
./tests/e2e_containerd_nri.sh
```

## 参考

- [Containerd NRI GitHub](https://github.com/containerd/nri)
- [NRI Protocol Documentation](https://github.com/containerd/nri/tree/main/pkg/api)
