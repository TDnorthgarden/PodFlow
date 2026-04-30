# M3 告警平台推送端到端设计

## 架构概览

```
┌─────────────────────────────────────────────────────────────────────┐
│  诊断流程                                                            │
│  ──────────────────────────────────────────────────────────────    │
│  采集器 ──► 诊断引擎 ──► 告警规则引擎 ──► 告警推送器 ──► 告警平台   │
│  Evidence   Diagnosis     AlertRules      Publisher      Webhook   │
└─────────────────────────────────────────────────────────────────────┘
```

## 告警数据模型

### 1. 告警级别 (AlertSeverity)
```rust
pub enum AlertSeverity {
    Critical = 1,   // P0: 立即处理，影响核心业务
    High = 2,       // P1: 尽快处理，明显影响
    Medium = 3,     // P2: 计划处理，潜在风险
    Low = 4,        // P3: 观察处理，轻微异常
    Info = 5,       // P4: 仅记录，无需处理
}
```

### 2. 告警状态 (AlertStatus)
```rust
pub enum AlertStatus {
    Firing,     // 正在触发
    Resolved,   // 已恢复
    Acknowledged, // 已确认
    Suppressed, // 已抑制
}
```

### 3. 告警规则 (AlertRule)
```rust
pub struct AlertRule {
    pub rule_id: String,           // 规则唯一标识
    pub name: String,              // 规则名称
    pub description: String,       // 规则描述
    pub enabled: bool,             // 是否启用
    
    // 触发条件
    pub condition: AlertCondition,   // 触发条件
    pub severity: AlertSeverity,     // 告警级别
    
    // 通知配置
    pub channels: Vec<String>,       // 通知渠道 ["webhook", "email", "sms"]
    pub suppress_window_secs: u64,   // 抑制窗口（防抖动）
    
    // 元数据
    pub labels: HashMap<String, String>,  // 标签
    pub annotations: HashMap<String, String>, // 注释
}
```

### 4. 告警条件 (AlertCondition)
```rust
pub enum AlertCondition {
    // 基于诊断结论
    ConclusionMatch {
        conclusion_pattern: String,  // 结论匹配模式
        min_confidence: f64,         // 最小置信度
    },
    
    // 基于证据指标
    MetricThreshold {
        evidence_type: String,
        metric_name: String,
        operator: ThresholdOperator,
        threshold: f64,
        duration_secs: u64,          // 持续时长
    },
    
    // 基于诊断状态
    DiagnosisStatus {
        status: DiagnosisStatus,
        min_evidence_count: usize,
    },
}
```

### 5. 告警实例 (AlertInstance)
```rust
pub struct AlertInstance {
    pub alert_id: String,           // 告警实例ID
    pub rule_id: String,            // 关联规则ID
    pub task_id: String,            // 关联诊断任务
    
    pub severity: AlertSeverity,
    pub status: AlertStatus,
    
    pub title: String,              // 告警标题
    pub description: String,        // 告警详情
    pub root_cause: String,         // 根因分析
    pub suggestion: String,         // 处理建议
    
    pub triggered_at: i64,          // 触发时间
    pub resolved_at: Option<i64>,   // 恢复时间
    pub acknowledged_at: Option<i64>, // 确认时间
    
    pub labels: HashMap<String, String>,
    pub evidence_refs: Vec<String>, // 关联证据
}
```

## 告警规则引擎

### 核心功能
1. **规则匹配**: 将诊断结果与告警规则匹配
2. **告警生成**: 生成标准告警实例
3. **告警抑制**: 防止告警风暴
4. **状态管理**: 维护告警生命周期

### 工作流程
```
1. 接收诊断结果 (DiagnosisResult)
   ↓
2. 评估所有活跃规则
   - 检查结论匹配
   - 检查指标阈值
   - 检查诊断状态
   ↓
3. 生成告警实例 (AlertInstance)
   - 去重（基于 dedup_key）
   - 抑制检查
   - 级别判定
   ↓
4. 发送到推送队列
   - 按渠道分组
   - 优先级排序
   ↓
5. 异步推送到告警平台
```

## 告警推送配置

### 渠道配置
```yaml
# config/alert_channels.yaml
channels:
  - name: "webhook"
    type: "webhook"
    enabled: true
    config:
      url: "https://alert.example.com/webhook"
      method: "POST"
      headers:
        Authorization: "Bearer ${ALERT_TOKEN}"
      timeout_secs: 30
      retry: 3

  - name: "kafka"
    type: "kafka"
    enabled: false
    config:
      brokers: ["kafka:9092"]
      topic: "alerts"
      compression: "lz4"

  - name: "email"
    type: "email"
    enabled: false
    config:
      smtp_server: "smtp.example.com"
      from: "alerts@nuts.io"
      to: ["ops@example.com"]
```

### 规则配置
```yaml
# config/alert_rules.yaml
rules:
  - rule_id: "cpu-contention-p0"
    name: "CPU资源争抢严重"
    description: "容器CPU使用率超过95%，触发资源争抢告警"
    enabled: true
    condition:
      type: "conclusion_match"
      conclusion_pattern: "CPU*"
      min_confidence: 0.8
    severity: "critical"
    channels: ["webhook", "email"]
    suppress_window_secs: 300
    labels:
      category: "resource"
      team: "platform"
    annotations:
      runbook: "https://wiki.example.com/runbooks/cpu-contention"
      playbook: "https://wiki.example.com/playbooks/cpu-throttling"

  - rule_id: "memory-leak-p1"
    name: "内存泄漏检测"
    description: "内存持续增长，疑似泄漏"
    enabled: true
    condition:
      type: "metric_threshold"
      evidence_type: "memory"
      metric_name: "growth_rate"
      operator: "greater_than"
      threshold: 10.0
      duration_secs: 60
    severity: "high"
    channels: ["webhook"]
    suppress_window_secs: 600
```

## 端到端流程

### 1. 采集触发
```bash
nuts-observer trigger \
  --pod-name myapp-xxx \
  --evidence-types cpu,memory \
  --window-secs 60
```

### 2. 诊断分析
诊断引擎分析证据，生成结论：
- 结论ID: cpu-contention-001
- 置信度: 0.92
- 证据强度: Strong

### 3. 告警规则匹配
```rust
// 匹配规则 cpu-contention-p0
if conclusion.matches("CPU*") && confidence > 0.8 {
    alert = AlertInstance::new(
        severity: Critical,
        title: "CPU资源争抢严重",
        ...
    )
}
```

### 4. 告警推送
```rust
// 推送到 webhook
POST https://alert.example.com/webhook
Content-Type: application/json
X-Dedup-Key: task-123-4567890

{
    "alert_id": "alert-xxx",
    "rule_id": "cpu-contention-p0",
    "task_id": "task-123",
    "severity": "critical",
    "status": "firing",
    "title": "CPU资源争抢严重",
    "description": "...",
    "timestamp": 1234567890
}
```

## 实现计划

### Phase 1: 数据模型和规则引擎
- [ ] 定义 AlertSeverity, AlertStatus, AlertRule, AlertInstance
- [ ] 实现 AlertRuleEngine
- [ ] 实现规则匹配逻辑

### Phase 2: 规则配置加载
- [ ] YAML 配置解析
- [ ] 热更新支持
- [ ] 规则验证

### Phase 3: 告警生命周期管理
- [ ] 告警生成
- [ ] 告警抑制
- [ ] 告警恢复

### Phase 4: 推送集成
- [ ] Webhook 推送
- [ ] Kafka 推送
- [ ] 多渠道路由

### Phase 5: 测试和文档
- [ ] 单元测试
- [ ] 集成测试
- [ ] 端到端测试

## 验收标准

1. **功能完整**: 诊断结果能自动触发告警
2. **多渠道支持**: 支持 Webhook 和 Kafka
3. **规则灵活**: 支持结论匹配和指标阈值
4. **告警抑制**: 防止告警风暴
5. **高可用**: 推送失败有重试和降级

## 后续优化

- **智能降噪**: AI 学习减少误报
- **根因关联**: 相关告警聚合
- **自愈联动**: 自动执行修复脚本
