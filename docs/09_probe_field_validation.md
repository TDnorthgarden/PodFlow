# 探针 raw -> Evidence 字段验证（PoC 实现验收）

> 目的：让 bpftrace/collector 并行开发能做“最小验收”。每个 evidence_type 都列出：
> 1) collector 需要的最小 raw 事件（来自 `docs/08_collector_bpftrace_to_fields.md`）
> 2) collector 必须生成的最小 Evidence 字段（来自 `docs/02_schemas.md`）
> 3) 失败/部分成功时的预期表现（`DiagnosisResult.status` 与字段缺失规则）

## 0. 全局校验规则（建议固定在测试脚本/验收用例里）
1) 输出可解析：生成的 Evidence JSON 必须能被 Schema 校验器/解析器解析通过。
2) 引用一致：若生成 DiagnosisResult，则 `evidence_refs[].evidence_id` 必须存在于本次任务输出的 Evidence 列表中。
3) 字段稀疏：当用户未请求或采集器无法计算时：
   - `metric_summary` 不应凭空补值；
   - 允许字段缺失或出现 `attribution.status=unknown/pid_cgroup_fallback`。
4) 部分成功：当关键分位/指标缺失导致规则只能给低置信结论时，DiagnosisResult.status 应为 `partial`。

## 1. `network` PoC 验证
### 1.1 collector 需要的最小 raw 事件
参考 `docs/08_collector_bpftrace_to_fields.md` 的 `network -> 字段映射（TCP connect 口径）`：
1) `tcp_attempt_stage`（含 attempt_index/stage/client SYN/server SYN-ACK/client ACK 观测时间）
2) `tcp_attempt_rst_or_reject`（含 attempt_index、rst_time_ms；用于 RST_BEFORE_SYNACK 细分）
3) `tcp_attempt_timeout`（含 attempt_index、timeout_time_ms；用于 SYN_TIMEOUT 细分）

### 1.2 必须生成的最小 Evidence 字段
参考 `docs/07_poc_checklist.md`：
1) `Evidence.evidence_type=network`
2) `metric_summary.connectivity_success_rate`
3) `metric_summary.latency_pXX_ms` / `latency_avg_ms`（按 requested_metrics_by_type 出现/缺失允许）
4) `jitter_ms`（当依赖字段可用时出现/缺失允许）
5) `events_topology.connectivity_failure_burst`（可选，但若出现必须按 details.failure_stage 与时间语义一致）

### 1.3 关键验收点
1) 当 `failure_stage=SYN_TIMEOUT` 时：
   - `synack_missing_attempts[].attempt_index`：同一 evidence 内按 SYN 观测时间升序，从 0 起稳定分配；
   - `synack_missing_attempts[].from_time_ms/to_time_ms`：按最优解边界定义一致；
   - `events_topology[].event_time_ms`：当 details.from_time_ms 存在时与之匹配。
2) 当未请求 latency 指标：不得生成 latency 分位字段；DiagnosisResult.status 允许为 `partial`（仅当关键结论确实依赖缺失指标）。

## 2. `block_io` PoC 验证
### 2.1 collector 需要的最小 raw 事件
参考 `docs/08_collector_bpftrace_to_fields.md` 的 `evidence_type=block_io -> 字段映射`：
1) `block_io_complete`（issue_time_ms/complete_time_ms/bytes/device/pid/cgroup_id 等可关联字段）
（如你们有 io_timeout 相关事件，则补充超时判定 raw）

### 2.2 必须生成的最小 Evidence 字段
1) `Evidence.evidence_type=block_io`
2) `metric_summary.io_latency_p99_ms`（建议用于规则触发；按请求出现/缺失允许）
3) `metric_summary.io_latency_p50/p90`、`throughput_bytes_per_s`、`queue_depth`、`timeout_count`（按请求出现/缺失允许）
4) `events_topology.io_latency_spike`（如你们实现 spike 事件；details 可选）
5) `events_topology.io_timeout`（如你们实现 timeout 事件）

### 2.3 关键验收点
1) latency_pXX_ms 的计算必须只基于完成的 IO 样本（文档已约束）。
2) throughput/queue_depth 若无法计算：允许缺失，但不得以“猜测值”填充。

## 3. `fs_stall` PoC 验证
### 3.1 collector 需要的最小 raw 事件
参考 `docs/08_collector_bpftrace_to_fields.md` 的 `evidence_type=fs_stall -> 字段映射`：
1) 最小可实现：能够把“文件相关进程/IO”聚合到同一归属 scope 的 IO 延迟样本
（若你们已有更精确的文件系统层观测，可在 artifacts 中记录实现增强点）

### 3.2 必须生成的最小 Evidence 字段
1) `Evidence.evidence_type=fs_stall`
2) `metric_summary.fs_stall_p99_ms`（建议）
3) `events_topology.fs_stall_spike`（可选 details）

### 3.3 关键验收点
1) 文件系统归因策略与 `scope_key`：必须能落到具体 Pod/cgroup（或 `attribution.status=unknown`）
2) 当 fs_stall 指标缺失时：诊断引擎只输出非 fs 结论或降级为 `partial`。

## 4. `syscall_latency` PoC 验证
### 4.1 collector 需要的最小 raw 事件
参考 `docs/08_collector_bpftrace_to_fields.md` 的 `evidence_type=syscall_latency -> 字段映射`：
1) syscall enter/exit 或等价 tracepoint（至少能算 latency 样本 + syscall_name + pid/cgroup）

### 4.2 必须生成的最小 Evidence 字段
1) `Evidence.evidence_type=syscall_latency`
2) `metric_summary.syscall_latency_p99_ms`（建议）
3) `top_calls.by_call[]`：`call_name` 与 `count`；可选 `p99_latency_ms`
4) `events_topology.syscall_latency_spike`（可选）

## 5. `cgroup_compete` PoC 验证
### 5.1 collector 需要的最小 raw 事件
参考 `docs/08_collector_bpftrace_to_fields.md` 的 `evidence_type=cgroup_compete -> 字段映射`：
1) CPU throttling / memory pressure / IO throttling 的任一或部分可计算指标（最少能稳定输出一个维度）

### 5.2 必须生成的最小 Evidence 字段
1) `Evidence.evidence_type=cgroup_compete`
2) `metric_summary.contention_score` 或至少一个维度比率（cpu_throttle_ratio/io_throttle_ratio/memory_pressure_index）
3) `events_topology.cgroup_throttle_burst`（可选 details）

## 6. 部分成功与 DiagnosisResult.status（实现验收动作）
当出现任一情况时，允许（且建议）把 DiagnosisResult.status 置为 `partial`：
1) evidence_refs 可解析，但关键指标缺失导致结论只能部分生成；
2) `attribution.status=unknown` 或回退来源较多，规则需要低置信度输出；
3) 采集只返回 `collection_status=partial` 或关键分位字段无法计算。

