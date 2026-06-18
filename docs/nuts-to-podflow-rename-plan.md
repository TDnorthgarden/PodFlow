# 「nuts」→「podflow」命名变更规划

## 背景

项目仓库已更名为 **PodFlow**（`github.com/TDnorthgarden/PodFlow`），但内部二进制、crate、模块、路径、环境变量等仍全量使用「nuts」命名。需要评估变更必要性并制定分期执行计划。

## 现状统计

| 维度 | 数量 | 示例 |
|---|---|---|
| Cargo 二进制目标 | 4 | `nuts-observer`, `nuts-observer-cli`, `nuts-collector-daemon`, `nuts-adapters` |
| Cargo crate 名 | 1 | `nuts-observer` / `nuts_observer` |
| Protobuf package | 1 | `nuts.collector` |
| 含 "nuts" 的文件名 | 8 | `nuts_observer_cli.rs`, `nuts-observer-nri.toml` 等 |
| Rust 模块引用 | ~70+ 源文件 | `use nuts_observer::*` |
| 文档文件 | 17 | 所有 `docs/*.md` + `README.md` |
| 环境变量 | 12 | `NUTS_LOG_LEVEL`, `NUTS_NRI_SOCKET_PATH` 等 |
| 文件系统路径 | 10+ | `/etc/nuts/`, `/var/log/nuts/`, `/run/nuts/`, `/root/nuts/` 等 |
| Docker 镜像 | 5 | `nuts-observer:latest`, `nuts-collector-daemon:latest` 等 |
| K8s 资源 | 5+ | `nuts-observer-nri` DaemonSet, ServiceAccount, ConfigMap 等 |
| systemd unit | 2 | `nuts-observer.service`, `nuts-collector-daemon.service` |
| Linux 用户/组 | 1 | `nuts` |
| 测试脚本 | 6 | `NUTS_DIR="/root/nuts"` |

## 变更必要性

**建议变更**，理由：
1. 对外不一致：仓库叫 PodFlow，二进制叫 `nuts-observer`，增加认知负担
2. 时机合适：当前版本 v0.1.0，用户基数小，变更成本可控

## 命名映射

| 旧名 | 新名 |
|---|---|
| `nuts-observer` | `podflow` |
| `nuts-observer-cli` | `podflow-cli` |
| `nuts-collector-daemon` | `podflow-collector` |
| `nuts-adapters` | `podflow-adapters` |
| `nuts_observer` (lib crate) | `podflow` |
| `nuts.collector` (proto) | `podflow.collector` |
| `NUTS_*` (环境变量) | `PODFLOW_*` |
| `/etc/nuts/` | `/etc/podflow/` |
| `/var/log/nuts/` | `/var/log/podflow/` |
| `/var/lib/nuts/` | `/var/lib/podflow/` |
| `/run/nuts/` | `/run/podflow/` |
| `/root/nuts/` | `/root/podflow/` |
| `nuts` (Linux user/group) | `podflow` |

## 分期方案

### Phase 1：内部源码（优先级最高，影响开发者）

**范围：**
- [ ] `Cargo.toml`：修改 package name、所有 binary name、lib name
- [ ] `Cargo.lock`：随 `Cargo.toml` 自动更新
- [ ] `proto/collector.proto`：修改 `package` 声明
- [ ] `src/proto/nuts.collector.rs` → 重命名为 `src/proto/podflow.collector.rs`，内容同步更新
- [ ] 所有 Rust 源文件中的 `use nuts_observer::*` → `use podflow::*`
- [ ] 所有 Rust 源文件中的 `crate::` 路径引用检查（通常无需改动，但需确认）
- [ ] `src/bin/nuts_observer_cli.rs` → `src/bin/podflow_cli.rs`
- [ ] `src/bin/nuts_adapters_cli.rs` → `src/bin/podflow_adapters_cli.rs`
- [ ] 编译验证：`cargo build` 和 `cargo test` 通过

**不涉及：** 外部文档、部署配置、路径、环境变量

---

### Phase 2：文档与环境变量（发布前完成）

**范围：**
- [ ] `README.md`：全局替换项目名、二进制名、路径、镜像名
- [ ] `docs/*.md`（17 个文件）：逐个审查替换
- [ ] `config.yaml`：注释、路径、socket 名
- [ ] `cases/cases.yaml`：注释
- [ ] 环境变量前缀全局替换：`NUTS_` → `PODFLOW_`
  - `deploy/kubernetes/nuts-observer-nri-daemonset.yaml`
  - 所有文档中的环境变量说明

**不涉及：** 文件系统路径、系统服务、Docker 镜像

---

### Phase 3：部署基础设施（需迁移指南）

**范围：**
- [ ] 文件系统路径：
  - `deploy/kubernetes/nuts-observer-nri-daemonset.yaml`
  - `systemd/nuts-observer.service`
  - `systemd/nuts-collector-daemon.service`
  - 所有文档和测试脚本中的路径引用
- [ ] 文件名重命名：
  - `deploy/nri/nuts-observer-nri.toml` → `deploy/nri/podflow-nri.toml`
  - `deploy/nri/99-nuts-observer-nri.conf` → `deploy/nri/99-podflow-nri.conf`
  - `deploy/kubernetes/nuts-observer-nri-daemonset.yaml` → `deploy/kubernetes/podflow-nri-daemonset.yaml`
  - `systemd/nuts-observer.service` → `systemd/podflow.service`
  - `systemd/nuts-collector-daemon.service` → `systemd/podflow-collector.service`
- [ ] K8s 资源名：DaemonSet、ServiceAccount、ConfigMap 名称
- [ ] Docker 镜像名：`nuts-observer` → `podflow`
- [ ] Linux 用户/组：`nuts` → `podflow`
- [ ] 测试脚本：`NUTS_DIR` → `PODFLOW_DIR` 等
- [ ] 编写迁移指南：已有部署如何从 nuts 迁移到 podflow

---

## 风险评估

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| Rust crate 改名后外部依赖无法引用 | 如被其他项目引用则 break | Phase 1 先确认无外部引用者 |
| K8s 资源改名需要重建 | 滚动更新时短暂中断 | Phase 3 提供零停机迁移步骤 |
| 环境变量改名影响已有配置 | 已部署实例的 env 失效 | 过渡期可同时兼容新旧变量名 |
| Docker 镜像改名 | 镜像仓库需同步更新 | CI/CD pipeline 同步调整 |
| Protobuf package 改名 | 影响序列化兼容性 | 确认无持久化数据依赖 package 名 |

## 执行建议

- Phase 1 可立即执行，纯内部变动，不影响用户
- Phase 2/3 在 v0.2.0 或正式发布前完成
- Phase 3 执行时需编写 `MIGRATION.md` 提供迁移指南
- 过渡期可在 Phase 3 阶段保留对旧路径/变量名的兼容（废弃警告）