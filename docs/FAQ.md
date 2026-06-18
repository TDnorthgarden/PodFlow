# 常见问题解答

## 安装和部署

### Q: PodFlow 支持哪些操作系统？
A: 目前支持以下操作系统：
- **RHEL/CentOS**: 7.0+ (x86_64)
- **Ubuntu/Debian**: 18.04+ (x86_64)
- **openEuler**: 20.03+ (x86_64)

### Q: 如何在Kubernetes集群中部署？
A: 推荐使用DaemonSet部署方式：

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
```

### Q: 需要什么权限？
A: PodFlow需要以下权限：
- **bpf权限**: 需要CAP_SYS_ADMIN能力加载bpf程序
- **容器权限**: 需要privileged容器访问宿主机文件系统
- **NRI权限**: 需要访问containerd的NRI socket

权限配置：
```bash
# 检查当前用户权限
id podflow

# 添加bpf权限
sudo usermod -a -G bpf $USER

# 重新登录
newgrp bpf
```

### Q: 如何升级版本？
A: 升级步骤：

1. **备份配置**: 
   ```bash
   sudo cp /etc/podflow/config.yaml /etc/podflow/config.yaml.backup
   ```

2. **停止服务**:
   ```bash
   sudo systemctl stop podflow
   ```

3. **更新程序**:
   ```bash
   # 使用包管理器
   sudo yum update podflow
   # 或从源码构建
   git clone https://github.com/your-org/podflow.git
   cd podflow
   cargo build --release
   sudo cp target/release/podflow /usr/local/bin/
   ```

4. **验证升级**:
   ```bash
   podflow --version
   podflow status
   ```

5. **启动服务**:
   ```bash
   sudo systemctl start podflow
   ```

## 诊断相关问题

### Q: 诊断没有输出结果怎么办？
A: 检查以下几点：

1. **权限问题**: 
   ```bash
   ls -la /sys/fs/bpf/
   # 确保bpf文件系统可访问
   ```

2. **目标容器状态**:
   ```bash
   kubectl get pods -n <namespace>
   # 确认目标容器正在运行
   ```

3. **NRI连接**:
   ```bash
   # 检查containerd状态
   sudo systemctl status containerd
   # 检查NRI socket
   ls -la /var/run/containerd/nri.sock
   ```

4. **配置检查**:
   ```bash
   podflow --config /etc/podflow/config.yaml --dry-run
   # 验证配置文件格式
   ```

### Q: 误报率较高如何优化？
A: 优化策略：

1. **调整阈值**:
   ```bash
   # 查看当前阈值
   podflow config list-rules
   
   # 调整阈值
   podflow config set-rule --rule-id cpu-high --threshold 85
   ```

2. **增加证据类型**:
   ```yaml
   # 在配置文件中添加更多证据类型
   evidence_types: ["cgroup_contention", "network", "block_io", "syscall_latency"]
   ```

3. **启用AI适配器**:
   ```bash
   # 启用AI分析提高准确性
   podflow --enable-ai
   ```

### Q: 性能影响较大怎么办？
A: 性能优化建议：

1. **减少采集频率**:
   ```yaml
   performance:
     sampling_rate: 50  # 降低采样率
     max_containers: 20  # 限制监控容器数量
   ```

2. **过滤目标**:
   ```bash
   # 只监控关键命名空间
   podflow watch --namespace production --exclude-namespace test
   ```

3. **异步处理**:
   ```yaml
   async_processing: true
   buffer_size: 1000
   ```

## 配置相关问题

### Q: 如何配置多个数据源？
A: 多数据源配置：

```yaml
datasources:
  nri:
    enabled: true
    socket_path: /var/run/containerd/nri.sock
  
  bpftrace:
    enabled: true
    scripts_path: /opt/podflow/bpftrace
  
  ai_adapter:
    enabled: true
    endpoint: http://localhost:8080/ai
    api_key: ${AI_API_KEY}
```

### Q: 配置文件优先级？
A: 配置文件加载顺序：
1. **命令行参数** (最高优先级)
2. **环境变量** (中等优先级)
3. **配置文件** (最低优先级)

```bash
# 命令行指定
podflow --server http://prod-api:8080

# 环境变量
export PODFLOW_SERVER=http://prod-api:8080

# 配置文件
# /etc/podflow/config.yaml
```

## 集成相关问题

### Q: 如何与Prometheus集成？
A: Prometheus集成配置：

```yaml
# metrics.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'podflow'
    static_configs:
      - targets: ['localhost:9090']
    metrics_path: /metrics
    scrape_interval: 5s
```

### Q: 如何与告警系统集成？
A: 告警平台集成：

```yaml
# alertmanager.yml
global:
  smtp_smarthost: 'localhost:587'

route:
  group_by: ['alertname']
  group_wait: 10s
  repeat_interval: 12h
  receiver: 'web.hook'

receivers:
  - name: 'web.hook'
    webhook_configs:
      - url: 'http://alertmanager:9093/api/v1/alerts'
        send_resolved: true
```

## 故障排查

### Q: 如何排查内存泄漏问题？
A: 内存泄漏排查步骤：

1. **启用内存监控**:
   ```bash
   podflow watch --pod-uid abc123 --evidence-types memory,oom_events --interval 30
   ```

2. **分析内存趋势**:
   ```bash
   # 查看内存使用趋势
   kubectl top pod abc123 --containers
   ```

3. **检查内存配置**:
   ```bash
   # 查看容器内存限制
   kubectl describe pod abc123 | grep -i memory
   ```

### Q: 如何排查网络问题？
A: 网络问题排查：

1. **网络连通性测试**:
   ```bash
   # 测试Pod网络
   kubectl exec abc123 -- ping google.com
   
   # 测试服务发现
   nslookup kubernetes.default.svc.cluster.local
   ```

2. **网络监控**:
   ```bash
   podflow watch --pod-uid abc123 --evidence-types network --metrics latency_p99,packet_loss
   ```

3. **网络策略检查**:
   ```bash
   # 检查NetworkPolicy
   kubectl get networkpolicy -n <namespace>
   ```

## 开发相关问题

### Q: 如何添加新的探针？
A: 添加新探针步骤：

1. **创建bpftrace脚本**:
   ```bash
   mkdir /opt/podflow/bpftrace/custom_probe
   cd /opt/podflow/bpftrace/custom_probe
   # 创建新的探针脚本
   ```

2. **注册探针**:
   ```bash
   # 在配置文件中注册
   echo "probes:" >> /etc/podflow/config.yaml
   echo "  - name: custom_probe" >> /etc/podflow/config.yaml
   echo "    path: /opt/podflow/bpftrace/custom_probe/probe.bt" >> /etc/podflow/config.yaml
   ```

3. **重载配置**:
   ```bash
   sudo systemctl reload podflow
   ```

### Q: 如何自定义诊断规则？
A: 自定义规则开发：

1. **规则文件位置**: `/etc/podflow/rules/`
2. **规则格式**:
   ```yaml
   rules:
     - rule_id: "custom-cpu-spike"
       evidence_type: "cgroup_contention"
       metric_name: "cpu_usage"
       operator: ">"
       threshold: 90.0
       severity: 8
       description: "CPU使用率异常峰值"
   ```
3. **热加载**:
   ```bash
   # 无需重启，自动加载新规则
   podflow config reload-rules
   ```

## 获取帮助

### Q: 在哪里可以获得更多帮助？
A: 获取帮助的途径：

1. **命令行帮助**:
   ```bash
   podflow --help
   ```

2. **社区支持**:
   - GitHub Issues: https://github.com/your-org/podflow/issues
   - GitHub Discussions: https://github.com/your-org/podflow/discussions
   - 文档: https://github.com/your-org/podflow/docs

3. **企业支持**:
   - 邮件支持: support@your-company.com
   - 技术支持: 通过企业渠道获取

---

## 更新日志

### v0.1.0 (2024-XX-XX)
- 新增用户指南文档
- 新增FAQ文档
- 优化Kubernetes部署指南
- 完善错误处理和故障排查

### v0.2.0 (计划中)
- 添加更多最佳实践案例
- 增强AI集成文档
- 添加性能调优指南
