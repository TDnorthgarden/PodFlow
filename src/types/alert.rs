//! 告警系统类型定义
//!
//! 定义告警规则、告警实例、告警级别等核心数据结构

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 告警级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// P0: 立即处理，影响核心业务
    Critical = 1,
    /// P1: 尽快处理，明显影响
    High = 2,
    /// P2: 计划处理，潜在风险
    Medium = 3,
    /// P3: 观察处理，轻微异常
    Low = 4,
    /// P4: 仅记录，无需处理
    Info = 5,
}

impl AlertSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Critical => "critical",
            AlertSeverity::High => "high",
            AlertSeverity::Medium => "medium",
            AlertSeverity::Low => "low",
            AlertSeverity::Info => "info",
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(AlertSeverity::Critical),
            2 => Some(AlertSeverity::High),
            3 => Some(AlertSeverity::Medium),
            4 => Some(AlertSeverity::Low),
            5 => Some(AlertSeverity::Info),
            _ => None,
        }
    }
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 告警状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    /// 正在触发
    Firing,
    /// 已恢复
    Resolved,
    /// 已确认
    Acknowledged,
    /// 已抑制
    Suppressed,
}

impl AlertStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertStatus::Firing => "firing",
            AlertStatus::Resolved => "resolved",
            AlertStatus::Acknowledged => "acknowledged",
            AlertStatus::Suppressed => "suppressed",
        }
    }
}

impl std::fmt::Display for AlertStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// 阈值操作符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThresholdOperator {
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Equals,
    NotEquals,
}

impl ThresholdOperator {
    pub fn evaluate(&self, value: f64, threshold: f64) -> bool {
        match self {
            ThresholdOperator::GreaterThan => value > threshold,
            ThresholdOperator::LessThan => value < threshold,
            ThresholdOperator::GreaterThanOrEqual => value >= threshold,
            ThresholdOperator::LessThanOrEqual => value <= threshold,
            ThresholdOperator::Equals => (value - threshold).abs() < f64::EPSILON,
            ThresholdOperator::NotEquals => (value - threshold).abs() >= f64::EPSILON,
        }
    }
}

/// 告警条件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AlertCondition {
    /// 基于诊断结论匹配
    ConclusionMatch {
        /// 结论匹配模式（支持通配符）
        conclusion_pattern: String,
        /// 最小置信度 (0.0 - 1.0)
        min_confidence: f64,
    },
    /// 基于证据指标阈值
    MetricThreshold {
        /// 证据类型
        evidence_type: String,
        /// 指标名称
        metric_name: String,
        /// 操作符
        operator: ThresholdOperator,
        /// 阈值
        threshold: f64,
        /// 持续时长（秒）
        duration_secs: u64,
    },
    /// 基于诊断状态
    DiagnosisStatus {
        /// 目标诊断状态
        status: String,
        /// 最小证据数量
        min_evidence_count: usize,
    },
    /// 组合条件（与）
    And { conditions: Vec<AlertCondition> },
    /// 组合条件（或）
    Or { conditions: Vec<AlertCondition> },
}

/// 告警规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    /// 规则唯一标识
    pub rule_id: String,
    /// 规则名称
    pub name: String,
    /// 规则描述
    #[serde(default)]
    pub description: String,
    /// 是否启用
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 触发条件
    pub condition: AlertCondition,
    /// 告警级别
    pub severity: AlertSeverity,
    /// 通知渠道列表
    #[serde(default)]
    pub channels: Vec<String>,
    /// 抑制窗口（秒）
    #[serde(default = "default_suppress_window")]
    pub suppress_window_secs: u64,
    /// 标签
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// 注释（额外信息）
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

fn default_suppress_window() -> u64 {
    300 // 5分钟默认抑制窗口
}

impl AlertRule {
    /// 创建新规则
    pub fn new(
        rule_id: &str,
        name: &str,
        condition: AlertCondition,
        severity: AlertSeverity,
    ) -> Self {
        Self {
            rule_id: rule_id.to_string(),
            name: name.to_string(),
            description: String::new(),
            enabled: true,
            condition,
            severity,
            channels: vec!["webhook".to_string()],
            suppress_window_secs: 300,
            labels: HashMap::new(),
            annotations: HashMap::new(),
        }
    }

    /// 添加标签
    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    /// 添加注释
    pub fn with_annotation(mut self, key: &str, value: &str) -> Self {
        self.annotations.insert(key.to_string(), value.to_string());
        self
    }

    /// 设置通知渠道
    pub fn with_channels(mut self, channels: Vec<String>) -> Self {
        self.channels = channels;
        self
    }
}

/// 告警实例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertInstance {
    /// 告警实例ID
    pub alert_id: String,
    /// 关联规则ID
    pub rule_id: String,
    /// 关联诊断任务ID
    pub task_id: String,
    /// 告警级别
    pub severity: AlertSeverity,
    /// 告警状态
    pub status: AlertStatus,
    /// 告警标题
    pub title: String,
    /// 告警描述
    #[serde(default)]
    pub description: String,
    /// 根因分析
    #[serde(default)]
    pub root_cause: String,
    /// 处理建议
    #[serde(default)]
    pub suggestion: String,
    /// 触发时间（Unix时间戳秒）
    pub triggered_at: i64,
    /// 恢复时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<i64>,
    /// 确认时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_at: Option<i64>,
    /// 标签
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// 关联证据ID列表
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    /// 去重键
    pub dedup_key: String,
}

impl AlertInstance {
    /// 生成去重键
    pub fn generate_dedup_key(rule_id: &str, task_id: &str) -> String {
        format!("{}-{}", rule_id, task_id)
    }

    /// 标记为已恢复
    pub fn resolve(&mut self) {
        self.status = AlertStatus::Resolved;
        self.resolved_at = Some(chrono::Utc::now().timestamp());
    }

    /// 标记为已确认
    pub fn acknowledge(&mut self) {
        self.status = AlertStatus::Acknowledged;
        self.acknowledged_at = Some(chrono::Utc::now().timestamp());
    }

    /// 检查是否仍在抑制窗口内
    pub fn is_in_suppress_window(&self, suppress_secs: u64) -> bool {
        let now = chrono::Utc::now().timestamp();
        let elapsed = now - self.triggered_at;
        elapsed < suppress_secs as i64
    }
}

/// 告警规则配置集合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRuleConfig {
    /// 版本
    #[serde(default = "default_version")]
    pub version: String,
    /// 规则列表
    pub rules: Vec<AlertRule>,
}

fn default_version() -> String {
    "1.0".to_string()
}

impl AlertRuleConfig {
    /// 创建空配置
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            rules: Vec::new(),
        }
    }

    /// 添加规则
    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    /// 获取启用的规则
    pub fn enabled_rules(&self) -> Vec<&AlertRule> {
        self.rules.iter().filter(|r| r.enabled).collect()
    }

    /// 从YAML加载
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    /// 保存为YAML
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }
}

impl Default for AlertRuleConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// 告警评估结果
#[derive(Debug, Clone)]
pub enum AlertEvaluationResult {
    /// 触发告警
    Firing(AlertInstance),
    /// 未触发（条件不满足）
    NotFiring,
    /// 被抑制
    Suppressed(String),
    /// 错误
    Error(String),
}

/// 告警通知消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertNotification {
    /// 消息版本
    pub version: String,
    /// 通知ID
    pub notification_id: String,
    /// 告警实例
    pub alert: AlertInstance,
    /// 通知渠道
    pub channel: String,
    /// 发送时间
    pub sent_at: i64,
    /// 重试次数
    pub retry_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_severity_ordering() {
        assert!(AlertSeverity::Critical < AlertSeverity::High);
        assert!(AlertSeverity::High < AlertSeverity::Medium);
        assert!(AlertSeverity::Medium < AlertSeverity::Low);
        assert!(AlertSeverity::Low < AlertSeverity::Info);
    }

    #[test]
    fn test_threshold_operator() {
        let op = ThresholdOperator::GreaterThan;
        assert!(op.evaluate(10.0, 5.0));
        assert!(!op.evaluate(5.0, 10.0));

        let op = ThresholdOperator::LessThan;
        assert!(op.evaluate(5.0, 10.0));
        assert!(!op.evaluate(10.0, 5.0));
    }

    #[test]
    fn test_alert_rule_builder() {
        let rule = AlertRule::new(
            "test-rule",
            "Test Rule",
            AlertCondition::ConclusionMatch {
                conclusion_pattern: "CPU*".to_string(),
                min_confidence: 0.8,
            },
            AlertSeverity::High,
        )
        .with_label("category", "test")
        .with_annotation("runbook", "https://example.com/runbook");

        assert_eq!(rule.rule_id, "test-rule");
        assert_eq!(rule.labels.get("category"), Some(&"test".to_string()));
        assert!(rule.enabled);
    }
}
