# bpftrace 脚本输出标准化规范

## 概述

本规范定义了bpftrace诊断脚本的**标准输出格式**，使客户自定义脚本能与nuts-observer采集器无缝对接。

## 标准输出格式

### 1. 基础规则

- **输出格式**: JSON Lines (每行一个独立JSON对象)
- **编码**: UTF-8
- **换行符**: `\n`
- **时间戳**: 毫秒级Unix时间戳 (nsecs / 1000000)

### 2. 标准事件类型

```json
// 1. 采集开始标记
{"type":"start","msg":"probe_name started","probe_id":"network.tcp_connect","version":"v0.1"}

// 2. 单个事件 (核心)
{"type":"<event_type>","pid":1234,"comm":"process_name","ts_ms":1234567890,<metric_fields>}

// 3. 聚合统计 (可选)
{"type":"stats","ts_ms":1234567890,"count":100,"p99":1234.5}

// 4. 错误/警告
{"type":"error","code":"PROBE_ERROR","msg":"description","ts_ms":1234567890}

// 5. 采集结束标记
{"type":"end","msg":"probe_name stopped","duration_ms":5000}
```

### 3. 字段命名规范

| 字段名 | 类型 | 说明 | 必填 |
|--------|------|------|------|
| `type` | string | 事件类型 | ✓ |
| `pid` | uint32 | 进程ID | ✓ |
| `comm` | string | 进程名(comm) | ✓ |
| `ts_ms` | uint64 | 毫秒时间戳 | ✓ |
| `latency_us` | uint64 | 延迟(微秒) | 延迟类 |
| `bytes` | uint64 | 字节数 | I/O类 |
| `count` | uint32 | 事件计数 | 统计类 |
| `dev` | string | 设备名 | I/O类 |
| `target` | string | 目标地址/端口 | 网络类 |

### 4. 证据类型映射

| evidence_type | 关键指标 | 标准事件type |
|---------------|----------|--------------|
| `network` | latency_p99, retransmit_rate | `tcp_connect`, `tcp_retransmit` |
| `block_io` | io_latency_p99, queue_depth | `io_complete`, `io_timeout` |
| `fs_stall` | fs_op_latency | `fs_op_start`, `fs_op_done` |
| `cgroup_contention` | cpu_throttle, memory_pressure | `cpu_throttle`, `memory_pressure` |
| `syscall_latency` | syscall_duration | `syscall_enter`, `syscall_exit` |
| `oom_events` | oom_kill | `oom_kill` |

## 客户脚本适配指南

### 方法一: 直接遵循标准 (推荐)

客户脚本按上述规范输出JSON，可直接被采集器解析。

**示例 - CPU throttle检测**:
```bpftrace
#!/usr/bin/env bpftrace

BEGIN {
    printf("{\"type\":\"start\",\"msg\":\"cpu_throttle_probe started\"}\n");
}

// 跟踪CPU throttle事件
tracepoint:sched:sched_stat_throttled {
    printf("{\"type\":\"cpu_throttle\",\"pid\":%d,\"comm\":\"%s\",\"throttled_us\":%llu,\"ts_ms\":%llu}\n",
        pid, comm, args->throttled_time / 1000, nsecs / 1000000);
}

interval:s:5 {
    printf("{\"type\":\"stats\",\"count\":%d}\n", count(@throttled));
    clear(@throttled);
}

END {
    printf("{\"type\":\"end\",\"msg\":\"cpu_throttle_probe stopped\"}\n");
}
```

### 方法二: 使用转换适配器

客户脚本保持原有输出格式，通过**nuts-bpftrace-adapter**转换：

```python
# 客户脚本输出格式: "PID=1234 TIME=1234567890 LATENCY=1234us"
# 适配器转换为标准JSON
```

适配器模式详见下方。

## 脚本模板库

### 模板1: 网络延迟 (network)

```bpftrace
#!/usr/bin/env bpftrace
/*
 * 标准网络延迟采集脚本
 * Evidence type: network
 * 输出字段: pid, comm, latency_us, target, ts_ms
 */

#include <net/sock.h>

BEGIN {
    printf("{\"type\":\"start\",\"msg\":\"network_latency started\",\"probe_id\":\"network.latency\"}\n");
}

kprobe:tcp_v4_connect {
    @start[tid] = nsecs;
    @target[tid] = arg1;  // 目标IP
}

kretprobe:tcp_v4_connect /@start[tid]/ {
    $latency_us = (nsecs - @start[tid]) / 1000;
    $target_ip = @target[tid];
    printf("{\"type\":\"tcp_connect\",\"pid\":%d,\"comm\":\"%s\",\"latency_us\":%llu,\"target\":\"%s\",\"ts_ms\":%llu}\n",
        pid, comm, $latency_us, ntop(AF_INET, $target_ip), nsecs / 1000000);
    delete(@start[tid]);
    delete(@target[tid]);
}

END {
    printf("{\"type\":\"end\",\"msg\":\"network_latency stopped\"}\n");
    clear(@start);
    clear(@target);
}
```

### 模板2: I/O延迟 (block_io)

```bpftrace
#!/usr/bin/env bpftrace
/*
 * 标准I/O延迟采集脚本
 * Evidence type: block_io
 * 输出字段: pid, comm, dev, bytes, rw, latency_us, ts_ms
 */

#include <linux/blkdev.h>

BEGIN {
    printf("{\"type\":\"start\",\"msg\":\"io_latency started\",\"probe_id\":\"block_io.latency\"}\n");
}

kprobe:blk_account_io_start {
    $req = (struct request *)arg0;
    @io_start[(uint64)$req] = nsecs;
    @io_pid[(uint64)$req] = pid;
    @io_comm[(uint64)$req] = comm;
    @io_dev[(uint64)$req] = $req->q->disk->disk_name;
}

kprobe:blk_account_io_done /@io_start[(uint64)arg0]/ {
    $req = (struct request *)arg0;
    $key = (uint64)$req;
    $latency_us = (nsecs - @io_start[$key]) / 1000;
    
    printf("{\"type\":\"io_complete\",\"pid\":%d,\"comm\":\"%s\",\"dev\":\"%s\",\"bytes\":%llu,\"rw\":\"%s\",\"latency_us\":%llu,\"ts_ms\":%llu}\n",
        @io_pid[$key], @io_comm[$key], @io_dev[$key], 
        $req->__data_len, ($req->cmd_flags & 1 ? "W" : "R"),
        $latency_us, nsecs / 1000000);
    
    delete(@io_start[$key]);
    delete(@io_pid[$key]);
    delete(@io_comm[$key]);
    delete(@io_dev[$key]);
}

END {
    printf("{\"type\":\"end\",\"msg\":\"io_latency stopped\"}\n");
}
```

### 模板3: 系统调用延迟 (syscall_latency)

```bpftrace
#!/usr/bin/env bpftrace
/*
 * 标准系统调用延迟采集脚本
 * Evidence type: syscall_latency
 * 输出字段: pid, comm, syscall_name, duration_us, ts_ms
 */

tracepoint:raw_syscalls:sys_enter {
    @start[tid] = nsecs;
    @syscall[tid] = args->id;
}

tracepoint:raw_syscalls:sys_exit /@start[tid]/ {
    $duration_us = (nsecs - @start[tid]) / 1000;
    $syscall_id = @syscall[tid];
    
    printf("{\"type\":\"syscall_exit\",\"pid\":%d,\"comm\":\"%s\",\"syscall_id\":%d,\"duration_us\":%llu,\"ts_ms\":%llu}\n",
        pid, comm, $syscall_id, $duration_us, nsecs / 1000000);
    
    delete(@start[tid]);
    delete(@syscall[tid]);
}
```

## 适配器模式

### 场景: 客户已有脚本无法修改

**客户脚本输出**:
```
[2024-01-15 10:30:45] PID:1234 Process:nginx Latency:1234us Target:192.168.1.1
```

**适配器配置** (`adapters/nginx_network.yaml`):
```yaml
adapter_id: nginx_network_latency
source_type: regex_log
source_pattern: '\[(?<ts>[^\]]+)\] PID:(?<pid>\d+) Process:(?<comm>\w+) Latency:(?<latency>\d+)us Target:(?<target>[\d.]+)'
target_schema:
  type: tcp_connect
  pid: "${pid}"
  comm: "${comm}"
  latency_us: "${latency}"
  target: "${target}"
  ts_ms: "parse_timestamp('${ts}', '%Y-%m-%d %H:%M:%S')"
```

**转换后输出**:
```json
{"type":"tcp_connect","pid":1234,"comm":"nginx","latency_us":1234,"target":"192.168.1.1","ts_ms":1705319445000}
```

## 验证工具

### 脚本合规检查器

```bash
# 检查脚本输出是否符合标准
./nuts-observer validate-bpftrace --script ./my_probe.bt --timeout 5s

# 输出:
# ✓ 事件格式正确
# ✓ 字段完整 (pid, comm, ts_ms)
# ✓ JSON解析成功
# ✗ 缺少end标记
# 建议: 在END探针中添加end事件
```

## 与Evidence结构映射

```
┌─────────────────────────────────────────────────────────────┐
│ bpftrace输出                                                │
│ {"type":"tcp_connect","pid":1234,"latency_us":1234}       │
└──────────────────┬──────────────────────────────────────────┘
                   │ 转换
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ Evidence结构                                                │
│ - schema_version: "evidence.v0.2"                           │
│ - evidence_type: "network"                                │
│ - metric_summary: {                                         │
│     "latency_p99_us": 1234,                                 │
│     "connection_count": 1                                   │
│   }                                                         │
│ - events_topology: [                                        │
│     {event_type:"tcp_connect", event_time_ms:..., ...}      │
│   ]                                                         │
└─────────────────────────────────────────────────────────────┘
```

## 附录

### A. 完整字段类型定义

```yaml
# /root/nuts/docs/schemas/bpftrace_output_v0.1.yaml
schemas:
  base_event:
    required: [type, pid, comm, ts_ms]
    fields:
      type: { type: string, enum: [start, end, stats, error, <custom>] }
      pid: { type: uint32 }
      comm: { type: string, max_length: 16 }
      ts_ms: { type: uint64 }
  
  latency_event:
    extends: base_event
    required: [latency_us]
    fields:
      latency_us: { type: uint64, description: "延迟微秒数" }
  
  io_event:
    extends: base_event
    required: [dev, bytes, rw, latency_us]
    fields:
      dev: { type: string }
      bytes: { type: uint64 }
      rw: { type: string, enum: [R, W] }
      latency_us: { type: uint64 }
```

### B. 测试用例

```bash
# 测试脚本输出合规性
echo '{"type":"test","pid":1,"comm":"init","ts_ms":1234567890}' | \
  ./nuts-observer validate --schema bpftrace_output

# 测试数据转换
echo '{"type":"tcp_connect","pid":1234,"comm":"test","latency_us":100,"ts_ms":1234567890}' | \
  ./nuts-observer convert --to evidence --evidence-type network
```
