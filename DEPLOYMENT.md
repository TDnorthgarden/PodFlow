# 部署和配置指南

## 目录

1. [系统要求](#系统要求)
2. [安装](#安装)
3. [配置](#配置)
4. [部署](#部署)
5. [监控](#监控)
6. [故障排查](#故障排查)

---

## 系统要求

### 硬件要求

| 资源 | 最小 | 推荐 |
|------|------|------|
| CPU | 2 核 | 4 核+ |
| 内存 | 512MB | 2GB+ |
| 磁盘 | 1GB | 10GB+ |
| 网络 | 100Mbps | 1Gbps+ |

### 软件要求

| 软件 | 版本 | 说明 |
|------|------|------|
| Linux | 5.8+ | 支持 eBPF |
| Containerd | 1.6+ | 支持 NRI |
| Rust | 1.82+ | 编译依赖 |
| Go | 1.19+ | 可选，用于 bpftrace |

### 权限要求

- Root 权限（用于 eBPF 和 Containerd 访问）
- 或者 CAP_SYS_ADMIN、CAP_SYS_RESOURCE 等能力

---

## 安装

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/example/nuts-observer.git
cd nuts-observer

# 编译发布版本
cargo build --release

# 二进制位置
ls -la target/release/nuts-*
```

### 使用 Docker

```bash
# 构建镜像
docker build -t nuts-observer:latest .

# 运行容器
docker run -d \
  --name nuts-observer \
  --privileged \
  -v /run/containerd/containerd.sock:/run/containerd/containerd.sock \
  -v /etc/nuts-observer:/etc/nuts-observer \
  nuts-observer:latest
```

### 使用 Kubernetes

```bash
# 创建命名空间
kubectl create namespace nuts-system

# 创建配置
kubectl create configmap nuts-observer-config \
  --from-file=config.yaml \
  -n nuts-system

# 部署 DaemonSet
kubectl apply -f k8s/daemonset.yaml
```

---

## 配置

### 环境变量

```bash
# 日志级别
export RUST_LOG="info"
export RUST_LOG="debug,nuts_observer=trace"

# AI 配置
export OPENAI_API_KEY="sk-..."
export OPENAI_MODEL="gpt-4"
export ANTHROPIC_API_KEY="sk-ant-..."

# 性能配置
export NUTS_CACHE_SIZE="1000"
export NUTS_BATCH_SIZE="100"
export NUTS_WORKER_THREADS="4"

# 告警配置
export NUTS_WEBHOOK_URL="http://localhost:8080/alerts"
export NUTS_KAFKA_BROKERS="localhost:9092"
```

### 配置文件

创建 `/etc/nuts-observer/config.yaml`：

```yaml
# 服务配置
server:
  listen_addr: "0.0.0.0:8080"
  health_check_interval_secs: 30

# 采集器配置
collector:
  socket_path: "/tmp/nuts-collector.sock"
  allowed_uids: [0, 1000, 1001]
  timeout_secs: 30
  max_retries: 3

# 诊断配置
diagnosis:
  rule_check_interval_ms: 5000
  max_evidence_retention: 1000
  confidence_threshold: 0.7

# AI 配置
ai:
  enabled: true
  provider: "openai"  # openai 或 anthropic
  model: "gpt-4"
  endpoint: "https://api.openai.com/v1/chat/completions"
  timeout_secs: 60
  max_retries: 3
  cache_ttl_secs: 3600
  fallback_mode: "keep_original"  # keep_original, reduce_confidence, mark_for_review

# 告警配置
alert:
  enabled: true
  channels:
    - type: "webhook"
      url: "http://localhost:8080/alerts"
      timeout_secs: 10
    - type: "kafka"
      brokers: ["localhost:9092"]
      topic: "nuts-alerts"
    - type: "email"
      smtp_server: "smtp.example.com"
      smtp_port: 587
      from_addr: "alerts@example.com"

# 性能配置
performance:
  cache_size: 1000
  batch_size: 100
  worker_threads: 4
  max_concurrent_tasks: 100

# 日志配置
logging:
  level: "info"
  format: "json"  # json 或 text
  output: "stdout"  # stdout 或 file
  file_path: "/var/log/nuts-observer.log"
  max_size_mb: 100
  max_backups: 10
```

### 规则配置

创建 `/etc/nuts-observer/rules.yaml`：

```yaml
rules:
  - id: "high_memory_usage"
    type: "threshold"
    evidence_type: "cgroup_contention"
    metric: "memory_usage_percent"
    threshold: 80.0
    severity: "warning"
    title: "High memory usage detected"
    description: "Memory usage is above 80%"

  - id: "memory_trend"
    type: "trend"
    evidence_type: "cgroup_contention"
    metric: "memory_usage_percent"
    window_size: 10
    trend_threshold: 0.5
    severity: "warning"
    title: "Memory usage trending up"
    description: "Memory usage is continuously increasing"

  - id: "io_latency_spike"
    type: "correlation"
    evidence_types: ["block_io", "syscall_latency"]
    metrics: ["io_latency_p99_ms", "syscall_latency_p99_ms"]
    correlation_threshold: 0.8
    severity: "critical"
    title: "I/O latency spike detected"
    description: "Block I/O and syscall latency are both high"
```

---

## 部署

### 本地部署

```bash
# 1. 启动采集守护进程
sudo ./target/release/nuts-collector-daemon \
  --config /etc/nuts-observer/config.yaml

# 2. 启动主服务
./target/release/nuts-observer \
  --config /etc/nuts-observer/config.yaml

# 3. 验证服务
curl http://localhost:8080/health
```

### Docker Compose 部署

```yaml
version: '3.8'

services:
  nuts-observer:
    image: nuts-observer:latest
    container_name: nuts-observer
    privileged: true
    ports:
      - "8080:8080"
    volumes:
      - /run/containerd/containerd.sock:/run/containerd/containerd.sock
      - ./config.yaml:/etc/nuts-observer/config.yaml
      - ./rules.yaml:/etc/nuts-observer/rules.yaml
    environment:
      - RUST_LOG=info
      - OPENAI_API_KEY=${OPENAI_API_KEY}
    networks:
      - nuts-network

  prometheus:
    image: prom/prometheus:latest
    container_name: prometheus
    ports:
      - "9090:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    networks:
      - nuts-network

  grafana:
    image: grafana/grafana:latest
    container_name: grafana
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    networks:
      - nuts-network

networks:
  nuts-network:
    driver: bridge
```

### Kubernetes 部署

```yaml
---
apiVersion: v1
kind: Namespace
metadata:
  name: nuts-system

---
apiVersion: v1
kind: ConfigMap
metadata:
  name: nuts-observer-config
  namespace: nuts-system
data:
  config.yaml: |
    server:
      listen_addr: "0.0.0.0:8080"
    collector:
      socket_path: "/tmp/nuts-collector.sock"
      allowed_uids: [0]

---
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: nuts-observer
  namespace: nuts-system
spec:
  selector:
    matchLabels:
      app: nuts-observer
  template:
    metadata:
      labels:
        app: nuts-observer
    spec:
      hostNetwork: true
      hostPID: true
      containers:
      - name: nuts-observer
        image: nuts-observer:latest
        imagePullPolicy: IfNotPresent
        securityContext:
          privileged: true
          capabilities:
            add:
              - SYS_ADMIN
              - SYS_RESOURCE
              - NET_ADMIN
        ports:
        - containerPort: 8080
          name: http
        volumeMounts:
        - name: containerd-sock
          mountPath: /run/containerd/containerd.sock
        - name: config
          mountPath: /etc/nuts-observer
        - name: sys
          mountPath: /sys
        - name: proc
          mountPath: /proc
        env:
        - name: RUST_LOG
          value: "info"
        - name: OPENAI_API_KEY
          valueFrom:
            secretKeyRef:
              name: nuts-observer-secrets
              key: openai-api-key
        resources:
          requests:
            cpu: 100m
            memory: 256Mi
          limits:
            cpu: 500m
            memory: 1Gi
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 30
          periodSeconds: 10
        readinessProbe:
          httpGet:
            path: /health
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 5
      volumes:
      - name: containerd-sock
        hostPath:
          path: /run/containerd/containerd.sock
      - name: config
        configMap:
          name: nuts-observer-config
      - name: sys
        hostPath:
          path: /sys
      - name: proc
        hostPath:
          path: /proc
      tolerations:
      - effect: NoSchedule
        operator: Exists
```

---

## 监控

### Prometheus 集成

```yaml
# prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'nuts-observer'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
```

### Grafana 仪表板

导入预定义的 Grafana 仪表板：

```bash
# 获取仪表板 ID
curl -X GET http://localhost:3000/api/dashboards/db/nuts-observer

# 或者手动导入 JSON
curl -X POST http://localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  -d @grafana-dashboard.json
```

### 关键指标

| 指标 | 说明 |
|------|------|
| `nri_events_total` | 处理的 NRI 事件总数 |
| `nri_event_processing_duration_microseconds` | 事件处理延迟 |
| `nri_attribution_cache_hit_rate` | 缓存命中率 |
| `nuts_diagnosis_conclusions_total` | 诊断结论总数 |
| `nuts_ai_calls_total` | AI 调用总数 |
| `nuts_alerts_sent_total` | 发送的告警总数 |

---

## 故障排查

### 常见问题

**Q: 采集器无法启动**

A: 检查以下几点：
```bash
# 1. 检查权限
sudo -l

# 2. 检查 Containerd
systemctl status containerd
systemctl restart containerd

# 3. 检查 NRI 配置
cat /etc/containerd/config.toml | grep -A 5 "\[plugins.nri\]"

# 4. 查看日志
RUST_LOG=debug ./nuts-collector-daemon
```

**Q: AI 调用失败**

A:
```bash
# 1. 检查 API Key
echo $OPENAI_API_KEY

# 2. 测试连接
curl -X POST https://api.openai.com/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"test"}]}'

# 3. 增加超时
# 在 config.yaml 中设置 timeout_secs: 120
```

**Q: 诊断结果为空**

A:
```bash
# 1. 检查证据采集
curl http://localhost:8080/v1/diagnostics:trigger \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{...}'

# 2. 查看诊断日志
RUST_LOG=nuts_observer::diagnosis=debug

# 3. 检查规则配置
cat /etc/nuts-observer/rules.yaml
```

### 调试命令

```bash
# 启用详细日志
RUST_LOG=debug,nuts_observer=trace ./nuts-observer

# 检查性能
cargo bench --bench performance_benchmarks

# 运行测试
cargo test --lib

# 代码检查
cargo clippy --all-targets -- -D warnings

# 格式检查
cargo fmt --check
```

---

## 升级

### 升级步骤

```bash
# 1. 备份配置
cp -r /etc/nuts-observer /etc/nuts-observer.backup

# 2. 停止服务
systemctl stop nuts-observer

# 3. 编译新版本
cargo build --release

# 4. 备份旧二进制
cp /usr/local/bin/nuts-observer /usr/local/bin/nuts-observer.old

# 5. 安装新二进制
sudo cp target/release/nuts-observer /usr/local/bin/

# 6. 启动服务
systemctl start nuts-observer

# 7. 验证
curl http://localhost:8080/health
```

---

## 性能调优

### 缓存优化

```yaml
performance:
  cache_size: 5000  # 增加缓存大小
  cache_ttl_secs: 3600  # 缓存过期时间
```

### 批处理优化

```yaml
performance:
  batch_size: 500  # 增加批处理大小
  batch_timeout_ms: 1000  # 批处理超时
```

### 并发优化

```yaml
performance:
  worker_threads: 8  # 增加工作线程
  max_concurrent_tasks: 500  # 增加并发任务数
```

---

## 安全建议

1. **访问控制**：限制 API 访问 IP
2. **认证**：启用 API 认证
3. **加密**：使用 HTTPS
4. **日志**：启用审计日志
5. **更新**：定期更新依赖

---

## 支持

- 📖 [项目文档](./CLAUDE.md)
- 🐛 [Issue Tracker](https://github.com/example/nuts-observer/issues)
- 💬 [讨论](https://github.com/example/nuts-observer/discussions)
