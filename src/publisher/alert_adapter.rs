//! 告警推送适配器
//!
//! 支持多渠道告警推送：Webhook、Kafka、Email等

use crate::types::alert::AlertInstance;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// 推送渠道类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertChannelType {
    Webhook,
    Kafka,
    Email,
    Sms,
    DingTalk,
    WeChat,
}

impl AlertChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertChannelType::Webhook => "webhook",
            AlertChannelType::Kafka => "kafka",
            AlertChannelType::Email => "email",
            AlertChannelType::Sms => "sms",
            AlertChannelType::DingTalk => "dingtalk",
            AlertChannelType::WeChat => "wechat",
        }
    }
}

/// 渠道配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: AlertChannelType,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(flatten)]
    pub config: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

/// Webhook 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    #[serde(default = "default_webhook_method")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_retry")]
    pub retry_count: u32,
    #[serde(default = "default_retry_interval")]
    pub retry_interval_ms: u64,
}

fn default_webhook_method() -> String {
    "POST".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_retry() -> u32 {
    3
}

fn default_retry_interval() -> u64 {
    1000
}

/// 告警推送适配器接口
#[async_trait]
pub trait AlertAdapter: Send + Sync {
    /// 适配器名称
    fn name(&self) -> &str;

    /// 渠道类型
    fn channel_type(&self) -> AlertChannelType;

    /// 是否可用
    fn is_available(&self) -> bool;

    /// 推送告警
    async fn push(&self, alert: &AlertInstance) -> Result<(), AdapterError>;

    /// 健康检查
    async fn health_check(&self) -> Result<(), AdapterError>;
}

/// 适配器错误
#[derive(Debug, Clone)]
pub struct AdapterError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl AdapterError {
    pub fn new(code: &str, message: &str, retryable: bool) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            retryable,
        }
    }

    pub fn non_retryable(code: &str, message: &str) -> Self {
        Self::new(code, message, false)
    }

    pub fn retryable(code: &str, message: &str) -> Self {
        Self::new(code, message, true)
    }
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AdapterError[{}]: {} (retryable: {})",
            self.code, self.message, self.retryable
        )
    }
}

impl std::error::Error for AdapterError {}

/// Webhook 推送适配器
pub struct WebhookAdapter {
    config: WebhookConfig,
    client: reqwest::Client,
    name: String,
}

impl WebhookAdapter {
    pub fn new(name: &str, config: WebhookConfig) -> Result<Self, AdapterError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| {
                AdapterError::non_retryable("CLIENT_BUILD_FAILED", &e.to_string())
            })?;

        Ok(Self {
            config,
            client,
            name: name.to_string(),
        })
    }

    /// 从配置创建
    pub fn from_config(name: &str, config: &HashMap<String, String>) -> Result<Self, AdapterError> {
        let url = config.get("url").ok_or_else(|| {
            AdapterError::non_retryable("MISSING_URL", "Webhook URL is required")
        })?;

        let mut headers = HashMap::new();
        if let Some(auth) = config.get("authorization") {
            headers.insert("Authorization".to_string(), auth.clone());
        }

        let webhook_config = WebhookConfig {
            url: url.clone(),
            method: config.get("method").cloned().unwrap_or_else(|| "POST".to_string()),
            headers,
            timeout_secs: config
                .get("timeout_secs")
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            retry_count: config
                .get("retry_count")
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
            retry_interval_ms: config
                .get("retry_interval_ms")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1000),
        };

        Self::new(name, webhook_config)
    }
}

#[async_trait]
impl AlertAdapter for WebhookAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> AlertChannelType {
        AlertChannelType::Webhook
    }

    fn is_available(&self) -> bool {
        // 简单检查 URL 是否有效
        !self.config.url.is_empty() && self.config.url.starts_with("http")
    }

    async fn push(&self, alert: &AlertInstance) -> Result<(), AdapterError> {
        let payload = build_webhook_payload(alert);
        let json_body = serde_json::to_string(&payload).map_err(|e| {
            AdapterError::non_retryable("SERIALIZE_FAILED", &e.to_string())
        })?;

        info!(
            "Pushing alert {} to webhook {}",
            alert.alert_id, self.config.url
        );

        let mut request = self
            .client
            .request(
                reqwest::Method::from_bytes(self.config.method.as_bytes()).unwrap_or(reqwest::Method::POST),
                &self.config.url,
            )
            .header("Content-Type", "application/json");

        // 添加自定义 headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        let response = request
            .body(json_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AdapterError::retryable("TIMEOUT", &e.to_string())
                } else {
                    AdapterError::retryable("REQUEST_FAILED", &e.to_string())
                }
            })?;

        let status = response.status();
        if status.is_success() {
            info!(
                "Alert {} pushed successfully to webhook (status: {})",
                alert.alert_id, status
            );
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            error!(
                "Failed to push alert {}: HTTP {} - {}",
                alert.alert_id, status, body
            );
            Err(AdapterError::non_retryable(
                &format!("HTTP_{}", status.as_u16()),
                &format!("HTTP error {}: {}", status, body),
            ))
        }
    }

    async fn health_check(&self) -> Result<(), AdapterError> {
        // 发送一个空的 GET 请求到 webhook 端点检查可用性
        // 或者检查 URL 格式正确性
        if !self.is_available() {
            return Err(AdapterError::non_retryable(
                "NOT_AVAILABLE",
                "Webhook URL is invalid or empty",
            ));
        }

        // 实际健康检查可以通过 HEAD 请求实现
        match self.client.head(&self.config.url).send().await {
            Ok(_) => Ok(()),
            Err(e) => Err(AdapterError::retryable("HEALTH_CHECK_FAILED", &e.to_string())),
        }
    }
}

/// Webhook 告警 Payload 结构
#[derive(Debug, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub version: String,
    pub alert_id: String,
    pub rule_id: String,
    pub task_id: String,
    pub severity: String,
    pub status: String,
    pub title: String,
    pub description: String,
    pub root_cause: String,
    pub suggestion: String,
    pub triggered_at: i64,
    pub labels: HashMap<String, String>,
    pub evidence_refs: Vec<String>,
    pub dedup_key: String,
}

fn build_webhook_payload(alert: &AlertInstance) -> WebhookPayload {
    WebhookPayload {
        version: "v1.0".to_string(),
        alert_id: alert.alert_id.clone(),
        rule_id: alert.rule_id.clone(),
        task_id: alert.task_id.clone(),
        severity: alert.severity.to_string(),
        status: alert.status.to_string(),
        title: alert.title.clone(),
        description: alert.description.clone(),
        root_cause: alert.root_cause.clone(),
        suggestion: alert.suggestion.clone(),
        triggered_at: alert.triggered_at,
        labels: alert.labels.clone(),
        evidence_refs: alert.evidence_refs.clone(),
        dedup_key: alert.dedup_key.clone(),
    }
}

/// 告警推送路由器
pub struct AlertRouter {
    /// 注册的适配器
    adapters: HashMap<String, Box<dyn AlertAdapter>>,
    /// 默认适配器
    default_adapter: Option<String>,
}

impl AlertRouter {
    /// 创建新的路由器
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
            default_adapter: None,
        }
    }

    /// 注册适配器
    pub fn register<A: AlertAdapter + 'static>(&mut self, name: &str, adapter: A) {
        let channel_type = adapter.channel_type();
        self.adapters.insert(name.to_string(), Box::new(adapter));
        info!("Registered alert adapter: {} ({:?})", name, channel_type);
    }

    /// 设置默认适配器
    pub fn set_default(&mut self, name: &str) -> Result<(), String> {
        if self.adapters.contains_key(name) {
            self.default_adapter = Some(name.to_string());
            Ok(())
        } else {
            Err(format!("Adapter {} not found", name))
        }
    }

    /// 获取适配器
    pub fn get_adapter(&self, name: &str) -> Option<&dyn AlertAdapter> {
        self.adapters.get(name).map(|a| a.as_ref())
    }

    /// 推送告警到指定渠道
    pub async fn push_to(
        &self,
        channel: &str,
        alert: &AlertInstance,
    ) -> Result<(), AdapterError> {
        let adapter = self.adapters.get(channel).ok_or_else(|| {
            AdapterError::non_retryable("CHANNEL_NOT_FOUND", &format!("Channel {} not found", channel))
        })?;

        adapter.push(alert).await
    }

    /// 推送告警到多个渠道
    pub async fn push_to_channels(
        &self,
        channels: &[String],
        alert: &AlertInstance,
    ) -> Vec<(String, Result<(), AdapterError>)> {
        let mut results = Vec::new();

        for channel in channels {
            let result = self.push_to(channel, alert).await;
            results.push((channel.clone(), result));
        }

        results
    }

    /// 使用默认适配器推送
    pub async fn push(&self, alert: &AlertInstance) -> Result<(), AdapterError> {
        let default = self.default_adapter.as_ref().ok_or_else(|| {
            AdapterError::non_retryable("NO_DEFAULT", "No default adapter configured")
        })?;

        self.push_to(default, alert).await
    }

    /// 执行健康检查
    pub async fn health_check(&self) -> HashMap<String, Result<(), AdapterError>> {
        let mut results = HashMap::new();

        for (name, adapter) in &self.adapters {
            let result = adapter.health_check().await;
            results.insert(name.clone(), result);
        }

        results
    }

    /// 获取可用适配器列表
    pub fn available_adapters(&self) -> Vec<(String, AlertChannelType)> {
        self.adapters
            .iter()
            .filter(|(_, a)| a.is_available())
            .map(|(name, a)| (name.clone(), a.channel_type()))
            .collect()
    }
}

impl Default for AlertRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建带重试的推送器
pub struct RetryingAlertPusher {
    router: AlertRouter,
    max_retries: u32,
    retry_interval_ms: u64,
}

impl RetryingAlertPusher {
    pub fn new(router: AlertRouter, max_retries: u32, retry_interval_ms: u64) -> Self {
        Self {
            router,
            max_retries,
            retry_interval_ms,
        }
    }

    /// 推送告警（带重试）
    pub async fn push_with_retry(
        &self,
        channel: &str,
        alert: &AlertInstance,
    ) -> Result<(), AdapterError> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                info!(
                    "Retrying alert {} push to {} (attempt {})",
                    alert.alert_id, channel, attempt
                );
                tokio::time::sleep(Duration::from_millis(self.retry_interval_ms)).await;
            }

            match self.router.push_to(channel, alert).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if !e.retryable || attempt == self.max_retries {
                        return Err(e);
                    }
                    warn!(
                        "Push failed (retryable), will retry: {} - {}",
                        e.code, e.message
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AdapterError::non_retryable("MAX_RETRIES", "Max retries exceeded")
        }))
    }
}

/// 创建默认的告警推送路由器（带 webhook 适配器）
pub fn create_default_router(webhook_url: &str) -> Result<AlertRouter, AdapterError> {
    let mut router = AlertRouter::new();

    // 创建 webhook 适配器
    let config = WebhookConfig {
        url: webhook_url.to_string(),
        method: "POST".to_string(),
        headers: HashMap::new(),
        timeout_secs: 30,
        retry_count: 3,
        retry_interval_ms: 1000,
    };

    let webhook = WebhookAdapter::new("default-webhook", config)?;
    router.register("webhook", webhook);
    router.set_default("webhook").map_err(|e| {
        AdapterError::non_retryable("SETUP_FAILED", &e)
    })?;

    Ok(router)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_config_default() {
        let config = WebhookConfig {
            url: "http://example.com/webhook".to_string(),
            method: default_webhook_method(),
            headers: HashMap::new(),
            timeout_secs: default_timeout(),
            retry_count: default_retry(),
            retry_interval_ms: default_retry_interval(),
        };

        assert_eq!(config.method, "POST");
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_alert_router() {
        let mut router = AlertRouter::new();
        
        // 创建一个 mock webhook 配置
        let config = WebhookConfig {
            url: "http://test.com/webhook".to_string(),
            method: "POST".to_string(),
            headers: HashMap::new(),
            timeout_secs: 5,
            retry_count: 1,
            retry_interval_ms: 100,
        };

        let adapter = WebhookAdapter::new("test-webhook", config).unwrap();
        router.register("test", adapter);

        assert!(router.get_adapter("test").is_some());
        assert!(router.get_adapter("nonexistent").is_none());
    }
}
