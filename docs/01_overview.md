# 容器智能故障分析插件（概览）

## 1. 项目简介
本项目面向容器场景，提供一款“智能故障分析插件”。当网络异常、块设备 I/O 延迟/吞吐瓶颈、文件系统卡顿、系统调用耗时异常、cgroup 资源争抢等问题出现时，插件通过内核观测与证据聚合，形成结构化诊断结论与处置建议，并支持：
1) 结构化日志输出本地；
2) 推送至告警平台；
3) （可选）对接 AI 输出解释与排查路径。

核心思路：
1) 基于内核观测（bpftrace）采集关键证据；
2) 在统一时间窗内聚合指标与事件；
3) 通过规则与关联分析形成诊断结论；
4) 输出结构化结果并进行外部对接。

## 2. 功能范围（与用户可见能力对齐）
- 网络连通性检测：丢包/延迟跟踪、关键统计与时间窗证据输出。
- 容器块设备 I/O 延迟与瓶颈分析：I/O 延迟异常、吞吐瓶颈、队列积压/抖动、延迟-吞吐耦合关系。
- 文件系统卡顿：与块设备 I/O 证据联动，减少误判。
- 系统调用耗时统计：耗时分布与 Top N（按调用类型/进程/容器聚合）。
- cgroup 资源争抢检测：CPU/内存/IO 争抢迹象、影响范围与可能触发原因。
- 灵活触发机制：手动触发（API/CLI）、条件触发（阈值/规则表达式）、异常事件联动（如 OOM）。
- 智输出：结构化日志（JSON）+ 告警平台推送（payload）。
- 对接 AI：基于证据链生成解释与排查建议；AI 不可用时降级保持核心链路可用。

## 3. 端到端架构（数据流）
```mermaid
flowchart LR
  %% ===== Inputs =====
  subgraph K8s["Kubernetes / 运行时环境"]
    NRI[NRI Pod 信息入口]
  end

  %% ===== Core plugin =====
  subgraph Plugin["故障分析插件（核心）"]
    Mapping["Pod->cgroup/pid 归属映射表\n(由NRI维护)"]
    Trigger["触发器 Trigger Service\n(API / CLI / 条件 / 异常联动)"]
    Collector["采集器 Collector\n(bpftrace 采集探针/脚本)"]
    Evidence["证据聚合 Evidence Aggregator\n(统一时间窗对齐/去重)"]
    Diagnosis["诊断引擎 Diagnosis Engine\n(规则 + 关联分析)"]
    Publisher["结果发布器 Result Publisher\n(结构化日志/告警推送)"]
    AI["AI 适配层 AI Adapter\n(可选：解释/排查路径建议)"]
  end

  %% ===== Outputs =====
  subgraph Outputs["输出与外部系统"]
    LocalLog["本地结构化日志(JSON)"]
    Alert["告警平台(推送 payload)"]
  end

  %% ===== Flow =====
  NRI --> Mapping
  Trigger -->|生成诊断任务与时间窗| Collector
  Mapping -->|归属映射| Collector
  Collector -->|bpftrace 证据(指标/事件/Top syscall)| Evidence
  Evidence -->|证据摘要/字段化结果| Diagnosis

  Diagnosis --> Publisher
  Diagnosis -->|证据与初步结论| AI
  AI -->|补充解释/建议| Publisher

  Publisher --> LocalLog
  Publisher --> Alert
```

## 4. 数据来源与关键前置依赖
- `NRI Pod 信息入口`：向插件输入 Pod/容器元信息，并维护 Pod 与 cgroup/pid 的归属映射（详见 `docs/03_nri_mapping_spec.md`）。
- `bpftrace`：作为底层观测采集方式，产出网络/块 IO/文件系统/系统调用/cgroup 等证据（详见 `docs/04_bpftrace_minimal_chains.md`）。
- 统一时间窗：将跨能力采集结果在同一诊断任务时间窗内对齐聚合，保证规则与关联分析的可用性。

## 6. Collector 字段映射文档
为便于并行开发“探针 -> Evidence 字段”对齐，建议统一阅读 `docs/08_collector_bpftrace_to_fields.md`。

## 5. 产出形式（输出口径）
- 本地：结构化日志（建议为 JSON），包含触发信息、证据摘要、结论、建议与 traceability 引用。
- 告警平台：发送诊断摘要与关键证据（payload 字段映射可在 `docs/05_api_cli_contract.md` 中定义）。
- AI（可选）：将证据链作为输入，输出解释与建议；失败时降级为“非 AI 诊断”。

