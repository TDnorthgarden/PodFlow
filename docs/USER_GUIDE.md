# PodFlow 用户指南

## 目录
- [快速开始](#快速开始)
- [基本概念](#基本概念)
- [命令行使用](#命令行使用)
- [常见问题](#常见问题)
- [故障排查](#故障排查)
- [最佳实践](#最佳实践)

## 快速开始

### 安装
```bash
# 从源码构建
git clone https://github.com/your-org/podflow.git
cd podflow
cargo build --release
sudo cp target/release/podflow /usr/local/bin/

# 使用包管理器安装（如果提供）
sudo yum install podflow  # RHEL/CentOS
sudo apt install podflow  # Ubuntu/Debian
```

### 快速验证
```bash
# 检查安装
podflow --help

# 检查系统状态
podflow status
```

## 基本概念

### 核心组件
- **NRI适配器**: 与容器运行时接口，获取Pod和容器信息
- **采集器**: 基于bpftrace的内核观测探针
- **证据聚合器**: 统一时间窗内的观测数据
- **诊断引擎**: 基于规则和关联分析的智能诊断
- **案例库**: 历史故障案例和解决方案

### 数据流
```
容器事件 → NRI适配器 → 证据聚合器 → 诊断引擎 → 结构化输出
```

### 证据类型
- **cgroup_contention**: cgroup资源争抢（CPU、内存、IO）
- **network**: 网络延迟、丢包、连接问题
- **block_io**: 块设备IO延迟、吞吐瓶颈
- **syscall_latency**: 系统调用耗时异常
- **oom_events**: 内存不足事件

## 命令行使用

### 基本命令结构
```bash
podflow [GLOBAL_OPTIONS] <SUBCOMMAND> [SUBCOMMAND_OPTIONS]
```

### 全局选项
- `--server <url>`: API服务器地址（默认: http://localhost:8080）
- `--no-color`: 禁用彩色输出
- `--config <file>`: 指定配置文件路径

### 主要子命令

#### 诊断命令
```bash
# 手动触发诊断
podflow trigger --pod-uid <uid> --namespace <ns>

# 持续监控
podflow watch --pod-uid <uid> --interval 5 --count 10

# 查询诊断结果
podflow query --task-id <task-id>

# 查看服务状态
podflow status
```

#### 配置管理
```bash
# 列出诊断规则
podflow config list-rules

# 添加诊断规则
podflow config set-rule --rule-id cpu-high --metric-name cpu_usage --operator ">" --threshold 80

# 导出规则配置
podflow config export --file backup-rules.yaml
```

#### 案例库管理
```bash
# 列出所有案例
podflow case list

# 查看案例详情
podflow case show --case-id <case-id>

# 根据指标匹配案例
podflow case match --metrics "cpu_usage=85,memory_usage=90"

# 导出案例库
podflow case export --file cases-backup.yaml
```

#### Pod管理
```bash
# 列出集群中的Pod
podflow list-pods --namespace default

# 导出Pod列表
podflow export --file pods.json
```

### 输出格式

#### JSON格式（默认）
```json
{
  "task_id": "task-123",
  "status": "completed",
  "evidence_count": 15,
  "diagnosis": {
    "conclusion": "CPU使用率过高",
    "confidence": 0.85,
    "root_cause": "应用程序CPU密集",
    "remediation": [
      {
        "action": "优化应用程序",
        "expected_outcome": "CPU使用率降低"
      }
    ]
  }
}
```

#### 表格格式
```bash
# Pod列表表格输出
NAMESPACE     POD NAME              POD UID              STATUS
default       my-app               abc123              Running
default       web-server            def456              Pending
```

#### YAML格式
```yaml
cases:
  - case_id: "euler-cpu-steal"
    title: "CPU窃取检测"
    description: "检测到容器CPU时间被异常窃取"
    evidence_types: ["cgroup_contention"]
    metric_patterns:
      - metric_name: "cpu_steal_time"
        operator: ">"
        threshold: 10.0
```

## 常见问题

### 安装和配置

#### Q: 如何在Kubernetes中部署？
A: 推荐使用DaemonSet方式部署：

```yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: podflow
  namespace: monitoring
spec:
  selector:
    matchLabels:
      name: podflow
  template:
    metadata:
      labels:
        name: podflow
    spec:
      containers:
      - name: podflow
        image: podflow:latest
        securityContext:
          privileged: true
        volumeMounts:
          - name: host-filesystem
            mountPath: /host
        env:
          - name: PODFLOW_LOG_LEVEL
            value: "info"
          - name: PODFLOW_SERVER
            value: "http://podflow-api:8080"
      volumes:
      - name: host-filesystem
        hostPath: /
```

#### Q: 权限不足怎么办？
A: 确保以下权限：

```bash
# 检查当前用户
id podflow

# 添加必要的权限
sudo usermod -a -G bpf $USER

# 重新登录
newgrp bpf
```

#### Q: 如何调整日志级别？
A: 通过环境变量或配置文件：

```bash
# 环境变量方式
export PODFLOW_LOG_LEVEL=debug
export PODFLOW_LOG_FORMAT=json

# 配置文件方式
cat > /etc/podflow/config.yaml << EOF
log_level: "debug"
output_format: "json"
EOF
```

### 诊断相关问题

#### Q: 诊断没有输出结果？
A: 检查以下几点：

1. **权限问题**: 确保有足够权限访问bpf和容器信息
2. **配置问题**: 检查NRI配置和容器运行时兼容性
3. **目标容器**: 确认目标容器正在运行且可访问
4. **时间窗口**: 诊断需要足够的时间窗口收集证据

```bash
# 调试模式运行
podflow trigger --pod-uid abc123 --log-level debug

# 检查详细状态
podflow status --verbose
```

#### Q: 误报率较高怎么办？
A: 优化策略：

1. **调整阈值**: 根据历史数据调整合理的阈值
2. **增加证据类型**: 添加更多证据类型提高准确性
3. **优化规则**: 改进诊断规则的逻辑
4. **机器学习**: 启用AI适配器提高判断准确性

```bash
# 查看当前规则
podflow config list-rules

# 调整阈值
podflow config set-rule --rule-id cpu-high --threshold 85
```

#### Q: 性能影响较大？
A: 优化建议：

1. **减少采集频率**: 调整bpftrace探针的采样率
2. **过滤目标**: 只采集必要的容器和指标
3. **异步处理**: 启用异步处理减少阻塞
4. **资源限制**: 设置合理的CPU和内存限制

```yaml
# 性能优化配置
performance:
  sampling_rate: 100  # 每秒采样次数
  max_containers: 50
  async_processing: true
  resource_limits:
    cpu_limit: "500m"
    memory_limit: "256Mi"
```

## 故障排查

### 常见错误代码

#### 权限错误
```
Error: Permission denied (bpf)
解决: sudo usermod -a -G bpf $USER && newgrp bpf
```

#### 连接错误
```
Error: Failed to connect to NRI socket
解决: 检查containerd运行状态和socket路径
```

#### 内存不足
```
Error: Out of memory
解决: 增加容器内存限制或优化采集逻辑
```

### 日志分析

#### 关键日志位置
```bash
# 系统日志
journalctl -u podflow -f

# 应用日志
/var/log/podflow/observer.log

# NRI日志
/var/log/nri/nri.log
```

#### 日志级别说明
- **ERROR**: 严重错误，需要立即处理
- **WARN**: 警告信息，需要注意
- **INFO**: 一般信息，正常运行状态
- **DEBUG**: 调试信息，详细执行过程

## 最佳实践

### 生产环境部署

#### 1. 资源配置
```yaml
# 推荐的生产配置
resources:
  requests:
    cpu: "100m"
    memory: "128Mi"
  limits:
    cpu: "500m" 
    memory: "512Mi"
```

#### 2. 安全配置
```yaml
securityContext:
  runAsUser: 1000
  runAsGroup: 1000
  privileged: true
  capabilities:
    add:
      - SYS_ADMIN
      - SYS_RESOURCE
```

#### 3. 监控策略
```yaml
# 分层监控策略
monitoring:
  tier1:  # 基础健康检查
    interval: 30s
    timeout: 5s
  tier2:  # 性能指标监控
    interval: 60s
    metrics: ["cpu", "memory", "network"]
  tier3:  # 故障检测
    interval: 10s
    evidence_types: ["cgroup_contention", "block_io"]
```

### 集成配置

#### Prometheus集成
```yaml
# Prometheus指标导出配置
metrics:
  enabled: true
  port: 9090
  path: /metrics
  labels:
    environment: production
    cluster: production
```

#### 告警集成
```yaml
# 告警配置示例
alerts:
  enabled: true
  webhook_url: "https://alertmanager.example.com/api/v1/alerts"
  severity_thresholds:
    critical: 8
    warning: 5
```

### 运维维护

#### 1. 健康检查脚本
```bash
#!/bin/bash
# 健康检查脚本
check_podflow_health() {
    # 检查进程状态
    if ! pgrep -f "podflow" > /dev/null; then
        echo "ERROR: podflow process not found"
        return 1
    fi
    
    # 检查API响应
    if ! curl -s http://localhost:8080/health > /dev/null; then
        echo "ERROR: API not responding"
        return 1
    fi
    
    echo "OK: podflow is healthy"
    return 0
}

# 定期执行健康检查
while true; do
    check_podflow_health
    sleep 30
done
```

#### 2. 日志轮转配置
```yaml
# 日志管理配置
logging:
  level: info
  file:
    path: /var/log/podflow/observer.log
    max_size: 100MB
    max_files: 5
    rotation: daily
  compression: gzip
```

#### 3. 性能监控
```bash
# 性能监控脚本
monitor_podflow_performance() {
    # CPU使用率
    cpu_usage=$(ps -p $(pgrep -f podflow) -o %cpu | awk '{sum+=$1} END {print sum}')
    
    # 内存使用
    memory_usage=$(ps -p $(pgrep -f podflow) -o %mem | awk '{sum+=$1} END {print sum}')
    
    # 文件描述符
    fd_count=$(lsof -p $(pgrep -f podflow) | wc -l)
    
    echo "CPU: ${cpu_usage}%, Memory: ${memory_usage}%, FDs: ${fd_count}"
}

# 添加到crontab
# */5 * * * * /usr/local/bin/monitor_podflow_performance >> /var/log/podflow-monitor.log
```

## API参考

### REST API端点

#### 诊断API
```http
POST /api/v1/diagnosis/trigger
Content-Type: application/json

{
  "pod_uid": "abc123",
  "namespace": "default", 
  "evidence_types": ["cgroup_contention", "network"],
  "metrics": ["cpu_usage", "memory_usage"],
  "window_secs": 60
}
```

#### 查询API
```http
GET /api/v1/diagnosis/tasks/{task_id}
Response:
{
  "task_id": "task-123",
  "status": "completed",
  "progress": 100,
  "result": {...}
}
```

#### 配置API
```http
GET /api/v1/config/rules
Response:
{
  "rules": [
    {
      "rule_id": "cpu-high",
      "evidence_type": "cgroup_contention",
      "metric_name": "cpu_usage",
      "operator": ">",
      "threshold": 80.0,
      "enabled": true
    }
  ]
}
```

## 版本信息

### 当前版本
```bash
podflow --version
```

### 版本兼容性
| 版本 | Kubernetes兼容性 | 主要特性 | 支持状态 |
|--------|------------------|----------|----------|
| v0.1.0 | 1.20+ | NRI v2, 案例库 | 稳定 |
| v0.2.0 | 1.22+ | NRI v3, AI集成 | 开发中 |

### 升级指南
1. **备份数据**: 导出重要配置和案例库
2. **测试兼容性**: 在测试环境验证新版本
3. **滚动升级**: 逐步替换生产实例
4. **监控回滚**: 准备快速回滚方案

---

## 获取帮助

- **GitHub Issues**: https://github.com/your-org/podflow/issues
- **文档**: https://github.com/your-org/podflow/docs
- **社区**: https://github.com/your-org/podflow/discussions

## 许可证

本项目采用 Apache License 2.0 许可证。详见 [LICENSE](../LICENSE) 文件。
