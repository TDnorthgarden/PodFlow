# bpftrace 最小证据链路（用于第 1 周端到端 PoC）

> 目标：选 2~3 条“最小链路”，把采集 -> 证据 schema -> 证据聚合 -> 诊断（最简规则）-> 结构化输出跑通。

## 1. 最小链路选择建议
建议从以下优先级中选 2~3 条：
1) 网络链路：连通性/丢包/延迟的最小指标与事件；
2) 块 I/O 链路：I/O 延迟与吞吐/队列的最小指标；
3) 文件系统或 syscall 链路（二选一即可）：先保证证据 schema 字段对齐。

本规划默认第 1 周做：`network + block_io`（加一个可选 `oom` 或 `syscall`）。

## 2. bpftrace 输出 -> Evidence Schema 对齐（通用映射）
采集器（Collector）把 bpftrace 原始采样结果转换为：
- `schema_version`
- `task_id`
- `evidence_id`（用于被诊断结果稳定引用）
- `evidence_type`
- `time_window`（epoch ms）
- `scope`（由 NRI 映射提供，或 pid/cgroup 回退；包含 `scope_key`）
- `collection`（包含 `collection_id / collection_status / probe_id / errors`）
- `metric_summary / events_topology / top_calls（按能力取舍）`
- `attribution`（包含 `status / confidence / source / mapping_version`）

## 3. 网络最小证据链路（建议字段清单）
Evidence type：`network`
- 归属/目标：
  - 归属使用 `scope_key`（pod/cgroup 哈希），
  - 探测目标标识可选填 `scope.network_target`（例如 `dst_ip/dst_port/protocol`），不参与归属键 hash。
  - 连通性探测口径建议为 `TCP connect`（默认 `protocol=tcp`）。
  - 按本项目选择的底层口径：成功需要在 `time_window` 内观测到 TCP 三次握手完成（`client(SYN)` -> `server(SYN-ACK)` -> `client(ACK)`）。
- 指标（metric_summary）：
  - `connectivity_success_rate`（可选：连通性成功率，0~1）
  - `loss_rate`（丢包率，0~1；若你们未采集 packet-loss 证据，该字段应缺失）
  - `latency_p50_ms`（可选）
  - `latency_p90_ms`（可选）
  - `latency_p99_ms`（可选；建议用于 `p99` 阈值触发）
  - `jitter_ms`（可选）
- 事件（events_topology）：
  - `connectivity_failure_burst`（可选：连通性失败突增/持续失败片段）
    - 建议填充（若实现成本允许）：`details.failure_stage` 用于区分失败发生在握手阶段（`SYN_TIMEOUT` / `RST_BEFORE_SYNACK` / `SYNACK_RECEIVED_ACK_TIMEOUT` / `OTHER`）；其中 `RST_BEFORE_SYNACK` 判定口径为“任意一侧观察到 RST/拒绝，且在 SYN-ACK 之前”
    - 事件时间语义：当 `details.from_time_ms` 提供时，`events_topology[].event_time_ms = details.from_time_ms`
    - 若 `failure_stage=SYN_TIMEOUT`：
      - 可选填：`details.synack_missing_interval{from_time_ms,to_time_ms}` 用于区分“缺失 SYN-ACK”发生的时间段
      - 若实现成本允许，可进一步填：`details.synack_missing_attempts[]`，每项包含 `attempt_index/from_time_ms/to_time_ms` 用于区分“缺失发生在哪次探测尝试”
        - 建议：同一 `network` evidence 内按 `SYN` 观测/发送时间升序分配 `attempt_index`，从 0 开始（保证确定性）
        - 建议：`from_time_ms` 使用该尝试开始时观察到的 client SYN 时间
        - `to_time_ms` 默认建议使用“下一次尝试的 client SYN 观测时间”，若没有下一次则为 `time_window.end_time_ms`
        - （已固化最优解口径）：to_time_ms 不使用 attempt_end，统一按“下一次 client SYN 观测”作为边界
    - 若 `failure_stage=SYNACK_RECEIVED_ACK_TIMEOUT`（见到 SYN-ACK 但未完成 ACK）：
      - 可选填：`details.ack_missing_interval{from_time_ms,to_time_ms}`
      - 若实现成本允许，可进一步填：`details.ack_missing_attempts[]`，每项包含 `attempt_index/from_time_ms/to_time_ms`
        - 其中 `attempt_index` 与 `SYN_TIMEOUT` 分段使用同一证据内 attempt_index 定义
        - `from_time_ms` 建议取该 attempt 对应的 SYN-ACK 首次观测时间；`to_time_ms` 仍按“下一次 client SYN 观测”作为边界
    - 若 `failure_stage=RST_BEFORE_SYNACK`（握手过程中任意一侧先看到 RST/拒绝，且在 SYN-ACK 之前）：
      - 可选填：`details.rst_observed_attempts[]`，每项包含 `attempt_index` 与 `rst_time_ms`
  - `packet_loss_burst`：给出 burst 起始时间 `event_time_ms`（当 `details.from_time_ms` 提供时需与之保持一致）
    - 若能采集/用户请求则在 `details` 中填充：`from_time_ms`、`to_time_ms`、`loss_rate_during`，以及可选的 `total_probe_count_during`/`loss_count_during`
  - `latency_spike`：延迟突增起点
    - 若实现成本允许，可在 `details` 中填充 `spike_window{start_time_ms,end_time_ms}` 用于解释 delta_p99_ms

## 4. 块 I/O 最小证据链路（建议字段清单）
Evidence type：`block_io`
- 指标（metric_summary）：
  - `io_latency_p50_ms`（可选）
  - `io_latency_p90_ms`（可选）
  - `io_latency_p99_ms`（可选；建议用于 `p99` 阈值触发）
  - `throughput_bytes_per_s`（可选，若能稳定采到）
  - `queue_depth` 或等价指标（可选）
- 事件（events_topology）：
  - `io_latency_spike`：I/O 延迟突增起点
  - `io_timeout`（可选，若能采到）

## 5. 可选：OOM 联动最小证据链路
Evidence type：`oom`
- 事件：
  - `oom_kill`：event_time_ms、触发对象的 cgroup/pod 归属 key

## 6. 文件系统卡顿最小证据链路（建议用于第 2~4 周）
Evidence type：`fs_stall`
- 指标（metric_summary）：
  - `fs_stall_p50_ms`（可选）
  - `fs_stall_p90_ms`（可选）
  - `fs_stall_p99_ms`（可选；建议用于 `p99` 阈值触发）
  - `fs_ops_per_s`（可选）
- 事件（events_topology）：
  - `fs_stall_spike`
    - `event_time_ms` 表示 spike 起点（与 spike_window.start_time_ms 一致，若提供）
    - details 可选：`latency_ms_at_spike`、`delta_p99_ms`、`spike_window`、`baseline_window`

## 7. 系统调用耗时最小证据链路（建议用于第 2~4 周）
Evidence type：`syscall_latency`
- 指标（metric_summary）：
  - `syscall_latency_p50_ms`（可选）
  - `syscall_latency_p90_ms`（可选）
  - `syscall_latency_p99_ms`（可选；建议用于 `p99` 阈值触发）
  - `syscall_ops_per_s`（可选）
- 证据聚合（top_calls，可选/建议）：
  - `top_calls.by_call[]` 至少包含 `call_name` 与 `count`；可选填 `p99_latency_ms`
- 事件（events_topology）：
  - `syscall_latency_spike`
    - details 可选：`top_call_name`、`delta_p99_ms`、`spike_window`、`baseline_window`

## 8. cgroup 资源争抢最小证据链路（建议用于第 4~6 周）
Evidence type：`cgroup_compete`
- 指标（metric_summary）：
  - `cpu_throttle_ratio`（可选）
  - `io_throttle_ratio`（可选）
  - `memory_pressure_index`（可选）
  - `contention_score`（可选）
- 事件（events_topology）：
  - `cgroup_throttle_burst`
    - details 可选：`resource_type`（`cpu`|`memory`|`io`）、`throttle_ratio_during`

## 9. 运行与验收要点
- 验收指标（第 1 周）：
  - 触发后能采集并输出至少一条 `network` 或 `block_io` Evidence；
  - Evidence 字段完整性符合 `docs/02_schemas.md`；
  - 输出器能落地结构化日志（可读 + 可解析 JSON）。

## 10. 采集脚本组织建议
- `scripts/bpftrace/network/*.bt`：网络相关探针；
- `scripts/bpftrace/block_io/*.bt`：块设备 I/O 相关探针；
- `scripts/bpftrace/fs_stall/*.bt`：文件系统卡顿相关探针；
- `scripts/bpftrace/syscall_latency/*.bt`：系统调用耗时相关探针；
- `scripts/bpftrace/cgroup_compete/*.bt`：cgroup 争抢相关探针；
- `collector/`：负责启动/停止、采样强度配置、归属映射注入、字段化输出。

