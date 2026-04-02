# Collector：bpftrace 采集到字段契约的映射（v0.2）

> 目的：让并行开发能够直接落地“探针产物 -> Evidence 字段”。本文件不强绑定某个 bpftrace tracepoint/kprobe 名称，
> 而是规定“探针必须产生哪些事件/原始量”，以及 Collector 如何聚合成 `docs/02_schemas.md` 中的字段。

## 1. 总体原则（硬约束）
1. `Evidence` 的粒度：`task_id + evidence_type + scope_key + time_window`。
2. 同一 `Evidence` 里可能有多个探针输出：Collector 必须合并到同一个 `evidence_id`，并把原始产物记录在 `artifacts[]`（可选，但建议）。
3. 只输出用户请求/采集到的指标：`metric_summary` 允许稀疏（不出现=不可用或未请求），禁止凭空补值。
4. `events_topology[].event_time_ms` 的语义：
   - 当 `details.from_time_ms` 存在时，`event_time_ms = from_time_ms`
   - `latency_spike` 的 `event_time_ms` 表示 spike 起点（与 `spike_window.start_time_ms` 一致，若提供）

## 2. Collector 统一的统计口径（用于 delta / 分位）
为了让实现默认一致，本文件给出确定性默认策略（如果你的实现有更好的方案，可在 artifacts 中记录规则版本，并保证规则库一致消费）。

### 2.1 spike_window / baseline_window 默认切分
对于需要 `delta_p99_ms` 的事件（`*_spike`）：
- `spike_window`: `[time_window.start_time_ms + (time_window.end_time_ms-time_window.start_time_ms)/2, time_window.end_time_ms]`
- `baseline_window`: `[time_window.start_time_ms, time_window.start_time_ms + (time_window.end_time_ms-time_window.start_time_ms)/2]`

如需自定义窗口，允许在事件 `details` 中提供 `spike_window` / `baseline_window` 覆盖默认策略。

### 2.2 latency 分布的统计集合
- `network`：`latency_pXX_ms` 基于成功握手的样本（有 SYN 和 ACK 两端观测），失败样本仅用于 success_rate 与 failure_stage / missing 事件细分。
- `block_io` / `fs_stall`：基于真实完成的 IO 样本；超时/未完成样本不应进入延迟分位（除非你们在实现里定义了替代口径，并在 artifacts 中写清）。

## 3. evidence 的构造步骤（Collector 流程）
对于一次诊断任务（task）：
1) 从触发器获得 `time_window`、`scope(target)`、`requested_evidence_types` 与 `requested_metrics_by_type` / `requested_events_by_type`。
2) 查 `NRI Pod->cgroup/pid` 映射表，生成 `scope_key=sha256_hex(pod_uid+"|"+cgroup_id)`（见 `docs/03_nri_mapping_spec.md`）。
3) 启动/加载 bpftrace 探针，并采集 raw 事件流。
4) 将 raw 事件流聚合为若干 `Evidence`：
   - 按 evidence_type 切分
   - 按 `scope_key` 与 `time_window` 切分
   - 计算/生成 `metric_summary`、`events_topology[]`、（可选）`top_calls`、`attribution`
5) 生成 `evidence_id`：
   - `evidence_id = sha256(task_id + "|" + evidence_type + "|" + collection_id + "|" + scope_key)`
6) 将 `Evidence` 输出给诊断引擎；诊断结果再由 Publisher 输出。

## 4. evidence_type：network -> 字段映射（TCP connect 口径）

### 4.1 探针 raw 事件要求（Collector 需要的最小输入）
探针必须产生下面几类 raw 事件，并可带同一 `probe_collection_id` 用于归并。

1) `tcp_attempt_stage`（用于 attempt_index 与握手阶段判定）
   - 字段建议：
     - `attempt_index`（或能让 Collector 在同一 evidence 内推导）
     - `stage`：`CLIENT_SYN` | `SERVER_SYNACK` | `CLIENT_ACK`
     - `stage_time_ms`（epoch ms）
     - `success`（可选，由 Collector 通过 stage 是否齐全判定）
2) `tcp_attempt_rst_or_reject`
   - 字段建议：
     - `attempt_index`
     - `rst_time_ms`
     - `observed_side`（可选；client/server/any，用于解释）
3) `tcp_attempt_timeout`
   - 字段建议：
     - `attempt_index`
     - `timeout_time_ms`
4) （可选）`tcp_latency_sample`：若你们直接在探针端计算了握手耗时并输出，可跳过阶段拼接。

### 4.2 Collector -> Evidence 字段生成
1) `metric_summary.connectivity_success_rate`
   - `total_probe_count = number of attempts`
   - `success_count = attempts with CLIENT_SYN+SERVER_SYNACK+CLIENT_ACK observed`
   - `connectivity_success_rate = success_count / total_probe_count`
2) `metric_summary.latency_pXX_ms / latency_avg_ms`
   - 样本定义：每个成功 attempt 的 `handshake_latency_ms = CLIENT_ACK_time_ms - CLIENT_SYN_time_ms`
   - 分位计算：p50/p90/p99，均单位 ms
3) `metric_summary.jitter_ms`
   - `jitter_ms = latency_p90_ms - latency_p50_ms`（前提是这些字段被请求/可用）
4) `events_topology.connectivity_failure_burst`
   - burst 的触发由你们规则库/采样逻辑决定（可先用“failure_rate 在 spike_window 或持续窗口超过阈值”）。
   - 当生成 event：
     - `event_time_ms = details.from_time_ms`（当 from_time_ms 提供）
     - `details.failure_stage`：
       - `SYN_TIMEOUT`：仅观测到 CLIENT_SYN（SYNACK 未出现）
       - `RST_BEFORE_SYNACK`：任意一侧观测到 RST/拒绝且在 SYNACK 之前
       - `SYNACK_RECEIVED_ACK_TIMEOUT`：SYNACK 出现但 ACK 未完成
   - `synack_missing_interval`：
     - `from_time_ms`：第一个 SYN 观测时间
     - `to_time_ms`：最后一次失败 attempt 的结束边界（最优解：下一次 client SYN 观测时间；若不存在则 `time_window.end_time_ms`）
   - `synack_missing_attempts[]`：
     - attempt_index：按 evidence 内 SYN 观测时间升序分配，从 0 开始
     - `from_time_ms/to_time_ms` 按同一 attempt 的 SYN 观测与下一次 SYN 边界
   - `ack_missing_interval / ack_missing_attempts[]`：
     - 按 SYNACK 观测时间分段；to_time_ms 同样使用下一次 client SYN 边界
   - `rst_observed_attempts[]`：
     - `rst_time_ms`：该 attempt 中首次观测到 RST/拒绝的时间
5) `events_topology.packet_loss_burst`（如你们额外采集 packet loss）
   - 由探测统计得到 `loss_count_during / total_probe_count_during`
   - `details.loss_rate_during = loss_count_during / total_probe_count_during`

### 4.3 attribution
- `attribution.status`：
  - 若能从 NRI 得到 pod/cgroup 映射：`nri_mapped`
  - 仅能从 pid/cgroup 反查：`pid_cgroup_fallback`
  - 否则：`unknown`

## 5. evidence_type：block_io -> 字段映射（块设备 IO）

### 5.1 探针 raw 事件要求
1) `block_io_issue`（可选：记录 issue_time_ms 与请求大小/设备）
2) `block_io_complete`
   - 字段建议：
     - `io_key`：能将 issue 与 complete 关联的唯一键（sector+ts+pid 可用组合）
     - `complete_time_ms`
     - `issue_time_ms`
     - `device`（或可映射到 target）
     - `bytes`（用于吞吐）
     - `pid`/`cgroup_id`（用于归属）
     - `rw_type`（读/写，可选）
     - `status`（完成是否成功；超时/错误可选）

### 5.2 Collector -> Evidence 字段生成
1) `metric_summary.io_latency_p50/p90/p99`
   - `io_latency_ms = complete_time_ms - issue_time_ms`
2) `metric_summary.throughput_bytes_per_s`
   - 在 `time_window` 内：`sum(bytes_completed) / window_duration_s`
3) `metric_summary.queue_depth`
   - 若实现难度较高：PoC 可用 `in_flight_count` 的峰值或时间平均替代（需在 artifacts 记录定义）。
4) `metric_summary.io_ops_per_s`
   - `IO_ops / window_duration_s`
5) `metric_summary.timeout_count`
   - 仅当你们有超时判定 raw 事件时生成
6) `events_topology.io_latency_spike`
   - spike_window/base_window 默认切分（见 2.1）
   - `details.delta_p99_ms = p99(spike_window)-p99(baseline_window)`

## 6. evidence_type：fs_stall -> 字段映射（文件系统卡顿）

### 6.1 探针 raw 事件要求（最小可实现方案）
你们可以用“文件系统相关进程 + IO 延迟”形成最小 PoC（不需要完全精确到 VFS 层）：
1) 文件相关 syscall 观测（可选，但建议）：
   - `file_syscall_start`：`pid/cgroup_id` + `syscall_type` + `start_time_ms`
2) 文件相关 IO 完成观测：
   - `block_io_complete`（同 block_io，但 Collector 需要能判断该 IO 是否属于文件系统相关进程）
   - 判断方法建议（可选）：
     - 简化：以 pid/cgroup 维度归属 + 你们认为“文件系统卡顿对应的 IO 模式”
     - 或：与 syscall_latency 的 top_calls 进行关联（第二阶段再增强）

### 6.2 Collector -> Evidence 字段生成
1) `metric_summary.fs_stall_pXX_ms`
   - 基于“被认定为文件系统卡顿相关”的 IO 延迟分位
2) `events_topology.fs_stall_spike`
   - 同 network/block_io 的 spike delta 计算逻辑（p99 spike - p99 baseline）

## 7. evidence_type：syscall_latency -> 字段映射（系统调用耗时统计）
### 7.1 探针 raw 事件要求
1) `syscall_enter/exit`（或等价 tracepoint）
   - 字段建议：
     - `pid/cgroup_id`
     - `syscall_name`
     - `start_time_ms`、`end_time_ms`
     - `latency_us_or_ms`

### 7.2 Collector -> Evidence 字段生成
1) `metric_summary.syscall_latency_pXX_ms`
   - 对用户请求的归属 scope 内所有 syscall 样本计算分位
2) `top_calls.by_call[]`
   - `call_name` 与 `count`
   - 可选：`p99_latency_ms` 用于规则优先判断
3) `events_topology.syscall_latency_spike`
   - 对关键（或整体）分位计算 delta，并产生 spike event_time_ms = spike_window.start_time_ms

## 8. evidence_type：cgroup_compete -> 字段映射（资源争抢）

### 8.1 探针 raw 事件要求（最小 PoC）
1) CPU throttling 事件（或可计算的节流时间）
   - `cpu_throttle`：`cgroup_id`、`throttle_time_ms` 或开始/结束
2) IO throttling 事件（若能获得）
   - `io_throttle`：同上
3) memory pressure 指标（可用 PSI 类事件或等价）
   - `memory_pressure`：`cgroup_id`、`pressure_value` 或 begin/end

### 8.2 Collector -> Evidence 字段生成
1) `metric_summary.cpu_throttle_ratio / io_throttle_ratio`
   - `throttle_time / window_duration`
2) `metric_summary.memory_pressure_index / contention_score`
   - 由实现侧定义固定公式，并记录到 artifacts（规则库用同一口径）
3) `events_topology.cgroup_throttle_burst`
   - 当 throttling 在 spike_window 或连续窗口超过阈值生成
   - `details.resource_type` 与 `throttle_ratio_during` 填充

## 9. failure / partial 的产出约束（实现侧检查表）
对于每个 evidence_type 的 collector 输出：
1) `metric_summary` 只包含用户 requested 的字段或采集到的字段
2) 若关键归属缺失：`attribution.status` 为 `unknown/pid_cgroup_fallback`，并保证输出仍可解析
3) 若分位所需样本不足：不生成该分位字段，Diagnosis 进入 `partial`

