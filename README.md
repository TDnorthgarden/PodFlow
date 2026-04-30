# Nuts Observer - 容器智能故障分析插件

[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()

Nuts Observer 是一个面向容器环境的智能故障诊断插件，基于 eBPF/bpftrace 采集内核级观测数据，通过规则引擎和 AI 增强生成诊断结论，并支持告警推送。

## ✨ 核心特性

- 🔍 **智能故障诊断**：基于 eBPF/bpftrace 的内核级观测数据采集
- 🧠 **AI 增强分析**：支持 OpenAI/Claude/本地模型增强诊断结论
- 🚨 **实时告警**：结构化日志输出 + 告警平台推送
- 🛡️ **权限分离**：特权采集守护进程 + 普通权限主服务架构
- 📊 **多维证据采集**：网络、I/O、系统调用、cgroup 资源争抢等
- ⚡ **高性能**：Rust 实现，Tokio 异步运行时，Axum Web 框架

## 📋 支持的功能

| 功能模块 | 描述 | 状态 |
|---------|------|------|
| 网络连通性检测 | TCP 连接延迟、丢包率、连通率统计 | ✅ |
| 块设备 I/O 延迟分析 | I/O 延迟分位值、吞吐量、队列深度 | ✅ |
| 文件系统卡顿分析 | 文件系统操作延迟统计 | ✅ |
| 系统调用耗时统计 | Top N 系统调用延迟分析 | ✅ |
| cgroup 资源争抢检测 | CPU/内存/IO 资源争用分析 | ✅ |
| OOM 事件自动检测 | 自动触发诊断并扩大采集范围 | ✅ |
| AI 增强诊断 | 支持多种 AI 模型增强诊断结论 | ✅ |
| 告警平台集成 | 结构化日志输出 + 告警推送 | ✅ |
| NRI 集成 | 支持 Kubernetes NRI 协议 | ✅ |

## 🚀 快速开始

### 系统要求

- **操作系统**: Linux 内核 5.8+（支持 eBPF）
- **Rust 工具链**: 1.70+（编译时需要）
- **bpftrace**: v0.19+
- **权限**: root 或 CAP_BPF, CAP_SYS_ADMIN, CAP_SYS_PTRACE

### 安装依赖

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install -y bpftrace curl build-essential pkg-config libssl-dev

# CentOS/RHEL
sudo yum install -y bpftrace curl gcc make openssl-devel

# openEuler
sudo dnf install -y bpftrace curl gcc make openssl-devel
```

### 从源码编译

```bash
# 克隆项目
git clone https://github.com/your-username/nuts-observer.git
cd nuts-observer

# 编译项目
cargo build --release

# 安装 bpftrace 脚本
sudo mkdir -p /usr/share/nuts/bpftrace
sudo cp -r scripts/bpftrace/* /usr/share/nuts/bpftrace/
sudo chmod -R 755 /usr/share/nuts/bpftrace
```

### 快速体验

```bash
# 启动服务（需要 root 权限）
sudo ./target/release/nuts-observer

# 在另一个终端使用 CLI
./target/release/nuts-observer-cli trigger --target pod:nginx --evidence-types network,block_io
```

## 🏗️ 架构设计

### 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                    Kubernetes / Runtime                     │
│  ┌─────────────┐                                            │
│  │    NRI      │─────Pod Metadata─────┐                    │
│  └─────────────┘                      │                    │
└───────────────────────────────────────┼────────────────────┘
                                        │
┌───────────────────────────────────────▼────────────────────┐
│                 Nuts Observer (Core Plugin)                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   Trigger   │  │  Collector  │  │  Evidence   │        │
│  │   Service   │  │ (bpftrace)  │  │ Aggregator  │        │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘        │
│         │                │                 │               │
│         └────────────────┼─────────────────┘               │
│                          │                                 │
│                  ┌───────▼───────┐                        │
│                  │   Diagnosis   │                        │
│                  │    Engine     │                        │
│                  └───────┬───────┘                        │
│                          │                                 │
│                  ┌───────▼───────┐  ┌─────────────┐      │
│                  │    Result     │  │     AI      │      │
│                  │   Publisher   │◄─┤   Adapter   │      │
│                  └───────┬───────┘  └─────────────┘      │
│                          │                                 │
└──────────────────────────┼─────────────────────────────────┘
                           │
                  ┌────────▼────────┐
                  │  Output Targets │
                  │ ┌─────────────┐ │
                  │ │ Local Logs  │ │
                  │ │  (JSON)     │ │
                  │ └─────────────┘ │
                  │ ┌─────────────┐ │
                  │ │ Alert       │ │
                  │ │ Platform    │ │
                  │ └─────────────┘ │
                  └─────────────────┘
```

### 权限分离架构

```
┌─────────────────┐    gRPC/Unix Socket    ┌─────────────────┐
│  Main Service   │◄──────────────────────►│  Collector      │
│  (non-privileged)│                        │  Daemon         │
│                 │                        │  (privileged)   │
│  - HTTP API     │                        │  - bpftrace     │
│  - CLI          │                        │  - eBPF         │
│  - Rule Engine  │                        │  - kernel       │
│  - AI Adapter   │                        │  monitoring     │
└─────────────────┘                        └─────────────────┘
```

## 📁 项目结构

```
nuts-observer/
├── src/
│   ├── main.rs              # 主服务入口
│   ├── lib.rs               # 库定义
│   ├── bin/
│   │   ├── nuts_observer_cli.rs      # CLI 工具
│   │   └── collector_daemon.rs       # 特权采集守护进程
│   ├── api/                 # HTTP API 实现
│   ├── collector/           # 数据采集模块
│   ├── diagnosis/           # 诊断引擎
│   ├── ai/                  # AI 增强模块
│   ├── publisher/           # 结果发布器
│   └── types/               # 类型定义
├── scripts/
│   └── bpftrace/            # bpftrace 采集脚本
│       ├── network/         # 网络诊断脚本
│       ├── block_io/        # 块设备 I/O 脚本
│       ├── syscall_latency/ # 系统调用延迟脚本
│       └── templates/       # 脚本模板
├── systemd/                 # systemd 服务文件
├── docs/                    # 项目文档
├── examples/                # 使用示例
├── cases/                   # 故障案例库
├── proto/                   # gRPC 协议定义
├── plans/                   # 项目规划文档
├── Cargo.toml              # Rust 项目配置
├── config.yaml             # 主配置文件
└── README.md               # 本文档
```

## 🔧 配置说明

### 主配置文件

创建 `/etc/nuts/config.yaml`：

```yaml
# 服务器配置
server:
  bind_address: "0.0.0.0"
  port: 8080

# 日志级别
log_level: "info"

# AI 配置（可选）
ai:
  enabled: true
  provider: "openai"  # openai, claude, local
  model: "gpt-4"
  api_key: "your-api-key"
  timeout_secs: 30

# 告警配置（可选）
alert:
  enabled: true
  webhook_url: "https://your-alert-platform.com/webhook"
  throttle_secs: 60

# 条件触发器
condition_triggers:
  - name: "high_io_latency"
    condition: "block_io.io_latency_p99_ms > 100"
    evidence_types: ["block_io", "syscall_latency"]
    window_seconds: 30
    cooldown_seconds: 300
    enabled: true

# 采集器配置
collector:
  daemon_socket: "/run/nuts/collector.sock"
  fallback_mode: "dev_sudo"
  max_collection_time_secs: 60
```

## 📖 使用指南

### HTTP API

#### 触发诊断

```bash
curl -X POST http://localhost:8080/v1/diagnostics:trigger \
  -H "Content-Type: application/json" \
  -d '{
    "target": {
      "pod": {
        "namespace": "default",
        "name": "nginx"
      }
    },
    "evidence_types": ["network", "block_io"],
    "time_window": {
      "start_time": "2024-01-01T00:00:00Z",
      "end_time": "2024-01-01T00:01:00Z"
    }
  }'
```

#### 查询诊断结果

```bash
curl "http://localhost:8080/v1/diagnostics/<task-id>"
```

### CLI 工具

```bash
# 查看帮助
./target/release/nuts-observer-cli --help

# 触发诊断
./target/release/nuts-observer-cli trigger \
  --target pod:nginx \
  --namespace default \
  --evidence-types network,block_io

# 查询结果
./target/release/nuts-observer-cli query --task-id <task-id>

# 实时监控
./target/release/nuts-observer-cli watch --target pod:nginx --interval 10
```

## 🐳 容器化部署

### 构建镜像

```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y bpftrace && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/nuts-observer /usr/local/bin/
COPY --from=builder /app/target/release/nuts-collector-daemon /usr/local/bin/
COPY --from=builder /app/scripts/bpftrace /usr/share/nuts/bpftrace
COPY config.yaml /etc/nuts/config.yaml
EXPOSE 8080
CMD ["nuts-observer"]
```

### 运行容器

```bash
docker run -d \
  --name nuts-observer \
  --privileged \
  --pid=host \
  -v /sys/fs/cgroup:/sys/fs/cgroup:ro \
  -v /run/nuts:/run/nuts \
  -v /etc/nuts:/etc/nuts:ro \
  -p 8080:8080 \
  nuts-observer:latest
```

## 🏭 生产环境部署

### Systemd 服务部署

```bash
# 创建系统用户和组
sudo groupadd nuts
sudo useradd -r -g nuts -s /bin/false nuts

# 创建目录
sudo mkdir -p /etc/nuts /var/log/nuts /run/nuts /usr/share/nuts/bpftrace

# 复制文件
sudo cp systemd/*.service /etc/systemd/system/
sudo cp config.yaml /etc/nuts/
sudo cp -r scripts/bpftrace/* /usr/share/nuts/bpftrace/

# 启动服务
sudo systemctl daemon-reload
sudo systemctl enable nuts-collector-daemon nuts-observer
sudo systemctl start nuts-collector-daemon nuts-observer
```

## 📊 支持的证据类型

| 证据类型 | 采集指标 | 输出字段 |
|---------|---------|----------|
| `network` | TCP 连接延迟、丢包率、连通率 | `latency_p99_ms`, `loss_rate`, `connect_rate` |
| `block_io` | I/O 延迟、吞吐量、队列深度 | `io_latency_p99_ms`, `throughput_mbps`, `queue_depth` |
| `syscall_latency` | 系统调用延迟统计 | `top_syscalls`, `p99_latency_ms`, `call_count` |
| `fs_stall` | 文件系统卡顿分析 | `stall_duration_ms`, `operation_type`, `file_path` |
| `cgroup_contention` | cgroup 资源争抢 | `cpu_throttle_rate`, `memory_usage_percent`, `io_wait_time` |
| `oom` | OOM 事件检测 | `oom_time`, `victim_pid`, `memory_usage_before` |

## 🤖 AI 增强功能

### 支持的 AI 提供商

```yaml
# OpenAI
ai:
  enabled: true
  provider: "openai"
  model: "gpt-4"
  api_key: "sk-..."

# Claude
ai:
  enabled: true
  provider: "claude"
  model: "claude-3-opus-20240229"
  api_key: "sk-ant-..."

# 本地模型（如 vLLM）
ai:
  enabled: true
  provider: "local"
  model: "qwen/qwen3-coder-next"
  api_base: "http://localhost:1234/v1"
```

### AI 输出示例

```json
{
  "ai_enhancement": {
    "status": "completed",
    "summary": "检测到网络延迟异常升高，可能与后端服务负载过高有关",
    "root_cause_analysis": "1. 网络延迟 P99 从 50ms 升高到 200ms\n2. 同时观察到 CPU 使用率上升 30%\n3. 后端服务响应时间同步增加",
    "actionable_steps": [
      "检查后端服务监控指标",
      "验证网络带宽使用情况",
      "考虑增加服务实例或优化查询"
    ],
    "confidence": 0.85
  }
}
```

## 🔍 故障案例库

项目内置 openEuler 社区常见故障模式：

```bash
# 查看所有案例
./target/release/nuts-observer-cli case list

# 匹配当前状态
./target/release/nuts-observer-cli case match --target pod:nginx
```

内置案例包括：
- `cpu_throttle`: CPU Throttle 导致服务延迟升高
- `memory_pressure`: 内存压力触发 OOM Kill
- `network_latency_spike`: 网络延迟 P99 异常升高
- `disk_io_latency`: 磁盘 IO 延迟导致应用卡顿

## 📈 监控与运维

### 健康检查

```bash
# 基础健康检查
curl http://localhost:8080/health

# 详细健康状态
curl http://localhost:8080/health/ready

# 统计信息
curl http://localhost:8080/health/stats
```

### 日志查看

```bash
# 查看服务日志
sudo journalctl -u nuts-observer -f

# 查看采集守护进程日志
sudo journalctl -u nuts-collector-daemon -f

# 查看结构化诊断日志
tail -f /var/log/nuts/diagnostics.log
```

## 🐛 故障排除

### 常见问题

1. **权限不足**
   ```bash
   # 检查 bpftrace 权限
   sudo bpftrace -l 'tracepoint:syscalls:sys_enter_*' | head -5
   
   # 检查 capabilities
   sudo getcap /usr/local/bin/nuts-collector-daemon
   ```

2. **bpftrace 脚本加载失败**
   ```bash
   # 验证脚本语法
   sudo bpftrace -d /usr/share/nuts/bpftrace/network/tcp_connect.bt
   
   # 检查内核版本
   uname -r
   ```

3. **服务无法启动**
   ```bash
   # 检查端口占用
   sudo lsof -i :8080
   
   # 查看服务状态
   sudo systemctl status nuts-observer
   sudo journalctl -u nuts-observer -n 50
   ```

### 调试模式

```bash
# 启用调试日志
export RUST_LOG=debug
./target/release/nuts-observer

# 或修改配置文件
log_level: "debug"
```

## 🤝 贡献指南

欢迎提交 Issue 和 Pull Request！

1. Fork 项目
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add some amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 打开 Pull Request

### 开发环境设置

```bash
# 安装开发依赖
cargo install cargo-watch

# 运行测试
cargo test

# 开发模式运行
cargo watch -x run

# 代码格式化
cargo fmt

# 代码检查
cargo clippy
```

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情。

## 📞 联系方式

- **项目主页**: [https://github.com/your-username/nuts-observer](https://github.com/your-username/nuts-observer)
- **问题反馈**: [GitHub Issues](https://github.com/your-username/nuts-observer/issues)
- **文档**: [项目 Wiki](https://github.com/your-username/nuts-observer/wiki)

## 🙏 致谢

感谢以下开源项目：
- [Rust](https://www.rust-lang.org/) - 系统编程语言
- [Tokio](https://tokio.rs/) - 异步运行时
- [Axum](https://github.com/tokio-rs/axum) - Web 框架
- [bpftrace](https://github.com/iovisor/bpftrace) - eBPF 追踪工具
- [openEuler](https://www.openeuler.org/) - 开源操作系统

---

**Nuts Observer** - 让容器故障诊断更智能、更高效！ 🚀