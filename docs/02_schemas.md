# 结构化 Schema（证据与诊断结果）

> 说明：本文件给出可落地的 `v0.2` 版本，用于并行开发与后续对接（NRI / 告警平台 / AI）。

## 1. 设计目标（为什么要改 v0.1）
1) 引用一致性：`DiagnosisResult` 必须能稳定引用 `Evidence`（`evidence_id` 必填 + 生成规则明确）。
2) 归属可解释：NRI 映射 / pid/cgroup 回退 / unknown 必须可区分，并带 `confidence`。
3) 单位与时间统一：所有时间使用 epoch ms；数值字段给出单位约定。
4) 可观测与降级：采集失败/部分成功不会导致输出不可解析；AI/告警失败不会破坏核心诊断结果。

## 2. Evidence（证据字段）Schema v0.2

### 2.1 顶层字段（必填）
- `schema_version`: string（例如 `"evidence.v0.2"`）
- `task_id`: string
- `evidence_id`: string（必填；用于被诊断结果稳定引用）
- `evidence_type`: string（例如：`network`|`block_io`|`fs_stall`|`syscall_latency`|`cgroup_compete`|`oom`）
- `collection`: object
  - `collection_id`: string（本次采集的唯一标识）
  - `collection_status`: string（`success`|`partial`|`failed`）
  - `probe_id`: string（bpftrace 脚本/探针标识）
  - `errors`: array（可选）
    - 每项：`code`: string、`message`: string、`retryable`: boolean（可选）、`detail`: object（可选）
- `time_window`: object
  - `start_time_ms`: number（epoch ms）
  - `end_time_ms`: number（epoch ms）
  - `collection_interval_ms`: number（可选）
- `scope`: object（归属范围，尽量与 NRI mapping 对齐）
  - `pod`: object（可选；归属不确定时允许缺失或为空）
    - `uid`: string（可选）
    - `name`: string（可选）
    - `namespace`: string（可选）
  - `container_id`: string（可选）
  - `cgroup_id`: string（可选；来自 NRI 映射或兜底）
  - `pid_scope`: object（可选）
    - `pids`: number[]（可选）
  - `scope_key`: string（必填；归属对齐用的“快键”）
    - 建议规则：`scope_key = sha256_hex(pod_uid + "|" + cgroup_id)`（当 `pod_uid` 或 `cgroup_id` 为空时用空字符串参与哈希，保证确定性）
  - `network_target`: object（可选；仅用于 network evidence 的“探测目标”标识，不参与归属键 hash）
    - `target_id`: string（可选；对外暴露的目标ID）
    - `dst_ip`: string（可选；在 TCP connect 模式下建议必填）
    - `dst_port`: number（可选；在 TCP connect 模式下建议必填）
    - `protocol`: string（可选；默认 `tcp`，建议仅在扩展 ICMP/其它探测时填写）
    - `endpoint`: string（可选；service/host 名称等）
- `selection`: object（可选，用于解释“为什么有些字段没出现”）
  - `requested_metrics`: string[]（可选）
  - `collected_metrics`: string[]（可选；建议是 requested 的子集）
  - `requested_events`: string[]（可选）
  - `collected_events`: string[]（可选；建议是 requested 的子集）

### 2.1.1 Evidence 粒度建议（用于并行实现一致性）
- 建议每个 `Evidence` 对象对应：`task_id + evidence_type + scope_key + time_window` 的组合（粒度最简单、并行实现最稳）。
- 同一组合下如果多个 bpftrace probe 均有输出：在 `collector` 侧合并到同一个 `Evidence`（同一个 `evidence_id`），并把各 probe 的原始产物放入 `artifacts[]` 里供追溯。

### 2.2 数据载荷（按 evidence_type 扩展）
- `metric_summary`: object（可选）
  - 规则：字段命名在同一 `evidence_type` 内保持稳定；数值单位在文档/实现中固定。
  - 常用示例（仅示意）：`p50_ms`、`p90_ms`、`p99_ms`、`avg_ms`、`max_ms`
  - 重要：`metric_summary` 允许是“稀疏的”，只包含实际采集到的指标；用户未请求/采集失败的指标允许缺失。
- `events_topology`: array（可选）
  - 每项包含：
    - `event_type`: string
    - `event_time_ms`: number
    - `severity`: number（可选；建议 0~10）
    - `details`: object（可选）
- `top_calls`: object（可选）
  - `by_call`: array（可选）
    - `call_name`: string
    - `count`: number
    - `p95_latency_ms`: number（可选）
    - `p99_latency_ms`: number（可选）

### 2.2.1 `evidence_type=network` 字段契约（建议用于第 1 周 PoC）
目标：把“连通性/丢包/延迟突增”的证据以稳定字段输出，便于规则直接阈值/统计触发。

`metric_summary`（取决于用户请求的 metrics，缺失表示未采集/不可用）
- `connectivity_success_rate`: number（连通性成功率，0~1；由连通性探测窗口计算）
  - 定义口径（建议固定在实现层）：
    - 成功（按你选择的 B，底层口径）：一次探测在 `time_window` 内观测到 TCP 三次握手完成序列
      - `client(SYN)` -> `server(SYN-ACK)` -> `client(ACK)` 均被探测/采集到（以你们 bpftrace 证据为准）
    - 失败：上述任意阶段未完成（例如只见到 SYN、收到 RST/拒绝、握手超时等）
  - 计算方式：`success_count / total_probe_count`
  - 建议补充：在 `events_topology[].details` 中附带 `total_probe_count` 与 `success_count`（若实现成本允许），否则只输出 success_rate
- `loss_rate`: number（丢包率，0~1；由采样窗口计算；当你们仅使用 TCP connect 探测且未采集 packet-loss 证据时，该字段应缺失）
- `latency_p50_ms`: number（可选；若能稳定采集/且被请求）
  - 定义建议：TCP connect 建连耗时的 p50（单位 ms），按“client 端 SYN（发起）观察时间 -> client 端 ACK（握手完成）观察时间”的差值计算
- `latency_p90_ms`: number（可选；若能稳定采集/且被请求）
  - 同上：TCP connect 建连耗时的 p90（单位 ms）
- `latency_p99_ms`: number（可选；用于 `p99` 阈值触发/且被请求）
  - 同上：TCP connect 建连耗时的 p99（单位 ms）
- `latency_avg_ms`: number（可选；平均延迟，若能稳定采集/且被请求）
  - 定义建议：TCP connect 建连耗时的平均值（单位 ms）
- `jitter_ms`: number（可选；延迟抖动）
  - 定义建议（简单可实现口径）：`latency_p90_ms - latency_p50_ms`（若 p90 未采集/未请求，则 jitter_ms 不产生或可按实现侧固定规则替代，但需保持一致）
  - 说明：抖动口径以实现层固定规则为准；若依赖字段缺失则该字段按“缺失/不可用”处理（字段不应凭空补值）。

- `events_topology`（事件类型取决于用户请求）
- 事件时间语义强约束：
  - `connectivity_failure_burst`：当 `details.from_time_ms` 提供时，`events_topology[].event_time_ms` 必须等于 `details.from_time_ms`
  - `packet_loss_burst`：当 `details.from_time_ms` 提供时，`events_topology[].event_time_ms` 必须等于 `details.from_time_ms`
  - `latency_spike`：`events_topology[].event_time_ms` 表示 spike 起点（与 spike_window 起点一致，若提供 spike_window）
  - `fs_stall_spike`：`events_topology[].event_time_ms` 表示 spike 起点（与 spike_window.start_time_ms 一致，若提供 spike_window）
  - `syscall_latency_spike`：`events_topology[].event_time_ms` 表示 spike 起点（与 spike_window.start_time_ms 一致，若提供 spike_window）
  - `cgroup_throttle_burst`：`events_topology[].event_time_ms` 表示争抢/节流突增起点
- `connectivity_failure_burst`
  - `details`（可选）：
    - `from_time_ms`
    - `to_time_ms`
    - `failure_rate_during`（失败率）
    - `total_probe_count_during`（可选）
    - `failure_count_during`（可选）
    - `failure_stage`（可选；用于区分失败发生在哪个握手阶段）
      - 建议枚举：
        - `SYN_TIMEOUT`（只见 SYN，未见 SYN-ACK）
        - `RST_BEFORE_SYNACK`（任意一侧观察到 RST/拒绝，且在 SYN-ACK 之前）
        - `SYNACK_RECEIVED_ACK_TIMEOUT`（见到 SYN-ACK，但未完成 ACK）
        - `OTHER`（其它/无法归类）
    - `synack_missing_interval`（可选；仅建议用于 `failure_stage=SYN_TIMEOUT`）
      - `from_time_ms`：从该失败突发中“第一个观测到的 client SYN（或等价 SYN 事件）”开始
      - `to_time_ms`：截至“最后一次失败探测尝试结束”的时间点
        - 默认建议口径（最优解）：同一 evidence 内按 client SYN 观测时间分段，取最后一个尝试的 `to_time_ms`；若最后一次尝试没有后续 SYN，则为 `time_window.end_time_ms`
    - `synack_missing_attempts`（可选，更进一步；仅建议用于 `failure_stage=SYN_TIMEOUT`）
      - 数组中的每一项描述一次失败探测尝试的“缺失 SYN-ACK 区间”
      - 每项包含：
        - `attempt_index`: number（必填；探测尝试序号，保证确定性）
          - 定义建议（最优解，推荐实现）：在同一个 `evidence`（同一 `task_id` + `evidence_type=network` + `scope_key` + `time_window`）内，对所有观测到的 `SYN`（client 发出）按 `from_time_ms` 升序排序，序号从 0 开始。
        - `from_time_ms`: number（该次尝试开始的 client SYN 观测时间点）
        - `to_time_ms`: number（该次尝试结束时间点）
          - 默认建议口径（最优解）：截至“下一次尝试的 client SYN 观测时间”，若不存在下一次尝试则为 `time_window.end_time_ms`
    - `ack_missing_interval`（可选，更进一步；仅建议用于 `failure_stage=SYNACK_RECEIVED_ACK_TIMEOUT`）
      - `from_time_ms`：从首次观测到该失败阶段所对应尝试的 SYN-ACK 时间开始
      - `to_time_ms`：截至最后一次失败探测尝试结束的时间点（与该 burst 内 attempt 的“下一次 SYN 边界”一致；若最后一次没有下一次 SYN 则为 `time_window.end_time_ms`）
    - `ack_missing_attempts`（可选，更进一步；仅建议用于 `failure_stage=SYNACK_RECEIVED_ACK_TIMEOUT`）
      - 数组中的每一项描述一次失败探测尝试中“缺失 ACK”的区间
      - 每项包含：
        - `attempt_index`: number（必填；与 `synack_missing_attempts` 相同的定义：同一 evidence 内按 SYN 升序分配）
        - `from_time_ms`: number（该 attempt 对应的 SYN-ACK 首次观测时间）
        - `to_time_ms`: number（该 attempt 对应的结束时间边界；与“下一次 client SYN 观测”一致，若不存在下一次则为 `time_window.end_time_ms`）
    - `rst_observed_attempts`（可选；仅建议用于 `failure_stage=RST_BEFORE_SYNACK`）
      - 数组中的每一项描述一次失败探测尝试中“任意一侧观察到 RST/拒绝”的时间点
      - 每项包含：
        - `attempt_index`: number（必填；与同一 evidence 的 attempt_index 定义一致）
        - `rst_time_ms`: number（该 attempt 中首次观测到 RST/拒绝的时间点）
- `packet_loss_burst`
  - `details`（可选；建议尽量填）：`from_time_ms`、`to_time_ms`、`total_probe_count_during`、`loss_count_during`、`loss_rate_during`
    - 建议口径：`loss_rate_during = loss_count_during / total_probe_count_during`
    - 若只采集到 `loss_rate_during`，则允许省略 count 字段
    - `total_probe_count_during`/`loss_count_during` 的单位与“探测一次”的口径保持一致（TCP connect 模式下若不采集 packet-loss 证据，则该字段/事件可不出现）
- `latency_spike`
  - `details`（可选）：`latency_ms_at_spike`、`delta_p99_ms`（相对基线的跃迁幅度）
    - `latency_ms_at_spike`: number（单位 ms；建议为 spike 窗口的 p99，若实现成本允许则可用实际观测分位）
    - `delta_p99_ms`: number
      - 定义建议：`latency_p99_ms(spike_window) - latency_p99_ms(baseline_window)`
    - `spike_window`: object（可选，用于解释 delta 的 spike 窗口范围）
      - `start_time_ms`: number
      - `end_time_ms`: number
    - `baseline_window`: object（可选）
      - `start_time_ms`: number
      - `end_time_ms`: number
      - 用于说明 delta 的基线窗口范围（若不填，delta_p99_ms 的基线窗口由实现层固定规则决定）

`top_calls`
- 可选：若你们计划把网络相关系统调用作为线索，可把 syscall latency 聚合塞在 `top_calls`（字段仍遵循通用定义）。

部分采集/缺失策略
- 若用户只请求了丢包指标而未请求延迟指标：只应出现 `loss_rate` 相关字段，其它字段允许缺失。
- `events_topology` 允许为空数组或省略字段（但建议仍输出空数组/空字段，便于调用方统一处理）。

### 2.2.2 `evidence_type=block_io` 字段契约（建议用于第 1 周 PoC）
目标：把“块设备 I/O 延迟突增/超时/吞吐瓶颈”以稳定字段输出，便于阈值和关联分析触发。

`metric_summary`（取决于用户请求的 metrics，缺失表示未采集/不可用）
- `io_latency_p50_ms`: number（可选）
- `io_latency_p90_ms`: number（可选）
- `io_latency_p99_ms`: number（可选；用于 `p99` 阈值触发）
- `throughput_bytes_per_s`: number（可选；吞吐速率）
- `io_ops_per_s`: number（可选；IO 速率）
- `queue_depth`: number（可选；队列深度/积压指标）
- `timeout_count`: number（可选；IO 超时次数；若能稳定采到）

- `events_topology`（事件类型取决于用户请求）
- `io_latency_spike`
  - `details`（可选）：`io_latency_ms_at_spike`、`delta_p99_ms`（相对基线的跃迁幅度）
- `io_queue_depth_spike`
  - `details`（可选）：`queue_depth_at_spike`、`delta_queue_depth`（相对基线的跃迁幅度）
- `io_timeout`
  - `details`（可选）：`timeout_count`、`device_or_target`（设备/目标标识）
- `throughput_drop`
  - `details`（可选）：`throughput_bytes_per_s_at_drop`、`delta_throughput`（相对基线的跃迁幅度）

部分采集/缺失策略
- 若用户未请求吞吐或队列深度：允许缺失；规则应只使用已出现的字段生成结论，并在置信度上进行降级。

### 2.2.3 `evidence_type=fs_stall` 字段契约（建议用于第 2~4 周）
目标：把“文件系统卡顿”以稳定字段输出，并可与 `block_io`/`syscall_latency` 形成关联证据链。

`metric_summary`（取决于用户请求的 metrics，缺失表示未采集/不可用）
- `fs_stall_p50_ms`: number（可选）
- `fs_stall_p90_ms`: number（可选）
- `fs_stall_p99_ms`: number（可选；建议用于 `p99` 阈值触发）
- `fs_ops_per_s`: number（可选；文件系统相关操作速率）

`events_topology`（事件类型取决于用户请求）
- `fs_stall_spike`
  - `details`（可选）：`latency_ms_at_spike`、`delta_p99_ms`（相对基线的跃迁幅度）
    - `latency_ms_at_spike`: number（单位 ms）
    - `delta_p99_ms`: number
      - 定义建议：`fs_stall_p99_ms(spike_window) - fs_stall_p99_ms(baseline_window)`
    - `spike_window`: object（可选）
      - `start_time_ms`: number
      - `end_time_ms`: number
    - `baseline_window`: object（可选）
      - `start_time_ms`: number
      - `end_time_ms`: number

部分采集/缺失策略
- 若用户未请求 p99：允许只输出可用分位字段；规则以可用字段降级。

### 2.2.4 `evidence_type=syscall_latency` 字段契约（建议用于第 2~4 周）
目标：输出系统调用耗时异常（Top-N + 分位），用于关联“卡在某类调用（文件/网络等）”。

`metric_summary`（取决于用户请求的 metrics，缺失表示未采集/不可用）
- `syscall_latency_p50_ms`: number（可选）
- `syscall_latency_p90_ms`: number（可选）
- `syscall_latency_p99_ms`: number（可选；建议用于 `p99` 阈值触发）
- `syscall_ops_per_s`: number（可选）

`top_calls`（建议用于 syscall_latency；字段仍遵循通用定义）
- `top_calls.by_call[]` 建议至少包含：
  - `call_name`
  - `count`
  - `p99_latency_ms`（可选但建议；用于规则/诊断优先判断）

`events_topology`
- `syscall_latency_spike`
  - `details`（可选）：`top_call_name`、`delta_p99_ms`
    - `top_call_name`: string（建议与 `top_calls` 中的 call_name 对齐）
    - `delta_p99_ms`: number
      - 定义建议：`syscall_latency_p99_ms(spike_window) - syscall_latency_p99_ms(baseline_window)`（或以 top_call 的 p99 delta 作为近似）
    - `spike_window`: object（可选）
    - `baseline_window`: object（可选）

部分采集/缺失策略
- 若 top_calls 无法提供：仍允许依赖分位字段输出，只要字段由用户请求/采集可得即可。

### 2.2.5 `evidence_type=cgroup_compete` 字段契约（建议用于第 4~6 周）
目标：检测资源争抢迹象（CPU/内存/IO 等），输出可用于关联分析与置信度降级的证据。

`metric_summary`（取决于用户请求的 metrics，缺失表示未采集/不可用）
- `cpu_throttle_ratio`: number（0~1，可选；CPU 被节流比例/强度）
- `io_throttle_ratio`: number（0~1，可选；IO 节流/等待强度比例）
- `memory_pressure_index`: number（0~1，可选；内存压力指数）
- `contention_score`: number（0~1，可选；综合争抢评分，可由实现侧定义算法）

`events_topology`
- `cgroup_throttle_burst`
  - `details`（可选）：`resource_type`、`throttle_ratio_during`
    - `resource_type`: string（`cpu`|`memory`|`io`）
    - `throttle_ratio_during`: number（0~1；可选）

部分采集/缺失策略
- 若无法解析某资源维度：允许仅输出其余维度，并在诊断中降低置信度。

### 2.2.6 `evidence_type=oom`（可选，用于第 2~3 周联动）
`events_topology`
- `oom_kill`
  - `event_time_ms`: OOM 触发时间
  - `details`（可选）：`killed_pid`、`oom_score_adj`、`target_cgroup_id`（如可得）

### 2.3 归属与质量标识（重要）
- `attribution`: object
  - `status`: string（`nri_mapped`|`pid_cgroup_fallback`|`unknown`）
  - `confidence`: number（0~1；可选）
  - `source`: string（`nri`|`pid_map`|`cgroup_map`|`none`）
  - `mapping_version`: string（可选；用于定位 mapping 表版本）

### 2.4 可追溯性（可选，但强烈建议）
- `artifacts`: array（可选）
  - 每项：`artifact_type`: string（例如 `raw_json`|`bttrace_out`）、`artifact_uri`: string、`digest`: string（可选）

### 2.5 `evidence_id` 生成规则（建议）
为避免并行与重试产生不同 ID，建议生成策略：
- `evidence_id = sha256(task_id + "|" + evidence_type + "|" + collection_id + "|" + scope.scope_key)`，取 hex 或 base64url
- 若你们已经有更好的唯一键（如全局 UUID），也可以替换，但要保证：同一任务同一采样与同一 scope_key 能映射到稳定的证据 ID。

## 3. Diagnosis Result（诊断结果）Schema v0.2

### 3.1 顶层字段（必填）
- `schema_version`: string（例如 `"diagnosis.v0.2"`）
- `task_id`: string
- `status`: string（`running`|`done`|`failed`|`partial`）
- `runtime`: object（可选但建议）
  - `started_time_ms`: number（epoch ms；可选）
  - `finished_time_ms`: number（epoch ms；可选）
  - `duration_ms`: number（可选）
- `trigger`: object（必填）
  - `trigger_type`: string（`manual`|`condition`|`event`）
  - `trigger_reason`: string
  - `trigger_time_ms`: number
  - `matched_condition`: string（可选）
  - `event_type`: string（可选，如 `OOM`）

### 3.2 证据引用（必填）
- `evidence_refs`: array（必填）
  - 每项：
    - `evidence_id`: string（必填；与 Evidence.evidence_id 对齐）
    - `evidence_type`: string（可选；用于快速过滤）
    - `scope_key`: string（可选；用于快速对齐）
    - `role`: string（可选；例如 `primary`|`support`|`context`）

### 3.3 结论与建议
- `conclusions`: array（必填，至少可为空数组但字段要存在）
  - 每项包含：
    - `conclusion_id`: string（必填；用于 traceability 交叉引用）
    - `title`: string
    - `confidence`: number（0~1；必填）
    - `evidence_strength`: string（`low`|`medium`|`high`）
    - `details`: object（可选）
- `recommendations`: array（可选）
  - 每项包含：
    - `priority`: number（越小越优先）
    - `action`: string
    - `expected_impact`: string（可选）
    - `verification`: string（可选：如何验证是否已缓解）

### 3.4 可追溯性（必填）
- `traceability`: object（必填）
  - `references`: array（建议非空）
    - `conclusion_id`: string
    - `evidence_ids`: string[]（必填）
    - `reasoning_summary`: string（简述关联逻辑）
  - `engine_version`: string（可选；规则引擎版本/模型版本）

### 3.5 AI 与告警的“可用性分离”（可选）
- `ai`: object（可选）
  - `enabled`: boolean
  - `status`: string（`ok`|`timeout`|`unavailable`|`failed`）
  - `summary`: string（可选；AI 给出的解释段）
- 告警平台 payload 属于 `Result Publisher` 的映射层，建议不要强耦合在 schema 里（但可以保留 `alert_payload_version` 作为扩展）。

## 4. 字段对齐硬规则（给并行开发的约束）
1) 时间统一：所有 `*_time_ms` / `time_window.*_ms` 必须是 epoch ms。
2) 归属对齐：`Evidence.scope.scope_key` 与 `NRI mapping` 的归属键生成逻辑一致；回退/unknown 必须有明确 `attribution.status`。
3) 引用一致：`Diagnosis.evidence_refs[].evidence_id` 必须能在本次任务的 Evidence 集合中解析到对应对象；否则 diagnosis.status 应标记 `partial`。
4) 规则引擎只依赖 `Evidence` 的字段（不得直接解析 bpftrace 输出格式）。
5) 输出器只依赖 `Diagnosis Result` 的字段（告警与 AI 以 diagnosis schema 为输入）。

## 6. 降级与部分证据处理规则（行为约束）
> 这部分用于约束：当采集成功率不高或某些能力不在范围内时，诊断结果如何仍然可解析、可追溯。

### 6.1 Diagnosis status
- `done`：所有 `evidence_refs` 均可解析到本任务的 Evidence，且结论/建议均基于已采集到的字段生成（即使某些 evidence_type 或 metrics 没有被请求/未出现）。
- `partial`：存在以下任意情况：
  - evidence_refs 中存在未解析的 evidence_id；
  - 触发了诊断，但关键规则所需指标未出现（例如触发了 `p99` 阈值，但 `latency_p99_ms` 未采集/未请求），导致结论只能低置信度或仅能输出部分结论；
  - 采集为 `collection_status=partial`，且关键字段缺失。
- `failed`：无法生成可用的结构化结论（例如 Evidence 集合为空或全量采集失败），但仍必须输出可解析的错误原因（在后续扩展中建议加 `errors[]` 字段）。

### 6.2 confidence / evidence_strength 的基本策略
- 证据归属 `attribution.status` 为 `unknown` 或回退来源（`pid_cgroup_fallback`）时：
  - `confidence` 建议整体下调（例如从 `>=0.7` 下调到 `<=0.5`，具体阈值由规则库决定）。
- 缺失关键分位字段（例如 network 缺 `latency_p99_ms`）时：
  - 用可用字段生成“趋势/异常存在”的结论时，`evidence_strength` 降为 `low/medium`，避免把缺失字段当作证据。

### 6.3 规则引擎的输入边界
- 规则引擎不得根据“字段未出现”推导其为 0 或正常；字段缺失应被视为“不确定”。
- 对阈值规则（如 `p99`）：
  - 若没有 `latency_p99_ms`（或 `io_latency_p99_ms`），阈值规则应返回“未命中/无法评估”，并让 Diagnosis 进入 `partial` 或只输出其它可评估结论。

## 5. 示例（简化版）
### 5.1 Evidence（network 示例，示意）
```json
{
  "schema_version": "evidence.v0.2",
  "task_id": "t-123",
  "evidence_id": "sha256(...)",
  "evidence_type": "network",
  "time_window": { "start_time_ms": 1712050000000, "end_time_ms": 1712050060000 },
  "scope": {
    "pod": { "uid": "pod-uid-1", "name": "app-1", "namespace": "default" },
    "cgroup_id": "cg-1",
    "scope_key": "sha256_hex(pod-uid-1|cg-1)"
  },
  "collection": {
    "collection_id": "c-456",
    "collection_status": "success",
    "probe_id": "net_trace.bt",
    "errors": []
  },
  "metric_summary": { "connectivity_success_rate": 0.85, "latency_p99_ms": 120 },
  "events_topology": [
    { "event_type": "connectivity_failure_burst", "event_time_ms": 1712050032000, "severity": 8 }
  ],
  "attribution": { "status": "nri_mapped", "confidence": 0.95, "source": "nri" }
}
```

### 5.2 Diagnosis Result（示意）
```json
{
  "schema_version": "diagnosis.v0.2",
  "task_id": "t-123",
  "status": "done",
  "trigger": { "trigger_type": "condition", "trigger_reason": "network connectivity/latency rule hit", "trigger_time_ms": 1712050060000 },
  "evidence_refs": [
    { "evidence_id": "sha256(...)", "evidence_type": "network", "role": "primary", "scope_key": "sha256_hex(pod-uid-1|cg-1)" }
  ],
  "conclusions": [
    { "conclusion_id": "con-1", "title": "网络延迟/丢包异常导致请求变慢", "confidence": 0.78, "evidence_strength": "high" }
  ],
  "recommendations": [
    { "priority": 1, "action": "检查网络抖动与目标端链路质量", "verification": "connectivity_success_rate 恢复到阈值以上，且 latency_p99_ms 恢复到阈值以下" }
  ],
  "traceability": {
    "references": [
      {
        "conclusion_id": "con-1",
        "evidence_ids": ["sha256(...)"],
        "reasoning_summary": "网络证据显示 p99 延迟与丢包在时间窗内显著跃迁"
      }
    ],
    "engine_version": "rules.v0.1"
  }
}
```
