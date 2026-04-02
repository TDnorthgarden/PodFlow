# NRI Pod 信息接入与归属映射规范

> 目的：把来自 NRI 的 Pod/容器元信息稳定落到插件侧的“归属映射表”，从而让 bpftrace 采集到的内核/进程事件能对齐到容器/Pod。

## 1. NRI 入口与职责边界
- NRI 输入：将 Pod/容器元信息与 cgroup/pid 关联关系推送给故障插件。
- 插件职责：
  - 维护映射表：Pod -> cgroup -> pid（或 pid -> cgroup -> Pod 反查）；
  - 处理 NRI 事件的新增/更新/删除；
  - 为证据采集与聚合提供“归属 key”和“归属状态”。

## 2. 映射表（建议的数据结构）
- `pod_map`: key=`pod_uid` 或 `namespace/name/uid`
  - 值：`{pod_uid, pod_name, namespace, containers[], ...}`
- `container_map`: key=`container_id`（或 runtime id）
  - 值：`{container_id, pod_uid, cgroup_ids[]}`
- `cgroup_map`: key=`cgroup_id`（或路径哈希/标识）
  - 值：`{cgroup_id, pod_uid, container_id(optional)}`
- `pid_map`（可选）：key=`pid`
  - 值：`{pid, cgroup_id}`（用于回退兜底）

> 你们需要根据实际 NRI payload 字段把 key/value 落地（下文标记 TBD）。

## 3. 归属 key 与优先级策略
建议统一采用如下优先级来降低不确定性：
1) 若 NRI 提供了 `pod -> cgroup` 或 `pod -> cgroup_id`：直接归属到 Pod/容器；
2) 若事件仅能关联到 pid/cgroup：通过映射表反查到 Pod；
3) 若映射缺失或延迟更新：标记 `Evidence.attribution.status` 为 `pid_cgroup_fallback` 或 `unknown`，并在诊断结果中体现低置信度。

### 3.1 `scope_key` 哈希规则（用于 evidence_id / 归属对齐）
为了减少资源并保证并行/重试下的确定性，建议使用固定哈希将 `pod_uid` 与 `cgroup_id` 归并到一个字符串键：
- `scope_key = sha256_hex(pod_uid + "|" + cgroup_id)`
- 当 `pod_uid` 或 `cgroup_id` 缺失时，用空字符串参与哈希，以保证生成结果确定且可复现

## 4. NRI 事件类型与处理规则
（字段命名以你们实际 NRI 实现为准）
- `Add/Update`：
  - 更新 pod/container/cgroup 映射；
  - 对已有条目做原子替换或版本号比较（避免并发乱序）。
- `Delete`：
  - 删除映射表中的对应关系；
  - 历史证据不回收（证据采集的 time_window 已固化）；只影响未来任务的归属。

## 5. 时序与延迟一致性
为了避免“bpftrace 事件早于 NRI 映射”的情况：
- 映射表支持短期缓存（TTL），例如 5~30s（TBD）；
- 证据聚合阶段允许在 time_window 内进行归属补偿：
  - 若映射在窗口内某时刻已出现，则归属到该 time_window 的对应 Pod；
  - 若始终缺失，则标记归属不确定并降级规则。

## 6. 兜底策略与错误码
- 归属缺失：返回可用于诊断的证据，但 `Evidence.attribution.status` 置为非理想值；
- NRI 不可用：触发链路仍可跑通（至少输出失败原因码与部分证据），AI/告警可降级。

建议在错误码中至少覆盖：
- `NRI_UNAVAILABLE`
- `MAPPING_MISSING`
- `MAPPING_STALE`
- `POD_DELETED_DURING_WINDOW`
- `ATTRIBUTION_UNCERTAIN`

