# 采集权限分离架构设计

## 概述

参考 **bpfman** 架构，将特权操作与非特权主进程分离，最小化攻击面。

## 架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                       用户空间                                    │
│                                                                  │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │                 nuts-observer (普通用户)                 │   │
│  │                    PID: <uid> (如 1000)                    │   │
│  │                                                          │   │
│  │  • HTTP Server (API)                    ┌──────────┐    │   │
│  │  • CLI Interface                        │  Config  │    │   │
│  │  • Diagnosis Engine                     │  Cache   │    │   │
│  │  • Case Library Manager                 └──────────┘    │   │
│  │  • Evidence Analyzer                                    │   │
│  │                                                          │   │
│  │  非特权组件: 不直接执行bpftrace，不访问/proc/<pid>/maps   │   │
│  └────────────────────┬──────────────────────────────────────┘   │
│                       │                                          │
│           ┌───────────┴───────────┐                             │
│           │                       │                             │
│           ▼                       ▼                             │
│  ┌────────────────────┐   ┌──────────────────────────────┐   │
│  │ nuts-collector-    │   │ nuts-config-daemon           │   │
│  │ daemon             │   │ (optional)                  │   │
│  │                    │   │                              │   │
│  │ PID: 0 (root)      │   │ PID: <uid> (普通用户)        │   │
│  │                    │   │ 但有写入配置文件的权限       │   │
│  │ 特权: CAP_BPF,     │   │                              │   │
│  │ CAP_SYS_ADMIN,    │   │ 功能:                         │   │
│  │ CAP_SYS_PTRACE    │   │ • 接收配置更新请求            │   │
│  │                    │   │ • 验证并写入配置文件          │   │
│  │ 功能:              │   │ • 通知主进程重载配置          │   │
│  │ • 执行bpftrace     │   │                              │   │
│  │ • 访问/proc/<pid>  │   │ 优势:                         │   │
│  │ • 加载eBPF程序     │   │ • 配置文件可设为root:root    │   │
│  │ • 采集内核数据     │   │   防止意外修改                │   │
│  │                    │   │                              │   │
│  │ Socket: /run/nuts/ │   │ Socket: /run/nuts/          │   │
│  │   collector.sock   │   │   config.sock                │   │
│  │ (Unix Socket,      │   │                              │   │
│  │  0600 权限)        │   │                              │   │
│  └────────────────────┘   └──────────────────────────────┘   │
│                       │                                          │
│                       ▼                                          │
│  ┌───────────────────────────────────────────────────────────┐   │
│  │                    内核空间                                │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │   │
│  │  │   eBPF VM    │  │   perf     │  │   trace    │     │   │
│  │  │   (BTF)      │  │   buffer   │  │   events   │     │   │
│  │  └──────────────┘  └──────────────┘  └──────────────┘     │   │
│  └───────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## 组件职责

### 1. nuts-collector-daemon (特权组件)

```rust
// src/bin/nuts-collector-daemon/main.rs
//! 特权采集守护进程
//! 
//! 权限要求:
//! - 运行用户: root 或具有 CAP_BPF + CAP_SYS_ADMIN + CAP_SYS_PTRACE
//! - 文件权限: /run/nuts/collector.sock (0660, root:root)
//! 
//! 安全边界:
//! - 只接受来自 Unix Socket 的请求
//! - 验证请求来源（检查 credentials）
//! - 限制单次采集时长和资源使用
//! - 不解析/处理采集数据，只做原始采集

use tonic::{transport::Server, Request, Response, Status};

pub mod collector {
    tonic::include_proto!("nuts.collector");
}

#[derive(Debug)]
struct CollectorService {
    // 权限配置
    allowed_uids: Vec<u32>,  // 允许调用者UID列表
    max_duration_secs: u64,  // 最大采集时长
}

#[tonic::async_trait]
impl collector::collector_server::Collector for CollectorService {
    /// 执行bpftrace采集
    async fn collect_bpftrace(
        &self,
        request: Request<collector::CollectRequest>,
    ) -> Result<Response<collector::CollectResponse>, Status> {
        // 1. 验证调用者身份 (Unix Socket credentials)
        let peer_uid = get_peer_uid(&request)?;
        if !self.allowed_uids.contains(&peer_uid) {
            return Err(Status::permission_denied("Unauthorized"));
        }
        
        let req = request.into_inner();
        
        // 2. 执行采集（在沙箱/超时限制内）
        let output = run_bpftrace_sandboxed(
            &req.script_path,
            req.duration_secs.min(self.max_duration_secs),
            req.scope_pid,
        ).map_err(|e| Status::internal(e.to_string()))?;
        
        // 3. 返回原始输出（不解析）
        Ok(Response::new(collector::CollectResponse {
            raw_output: output,
            collection_id: generate_collection_id(),
            duration_ms: ..., // 实际耗时
        }))
    }
    
    /// 读取/proc/<pid>/... 文件
    async fn read_proc(
        &self,
        request: Request<collector::ReadProcRequest>,
    ) -> Result<Response<collector::ReadProcResponse>, Status> {
        // 1. 验证调用者
        // 2. 限制可访问的 /proc 路径（防止越权）
        // 3. 返回文件内容
    }
}
```

### 2. nuts-observer (非特权主进程)

```rust
// src/collector/collector_client.rs
//! 采集器客户端 - 通过 Unix Socket 连接到特权 daemon

use tonic::transport::{Channel, Endpoint};
use tokio::net::UnixStream;

pub struct CollectorClient {
    client: collector::collector_client::CollectorClient<Channel>,
}

impl CollectorClient {
    /// 连接到 collector daemon
    pub async fn connect(socket_path: &str) -> Result<Self, CollectorError> {
        // 使用 Unix Socket 连接
        let channel = Endpoint::try_from("http://[::]:50051")?
            .connect_with_connector(service_fn(move |_| {
                UnixStream::connect(socket_path)
            })).await?;
        
        Ok(Self {
            client: collector::collector_client::CollectorClient::new(channel),
        })
    }
    
    /// 请求采集
    pub async fn collect(
        &mut self,
        script_path: &str,
        duration_secs: u64,
        scope_pid: Option<u32>,
    ) -> Result<String, CollectorError> {
        let request = tonic::Request::new(CollectRequest {
            script_path: script_path.to_string(),
            duration_secs,
            scope_pid,
        });
        
        let response = self.client.collect_bpftrace(request).await?;
        Ok(response.into_inner().raw_output)
    }
}
```

## gRPC 协议定义

```protobuf
// proto/collector.proto
syntax = "proto3";
package nuts.collector;

// 采集请求
message CollectRequest {
    string task_id = 1;           // 任务ID（用于追踪）
    string script_path = 2;       // bpftrace脚本路径
    uint64 duration_secs = 3;     // 采集时长
    optional uint32 scope_pid = 4; // 目标进程PID（可选）
    string evidence_type = 5;     // 证据类型
    map<string, string> params = 6; // 额外参数
}

// 采集响应
message CollectResponse {
    string collection_id = 1;     // 采集ID
    bytes raw_output = 2;         // 原始输出（JSON Lines）
    uint64 duration_ms = 3;       // 实际耗时
    string status = 4;            // 状态: success, timeout, error
    optional string error_msg = 5; // 错误信息
    uint32 event_count = 6;       // 采集到的事件数
}

// /proc读取请求
message ReadProcRequest {
    string path = 1;              // /proc/下的路径，如 "1234/maps"
    optional uint32 pid = 2;      // 目标PID
}

message ReadProcResponse {
    bytes content = 1;            // 文件内容
    bool exists = 2;              // 文件是否存在
}

// 健康检查
message HealthRequest {}
message HealthResponse {
    bool healthy = 1;
    string version = 2;
    uint32 active_collections = 3;
}

service Collector {
    // 执行bpftrace采集
    rpc CollectBpftrace(CollectRequest) returns (CollectResponse);
    
    // 读取/proc文件
    rpc ReadProc(ReadProcRequest) returns (ReadProcResponse);
    
    // 健康检查
    rpc Health(HealthRequest) returns (HealthResponse);
}
```

## Unix Socket 安全设计

```rust
// 服务端（daemon）创建 socket
use std::os::unix::fs::PermissionsExt;

pub fn create_secure_socket(path: &str) -> Result<UnixListener, std::io::Error> {
    // 确保目录存在且权限正确
    let dir = Path::new(path).parent().unwrap();
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
    
    // 创建 socket 文件
    let listener = UnixListener::bind(path)?;
    
    // 设置权限：只有 root 和 nuts 用户可以访问
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(0o660)
    )?;
    
    // 设置属组（需要 root）
    // chown root:nuts /run/nuts/collector.sock
    
    Ok(listener)
}

// 客户端获取自己的 UID 发送给服务端验证
pub fn get_unix_credentials(stream: &UnixStream) -> Result<UCred, std::io::Error> {
    let cred = stream.peer_cred()?;
    Ok(cred)
}
```

## 部署方案

### 方案A: systemd 服务 (推荐生产环境)

```ini
# /etc/systemd/system/nuts-collector-daemon.service
[Unit]
Description=Nuts Collector Daemon (Privileged)
After=network.target

[Service]
Type=simple
User=root
Group=nuts
ExecStart=/usr/bin/nuts-collector-daemon --socket=/run/nuts/collector.sock
Restart=always
RestartSec=5

# 权限设置
AmbientCapabilities=CAP_BPF CAP_SYS_ADMIN CAP_SYS_PTRACE
CapabilityBoundingSet=CAP_BPF CAP_SYS_ADMIN CAP_SYS_PTRACE
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/run/nuts /tmp

# 资源限制
LimitNOFILE=65536
MemoryMax=512M
CPUQuota=50%

[Install]
WantedBy=multi-user.target
```

### 方案B: 容器部署

```yaml
# docker-compose.yml
version: '3.8'

services:
  nuts-collector:
    image: nuts-collector-daemon:latest
    privileged: true  # 需要特权来加载eBPF
    cap_add:
      - BPF
      - SYS_ADMIN
      - SYS_PTRACE
    volumes:
      - /run/nuts:/run/nuts
      - /tmp:/tmp
    networks:
      - nuts-internal
    
  nuts-observer:
    image: nuts-observer:latest
    user: "1000:1000"  # 非特权用户
    volumes:
      - /run/nuts:/run/nuts:ro  # 只读访问 socket
    depends_on:
      - nuts-collector
    networks:
      - nuts-internal
      - external  # 对外提供API

networks:
  nuts-internal:
    internal: true
  external:
```

## 实现步骤

### 阶段1: 基础架构 (1-2天)

1. **创建 protocol buffer 定义**
   - `proto/collector.proto`
   - 定义请求/响应消息

2. **实现 nuts-collector-daemon**
   - 创建 `src/bin/collector-daemon/main.rs`
   - 实现 gRPC 服务
   - Unix Socket 监听
   - 权限验证

3. **实现 CollectorClient**
   - 创建 `src/collector/collector_client.rs`
   - Unix Socket 连接
   - 请求封装

### 阶段2: 集成采集器 (1-2天)

4. **改造现有采集器**
   - `src/collector/network.rs` - 使用 client 代替直接执行
   - `src/collector/block_io.rs`
   - 添加 fallback 逻辑（如果 daemon 不可用）

5. **错误处理和降级**
   - daemon 不可用时提示用户
   - 开发模式：允许直接 sudo（可选）

### 阶段3: 测试和部署 (1天)

6. **集成测试**
   - 测试 Unix Socket 通信
   - 测试权限验证
   - 测试采集流程

7. **systemd 服务配置**
   - 编写 service 文件
   - 创建安装脚本

## 与现有代码的整合

```rust
// src/collector/network.rs 修改示例

// 旧实现（直接执行）
let child = Command::new("sudo")
    .args(["bpftrace", script_path])
    .spawn()?;

// 新实现（通过 daemon）
async fn collect_via_daemon(
    config: &NetworkCollectorConfig
) -> Result<Evidence, CollectorError> {
    let mut client = CollectorClient::connect("/run/nuts/collector.sock").await?;
    
    let output = client.collect(
        "scripts/bpftrace/network/tcp_connect.bt",
        duration_secs,
        Some(pid),
    ).await?;
    
    // 解析 daemon 返回的原始输出
    parse_bpftrace_output(&output)
}

// 自动降级（如果 daemon 不可用）
pub async fn run_network_collect(cfg: NetworkCollectorConfig) -> Evidence {
    match collect_via_daemon(&cfg).await {
        Ok(evidence) => evidence,
        Err(e) => {
            warn!("Daemon unavailable: {}, falling back to dev mode", e);
            run_network_collect_dev(cfg)  // 旧实现
        }
    }
}
```

## 安全考虑

1. **最小权限原则**
   - daemon 只保留必要 capabilities
   - 主进程以普通用户运行
   - 配置文件分离

2. **输入验证**
   - 验证所有请求参数
   - 限制脚本路径（只允许白名单路径）
   - 限制采集时长

3. **资源限制**
   - cgroup 限制 CPU/内存
   - 超时机制
   - 文件描述符限制

4. **审计日志**
   - 记录所有采集请求
   - 记录调用者身份
   - 记录执行结果

## 优势

| 特性 | 之前 (直接sudo) | 现在 (分离架构) |
|------|----------------|----------------|
| 主进程权限 | root | 普通用户 |
| 攻击面 | 大（整个程序） | 小（仅 daemon）|
| 生产部署 | 不安全 | 安全 |
| 权限粒度 | 全部或没有 | 精细化控制 |
| 审计 | 困难 | 清晰 |
| 容器化 | 需要 --privileged | 可分离特权 |

## 参考

- bpfman: https://github.com/bpfman/bpfman
- Linux capabilities: https://man7.org/linux/man-pages/man7/capabilities.7.html
- gRPC over Unix Socket: https://github.com/hyperium/tonic/blob/master/examples/src/uds/
