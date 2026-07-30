# RPM 打包说明

## 产出包

| RPM 包名 | 内容 | 运行用户 |
|---|---|---|
| `podflow` | 主服务 + CLI + adapters + bpftrace 脚本 + NRI 插件配置 | `podflow` |
| `podflow-collector` | 特权 eBPF 采集守护进程 | `root` |

## 构建环境要求

```bash
# RHEL/CentOS 8/9
dnf install -y rpm-build rpmdevtools rust cargo protobuf-compiler gcc make openssl-devel

# 安装 Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

## 快速构建

```bash
# 方式 1: 从源码完整构建 SRPM + RPM
cd rpm
make rpm

# 方式 2: 使用预编译二进制快速构建（跳过编译）
cd rpm
make rpm-fast

# 方式 3: 仅构建 SRPM（源码包）
make srpm
```

## 安装

```bash
# 本地安装
cd rpm
make install

# 或手动安装
rpm -ivh podflow-0.1.0-1.el9.x86_64.rpm
rpm -ivh podflow-collector-0.1.0-1.el9.x86_64.rpm
```

## 安装后的目录布局

```
/usr/bin/
├── podflow              # 主服务二进制
├── podflow-cli          # CLI 工具
├── podflow-adapters              # 适配器 CLI
└── podflow-collector      # 采集守护进程

/etc/podflow/
└── config.yaml                # 主配置文件

/etc/nri/conf.d/
├── podflow-nri.toml     # NRI 插件配置
└── 99-podflow-nri.conf  # containerd NRI drop-in

/usr/share/podflow/bpftrace/
├── templates/                 # bpftrace 模板
│   ├── cgroup_contention.bt
│   ├── fs_stall.bt
│   ├── network_latency.bt
│   ├── oom_events.bt
│   ├── softirq_contention.bt
│   └── syscall_latency.bt
├── adapters/                  # 适配器配置
├── block_io/                  # 块设备 I/O 脚本
└── network/                   # 网络脚本

/var/lib/podflow/        # 持久化数据
/var/log/podflow/        # 日志
/run/podflow/            # 运行时 socket

/usr/lib/systemd/system/
├── podflow.service
└── podflow-collector.service
```

## 启动服务

```bash
# 先启动 collector daemon（需要 root 权限加载 eBPF）
systemctl start podflow-collector

# 再启动主服务
systemctl start podflow

# 设置开机自启
systemctl enable podflow-collector podflow
```

## 防火墙

主服务监听 8080 端口（HTTP API），如需外部访问：

```bash
firewall-cmd --add-port=8080/tcp --permanent
firewall-cmd --reload
```

## 卸载

```bash
make uninstall
# 或
rpm -e podflow podflow-collector
```

注意：卸载不会删除 `/var/lib/podflow/` 下的持久化数据，需手动清理。