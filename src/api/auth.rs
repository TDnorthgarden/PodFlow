//! API Key 认证中间件
//!
//! 通过 `X-API-Key` 或 `Authorization: Bearer <key>` 请求头验证 API 访问。
//! 若配置中未设置 api_key，则跳过认证（开发模式）。

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

/// API Key 认证配置
#[derive(Clone)]
pub struct ApiKeyConfig {
    pub api_key: Option<String>,
    pub header_name: String,
}

/// API Key 认证中间件
///
/// 对 /health 和 /health/ready 路径跳过认证（供 K8s 探针使用）。
pub async fn api_key_auth(
    axum::extract::State(config): axum::extract::State<ApiKeyConfig>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = request.uri().path().to_string();

    // 健康检查端点跳过认证
    if path == "/health" || path == "/health/ready" || path == "/health/stats" {
        return Ok(next.run(request).await);
    }

    // 未配置 API Key 时跳过认证（开发模式）
    let expected_key = match &config.api_key {
        Some(key) if !key.is_empty() => key,
        _ => return Ok(next.run(request).await),
    };

    // 从请求头提取 API Key
    let provided_key = request
        .headers()
        .get(&config.header_name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            // 尝试从 Authorization: Bearer <key> 提取
            request
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .map(|s| s.to_string())
        });

    match provided_key {
        Some(key) if key == *expected_key => Ok(next.run(request).await),
        _ => {
            tracing::warn!(
                "[Auth] Unauthorized API access attempt to {} from {:?}",
                path,
                request
                    .headers()
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
            );
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}
