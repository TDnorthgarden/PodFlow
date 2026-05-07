//! 真实 AI 集成模块
//!
//! 支持真实 AI 服务调用（OpenAI、Anthropic）、缓存、重试和降级

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{info, warn, error};

/// AI 响应缓存条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub response: String,
    pub created_at: SystemTime,
    pub ttl_secs: u64,
}

impl CacheEntry {
    pub fn is_expired(&self) -> bool {
        match self.created_at.elapsed() {
            Ok(elapsed) => elapsed > Duration::from_secs(self.ttl_secs),
            Err(_) => true,
        }
    }
}

/// AI 响应缓存
pub struct AiResponseCache {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    default_ttl_secs: u64,
}

impl AiResponseCache {
    pub fn new(default_ttl_secs: u64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            default_ttl_secs,
        }
    }

    /// 生成缓存键
    fn generate_key(prompt: &str, model: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        prompt.hash(&mut hasher);
        model.hash(&mut hasher);
        format!("ai_cache_{}", hasher.finish())
    }

    /// 获取缓存
    pub async fn get(&self, prompt: &str, model: &str) -> Option<String> {
        let key = Self::generate_key(prompt, model);
        let cache = self.cache.read().await;

        if let Some(entry) = cache.get(&key) {
            if !entry.is_expired() {
                info!("AI cache hit for key: {}", key);
                return Some(entry.response.clone());
            }
        }
        None
    }

    /// 设置缓存
    pub async fn set(&self, prompt: &str, model: &str, response: String) {
        let key = Self::generate_key(prompt, model);
        let entry = CacheEntry {
            response,
            created_at: SystemTime::now(),
            ttl_secs: self.default_ttl_secs,
        };

        let mut cache = self.cache.write().await;
        cache.insert(key.clone(), entry);
        info!("AI cache set for key: {}", key);
    }

    /// 清理过期缓存
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, entry| !entry.is_expired());
        info!("AI cache cleanup completed");
    }

    /// 获取缓存统计
    pub async fn stats(&self) -> (usize, usize) {
        let cache = self.cache.read().await;
        let total = cache.len();
        let expired = cache.values().filter(|e| e.is_expired()).count();
        (total, expired)
    }
}

/// AI 重试策略
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff_ms: 100,
            max_backoff_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    /// 计算重试延迟
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        let backoff_ms = (self.initial_backoff_ms as f64
            * self.backoff_multiplier.powi(attempt as i32))
            .min(self.max_backoff_ms as f64) as u64;
        Duration::from_millis(backoff_ms)
    }
}

/// AI 调用结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCallResult {
    pub success: bool,
    pub response: Option<String>,
    pub error: Option<String>,
    pub retries: u32,
    pub duration_ms: u64,
    pub cached: bool,
}

impl AiCallResult {
    pub fn success(response: String, retries: u32, duration_ms: u64, cached: bool) -> Self {
        Self {
            success: true,
            response: Some(response),
            error: None,
            retries,
            duration_ms,
            cached,
        }
    }

    pub fn failure(error: String, retries: u32, duration_ms: u64) -> Self {
        Self {
            success: false,
            response: None,
            error: Some(error),
            retries,
            duration_ms,
            cached: false,
        }
    }
}

/// AI 服务客户端特征
#[async_trait]
pub trait AiServiceClient: Send + Sync {
    /// 调用 AI 服务
    async fn call(&self, prompt: &str) -> Result<String, String>;

    /// 获取模型名称
    fn model_name(&self) -> &str;

    /// 健康检查
    async fn health_check(&self) -> Result<(), String>;
}

/// OpenAI 客户端实现
pub struct OpenAiServiceClient {
    api_key: String,
    model: String,
    endpoint: String,
    client: reqwest::Client,
}

impl OpenAiServiceClient {
    pub fn new(api_key: String, model: String) -> Result<Self, String> {
        let endpoint = "https://api.openai.com/v1/chat/completions".to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            api_key,
            model,
            endpoint,
            client,
        })
    }
}

#[async_trait]
impl AiServiceClient for OpenAiServiceClient {
    async fn call(&self, prompt: &str) -> Result<String, String> {
        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a Kubernetes container diagnostics expert. Analyze the provided evidence and diagnosis to provide insights."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.3,
            "max_tokens": 2000,
        });

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No content in response".to_string())
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn health_check(&self) -> Result<(), String> {
        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 10,
        });

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Health check failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("Health check failed with status: {}", response.status()))
        }
    }
}

/// Anthropic Claude 客户端实现
pub struct AnthropicServiceClient {
    api_key: String,
    model: String,
    endpoint: String,
    client: reqwest::Client,
}

impl AnthropicServiceClient {
    pub fn new(api_key: String, model: String) -> Result<Self, String> {
        let endpoint = "https://api.anthropic.com/v1/messages".to_string();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            api_key,
            model,
            endpoint,
            client,
        })
    }
}

#[async_trait]
impl AiServiceClient for AnthropicServiceClient {
    async fn call(&self, prompt: &str) -> Result<String, String> {
        let request_body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2000,
            "system": "You are a Kubernetes container diagnostics expert. Analyze the provided evidence and diagnosis to provide insights.",
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
        });

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, text));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        json["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No content in response".to_string())
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn health_check(&self) -> Result<(), String> {
        let request_body = serde_json::json!({
            "model": self.model,
            "max_tokens": 10,
            "messages": [{"role": "user", "content": "ping"}],
        });

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Health check failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("Health check failed with status: {}", response.status()))
        }
    }
}

/// AI 调用管理器（支持缓存、重试、降级）
pub struct AiCallManager {
    client: Arc<dyn AiServiceClient>,
    cache: AiResponseCache,
    retry_policy: RetryPolicy,
}

impl AiCallManager {
    pub fn new(
        client: Arc<dyn AiServiceClient>,
        cache_ttl_secs: u64,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            client,
            cache: AiResponseCache::new(cache_ttl_secs),
            retry_policy,
        }
    }

    /// 调用 AI 服务（带缓存和重试）
    pub async fn call_with_retry(&self, prompt: &str) -> AiCallResult {
        let start = std::time::Instant::now();
        let model = self.client.model_name();

        // 1. 检查缓存
        if let Some(cached_response) = self.cache.get(prompt, model).await {
            let duration_ms = start.elapsed().as_millis() as u64;
            info!("AI call completed from cache in {}ms", duration_ms);
            return AiCallResult::success(cached_response, 0, duration_ms, true);
        }

        // 2. 重试调用
        let mut last_error = String::new();
        for attempt in 0..=self.retry_policy.max_retries {
            if attempt > 0 {
                let backoff = self.retry_policy.calculate_backoff(attempt - 1);
                warn!(
                    "AI call attempt {} failed, retrying after {:?}",
                    attempt, backoff
                );
                tokio::time::sleep(backoff).await;
            }

            match self.client.call(prompt).await {
                Ok(response) => {
                    // 缓存成功响应
                    self.cache.set(prompt, model, response.clone()).await;

                    let duration_ms = start.elapsed().as_millis() as u64;
                    info!(
                        "AI call completed successfully in {}ms (attempt {})",
                        duration_ms, attempt + 1
                    );
                    return AiCallResult::success(response, attempt, duration_ms, false);
                }
                Err(e) => {
                    last_error = e;
                    error!("AI call attempt {} failed: {}", attempt + 1, last_error);
                }
            }
        }

        // 3. 所有重试都失败
        let duration_ms = start.elapsed().as_millis() as u64;
        error!(
            "AI call failed after {} retries in {}ms",
            self.retry_policy.max_retries + 1,
            duration_ms
        );
        AiCallResult::failure(last_error, self.retry_policy.max_retries, duration_ms)
    }

    /// 健康检查
    pub async fn health_check(&self) -> Result<(), String> {
        self.client.health_check().await
    }

    /// 获取缓存统计
    pub async fn cache_stats(&self) -> (usize, usize) {
        self.cache.stats().await
    }

    /// 清理过期缓存
    pub async fn cleanup_cache(&self) {
        self.cache.cleanup_expired().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_entry_expiration() {
        let entry = CacheEntry {
            response: "test".to_string(),
            created_at: SystemTime::now() - Duration::from_secs(10),
            ttl_secs: 5,
        };
        assert!(entry.is_expired());

        let entry = CacheEntry {
            response: "test".to_string(),
            created_at: SystemTime::now(),
            ttl_secs: 60,
        };
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_retry_policy_backoff() {
        let policy = RetryPolicy::default();
        let backoff0 = policy.calculate_backoff(0);
        let backoff1 = policy.calculate_backoff(1);
        let backoff2 = policy.calculate_backoff(2);

        assert!(backoff0 < backoff1);
        assert!(backoff1 < backoff2);
        assert!(backoff2.as_millis() <= policy.max_backoff_ms as u128);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let cache = AiResponseCache::new(60);

        // 测试缓存设置和获取
        cache.set("prompt1", "model1", "response1".to_string()).await;
        let result = cache.get("prompt1", "model1").await;
        assert_eq!(result, Some("response1".to_string()));

        // 测试缓存未命中
        let result = cache.get("prompt2", "model1").await;
        assert_eq!(result, None);

        // 测试统计
        let (total, expired) = cache.stats().await;
        assert_eq!(total, 1);
        assert_eq!(expired, 0);
    }
}
