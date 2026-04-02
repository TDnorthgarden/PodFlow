# 第 1~4 周 PoC 验收清单（按 Evidence 类型）

> 目的：让并行开发不返工。建议每个 PoC 均用同一套“必备字段 + 可选字段 + 失败降级检查”验收。

## 0. 全局必备（所有 evidence_type 通用）
每次触发生成至少一个 `Evidence` 对象，必须满足：
1) 结构可解析：JSON 字段名与类型符合 `docs/02_schemas.md` 的 `Evidence v0.2`。
2) ID/引用一致：
   - `Evidence.evidence_id` 必填；
   - 若生成 `DiagnosisResult`，其 `evidence_refs[].evidence_id` 必能解析到本次任务内的 Evidence。
3) 时间与范围：
   - `time_window.start_time_ms/end_time_ms` 为 epoch ms；
   - Evidence 归属范围字段（`scope_key`）由 `docs/03_nri_mapping_spec.md` 规则生成。
4) 采集状态与降级：
   - `collection.collection_status` 为 `success|partial|failed`；
   - 若关键指标未请求或不可用，则 `metric_summary` 仅包含已采集/已请求字段，禁止凭空补值；
   - 生成 `DiagnosisResult.status` 与引用可解析性/字段缺失情况匹配（`done|partial|failed`）。

## 0.1 更细粒度的探针->字段验收
如果需要把 bpftrace raw 事件与 collector 字段生成做逐条对齐验收，可参考 `docs/09_probe_field_validation.md`。

## 1. `network`（建议第 1 周）
至少生成以下内容：
- `Evidence.evidence_type = network`
- `metric_summary`
  - 必备（按用户请求）：`connectivity_success_rate`（TCP 三次握手完成序列成功判定）
  - `latency_p50_ms/p90_ms/p99_ms/latency_avg_ms/jitter_ms`：按用户请求出现/缺失允许
- `events_topology`
  - 可选：`connectivity_failure_burst`，并可填 `failure_stage` 与握手阶段细分字段
  - 若 `packet_loss_burst` 出现：`details.from_time_ms` 提供时，事件 `event_time_ms` 与之保持一致
- `scope.network_target`：如果触发时提供了目标，建议填充用于可读性；不参与 `scope_key` hash

验收重点：
- `failure_stage` 枚举值与含义正确；
- 当 `failure_stage=SYN_TIMEOUT` 时，`synack_missing_attempts[].attempt_index` 按同一 evidence 内 SYN 观测时间升序从 0 开始且稳定。

## 2. `block_io`（建议第 1 周）
至少生成以下内容：
- `Evidence.evidence_type = block_io`
- `metric_summary`
  - `io_latency_p99_ms`（建议用于规则触发；可缺失但需符合“按请求/可用”）
  - `io_latency_p50_ms/p90_ms/throughput_bytes_per_s/queue_depth/timeout_count`：按用户请求出现/缺失允许
- `events_topology`
  - `io_latency_spike`（details 可选：`io_latency_ms_at_spike`、`delta_p99_ms` 等）
  - `io_timeout`（details 可选）

## 3. `fs_stall`（建议第 2~4 周）
至少生成以下内容：
- `Evidence.evidence_type = fs_stall`
- `metric_summary`
  - `fs_stall_p99_ms`（建议用于规则触发；可缺失但需符合“按请求/可用”）
- `events_topology`
  - `fs_stall_spike`（可选 details：`latency_ms_at_spike`、`delta_p99_ms`、`spike_window`、`baseline_window`）

关联验收：
- 若诊断引擎同时有 `block_io`/`syscall_latency` evidence，应能产生“文件系统卡顿与 I/O/调用耗时”的关联结论（允许置信度降级）。

## 4. `syscall_latency`（建议第 2~4 周）
至少生成以下内容：
- `Evidence.evidence_type = syscall_latency`
- `metric_summary`
  - `syscall_latency_p99_ms`（建议用于规则触发；可缺失但需符合“按请求/可用”）
- `top_calls`（建议用于规则判断）
  - `top_calls.by_call[]` 至少包含 `call_name` 与 `count`；可选 `p99_latency_ms`
- `events_topology`
  - `syscall_latency_spike`（details 可选：`top_call_name`、`delta_p99_ms` 等）

关联验收：
- 当 evidence 包含 `fs_stall` 或 `network` 时，规则引擎应能产出“调用类型变化解释卡顿”的结论（允许只输出部分结论）。

## 5. `cgroup_compete`（建议第 4~6 周）
至少生成以下内容：
- `Evidence.evidence_type = cgroup_compete`
- `metric_summary`
  - `contention_score`（建议）
  - 其它维度（cpu_throttle_ratio/io_throttle_ratio/memory_pressure_index）按请求出现/缺失允许
- `events_topology`
  - `cgroup_throttle_burst`（details 可选：`resource_type`、`throttle_ratio_during`）

关联验收：
- 当触发了资源争抢事件，规则引擎应能把网络/IO/FS 的延迟异常解释到“争抢背景”（允许降级）。

## 6. `oom`（可选：第 2~3 周联动）
至少生成以下内容：
- `Evidence.evidence_type = oom`
- `events_topology`
  - `oom_kill`（event_time_ms 与可选 details）

联动验收：
- 若触发器支持 OOM 异常联动：应能扩大诊断时间窗或增强关联证据收集（允许部分证据缺失但结构可解析）。

