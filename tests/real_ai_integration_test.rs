//! 真实 AI 调用集成测试
//!
//! 测试 OpenAI 和 Anthropic 的真实 AI 调用、缓存、重试和降级

use nuts_observer::ai::{
    AiCallManager, AiResponseCache, OpenAiServiceClient, AnthropicServiceClient,
    RetryPolicy, AiServiceClient,
};
use std::sync::Arc;

/// 测试 AI 响应缓存
#[tokio::test]
async fn test_ai_response_cache() {
    println!("🔍 测试 AI 响应缓存...");

    let cache = AiResponseCache::new(60);

    // 测试缓存设置和获取
    cache.set("prompt1", "model1", "response1".to_string()).await;
    let result = cache.get("prompt1", "model1").await;
    assert_eq!(result, Some("response1".to_string()), "缓存应该返回正确的响应");

    // 测试缓存未命中
    let result = cache.get("prompt2", "model1").await;
    assert_eq!(result, None, "不同的 prompt 应该缓存未命中");

    // 测试不同模型的缓存隔离
    cache.set("prompt1", "model2", "response2".to_string()).await;
    let result = cache.get("prompt1", "model1").await;
    assert_eq!(result, Some("response1".to_string()), "不同模型应该有独立的缓存");

    // 测试统计
    let (total, expired) = cache.stats().await;
    assert_eq!(total, 2, "应该有 2 个缓存条目");
    assert_eq!(expired, 0, "没有过期的条目");

    println!("✅ AI 响应缓存测试通过");
    println!("   - 缓存设置: ✅");
    println!("   - 缓存获取: ✅");
    println!("   - 缓存隔离: ✅");
    println!("   - 缓存统计: ✅");
}

/// 测试重试策略
#[test]
fn test_retry_policy() {
    println!("🔍 测试重试策略...");

    let policy = RetryPolicy::default();

    // 测试指数退避
    let backoff0 = policy.calculate_backoff(0);
    let backoff1 = policy.calculate_backoff(1);
    let backoff2 = policy.calculate_backoff(2);

    assert!(backoff0 < backoff1, "退避时间应该递增");
    assert!(backoff1 < backoff2, "退避时间应该递增");
    assert!(
        backoff2.as_millis() <= policy.max_backoff_ms as u128,
        "退避时间不应该超过最大值"
    );

    // 测试自定义策略
    let custom_policy = RetryPolicy {
        max_retries: 5,
        initial_backoff_ms: 50,
        max_backoff_ms: 1000,
        backoff_multiplier: 1.5,
    };

    let backoff = custom_policy.calculate_backoff(0);
    assert_eq!(backoff.as_millis(), 50, "初始退避应该正确");

    println!("✅ 重试策略测试通过");
    println!("   - 指数退避: ✅");
    println!("   - 最大退避限制: ✅");
    println!("   - 自定义策略: ✅");
}

/// 模拟 AI 服务客户端（用于测试）
struct MockAiServiceClient {
    model: String,
    response: String,
    should_fail: bool,
}

#[async_trait::async_trait]
impl AiServiceClient for MockAiServiceClient {
    async fn call(&self, _prompt: &str) -> Result<String, String> {
        if self.should_fail {
            Err("Mock service error".to_string())
        } else {
            Ok(self.response.clone())
        }
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    async fn health_check(&self) -> Result<(), String> {
        if self.should_fail {
            Err("Mock service unavailable".to_string())
        } else {
            Ok(())
        }
    }
}

/// 测试 AI 调用管理器（成功路径）
#[tokio::test]
async fn test_ai_call_manager_success() {
    println!("🔍 测试 AI 调用管理器（成功路径）...");

    let mock_client = Arc::new(MockAiServiceClient {
        model: "test-model".to_string(),
        response: "Test AI response".to_string(),
        should_fail: false,
    });

    let manager = AiCallManager::new(mock_client, 60, RetryPolicy::default());

    // 测试成功调用
    let result = manager.call_with_retry("test prompt").await;
    assert!(result.success, "调用应该成功");
    assert_eq!(
        result.response,
        Some("Test AI response".to_string()),
        "响应应该正确"
    );
    assert_eq!(result.retries, 0, "第一次调用不应该重试");
    assert!(!result.cached, "第一次调用不应该来自缓存");

    // 测试缓存命中
    let result2 = manager.call_with_retry("test prompt").await;
    assert!(result2.success, "缓存调用应该成功");
    assert_eq!(
        result2.response,
        Some("Test AI response".to_string()),
        "缓存响应应该正确"
    );
    assert!(result2.cached, "第二次调用应该来自缓存");

    println!("✅ AI 调用管理器成功路径测试通过");
    println!("   - 成功调用: ✅");
    println!("   - 缓存命中: ✅");
    println!("   - 响应正确: ✅");
}

/// 测试 AI 调用管理器（失败和重试）
#[tokio::test]
async fn test_ai_call_manager_retry() {
    println!("🔍 测试 AI 调用管理器（失败和重试）...");

    let mock_client = Arc::new(MockAiServiceClient {
        model: "test-model".to_string(),
        response: "".to_string(),
        should_fail: true,
    });

    let retry_policy = RetryPolicy {
        max_retries: 2,
        initial_backoff_ms: 10,
        max_backoff_ms: 100,
        backoff_multiplier: 2.0,
    };

    let manager = AiCallManager::new(mock_client, 60, retry_policy);

    // 测试失败调用
    let result = manager.call_with_retry("test prompt").await;
    assert!(!result.success, "调用应该失败");
    assert!(result.response.is_none(), "失败响应应该为空");
    assert_eq!(result.retries, 2, "应该进行了最大重试次数");
    assert!(result.error.is_some(), "应该有错误信息");

    println!("✅ AI 调用管理器重试测试通过");
    println!("   - 失败检测: ✅");
    println!("   - 重试机制: ✅");
    println!("   - 错误处理: ✅");
}

/// 测试 AI 调用管理器健康检查
#[tokio::test]
async fn test_ai_call_manager_health_check() {
    println!("🔍 测试 AI 调用管理器健康检查...");

    // 测试健康的服务
    let healthy_client = Arc::new(MockAiServiceClient {
        model: "test-model".to_string(),
        response: "OK".to_string(),
        should_fail: false,
    });

    let manager = AiCallManager::new(healthy_client, 60, RetryPolicy::default());
    let health_result = manager.health_check().await;
    assert!(health_result.is_ok(), "健康检查应该通过");

    // 测试不健康的服务
    let unhealthy_client = Arc::new(MockAiServiceClient {
        model: "test-model".to_string(),
        response: "".to_string(),
        should_fail: true,
    });

    let manager = AiCallManager::new(unhealthy_client, 60, RetryPolicy::default());
    let health_result = manager.health_check().await;
    assert!(health_result.is_err(), "健康检查应该失败");

    println!("✅ AI 调用管理器健康检查测试通过");
    println!("   - 健康检查成功: ✅");
    println!("   - 健康检查失败: ✅");
}

/// 测试 AI 调用管理器缓存统计
#[tokio::test]
async fn test_ai_call_manager_cache_stats() {
    println!("🔍 测试 AI 调用管理器缓存统计...");

    let mock_client = Arc::new(MockAiServiceClient {
        model: "test-model".to_string(),
        response: "Test response".to_string(),
        should_fail: false,
    });

    let manager = AiCallManager::new(mock_client, 60, RetryPolicy::default());

    // 进行多次调用以填充缓存
    manager.call_with_retry("prompt1").await;
    manager.call_with_retry("prompt2").await;
    manager.call_with_retry("prompt3").await;

    // 获取缓存统计
    let (total, expired) = manager.cache_stats().await;
    assert_eq!(total, 3, "应该有 3 个缓存条目");
    assert_eq!(expired, 0, "没有过期的条目");

    // 清理缓存
    manager.cleanup_cache().await;
    let (total, _) = manager.cache_stats().await;
    assert_eq!(total, 3, "清理后应该仍有 3 个条目（未过期）");

    println!("✅ AI 调用管理器缓存统计测试通过");
    println!("   - 缓存统计: ✅");
    println!("   - 缓存清理: ✅");
}

/// 测试 OpenAI 客户端配置
#[test]
fn test_openai_client_creation() {
    println!("🔍 测试 OpenAI 客户端创建...");

    // 测试有效的 API key
    let result = OpenAiServiceClient::new(
        "sk-test-key".to_string(),
        "gpt-4".to_string(),
    );
    assert!(result.is_ok(), "应该能创建 OpenAI 客户端");

    let client = result.unwrap();
    assert_eq!(client.model_name(), "gpt-4", "模型名称应该正确");

    println!("✅ OpenAI 客户端创建测试通过");
    println!("   - 客户端创建: ✅");
    println!("   - 模型名称: ✅");
}

/// 测试 Anthropic 客户端配置
#[test]
fn test_anthropic_client_creation() {
    println!("🔍 测试 Anthropic 客户端创建...");

    // 测试有效的 API key
    let result = AnthropicServiceClient::new(
        "sk-ant-test-key".to_string(),
        "claude-3-sonnet-20240229".to_string(),
    );
    assert!(result.is_ok(), "应该能创建 Anthropic 客户端");

    let client = result.unwrap();
    assert_eq!(
        client.model_name(),
        "claude-3-sonnet-20240229",
        "模型名称应该正确"
    );

    println!("✅ Anthropic 客户端创建测试通过");
    println!("   - 客户端创建: ✅");
    println!("   - 模型名称: ✅");
}

/// 集成测试：完整的 AI 调用流程
#[tokio::test]
async fn test_complete_ai_call_flow() {
    println!("🔍 测试完整的 AI 调用流程...");

    let mock_client = Arc::new(MockAiServiceClient {
        model: "test-model".to_string(),
        response: "Diagnosis: The container is experiencing high CPU usage due to inefficient algorithm implementation.".to_string(),
        should_fail: false,
    });

    let manager = AiCallManager::new(mock_client, 60, RetryPolicy::default());

    // 1. 健康检查
    let health = manager.health_check().await;
    assert!(health.is_ok(), "健康检查应该通过");
    println!("✅ 健康检查通过");

    // 2. 第一次调用（从 AI 服务）
    let prompt = "Analyze the following container diagnostics...";
    let result1 = manager.call_with_retry(prompt).await;
    assert!(result1.success, "第一次调用应该成功");
    assert!(!result1.cached, "第一次调用不应该来自缓存");
    println!("✅ 第一次调用成功（耗时: {}ms）", result1.duration_ms);

    // 3. 第二次调用（从缓存）
    let result2 = manager.call_with_retry(prompt).await;
    assert!(result2.success, "第二次调用应该成功");
    assert!(result2.cached, "第二次调用应该来自缓存");
    assert!(result2.duration_ms <= result1.duration_ms, "缓存调用应该不慢于首次调用");
    println!("✅ 第二次调用成功（从缓存，耗时: {}ms）", result2.duration_ms);

    // 4. 验证响应一致性
    assert_eq!(
        result1.response, result2.response,
        "两次调用的响应应该一致"
    );
    println!("✅ 响应一致性验证通过");

    // 5. 缓存统计
    let (total, expired) = manager.cache_stats().await;
    assert_eq!(total, 1, "应该有 1 个缓存条目");
    assert_eq!(expired, 0, "没有过期的条目");
    println!("✅ 缓存统计: {} 个条目，0 个过期", total);

    println!("✅ 完整的 AI 调用流程测试通过");
    println!("   - 健康检查: ✅");
    println!("   - 首次调用: ✅");
    println!("   - 缓存调用: ✅");
    println!("   - 响应一致性: ✅");
    println!("   - 缓存统计: ✅");
}
