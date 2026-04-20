//! 规则管理 HTTP API 模块
//!
//! 提供诊断规则的动态 CRUD 接口
//!
//! 端点：
//! - GET    /v1/rules              列出所有规则
//! - GET    /v1/rules/:rule_id     获取单个规则
//! - POST   /v1/rules              创建新规则
//! - PUT    /v1/rules/:rule_id     更新规则
//! - DELETE /v1/rules/:rule_id     删除规则
//! - POST   /v1/rules/reload       重新加载默认规则
//! - GET    /v1/rules/status      获取规则管理器状态
//! - POST   /v1/rules/import       导入规则（YAML）
//! - GET    /v1/rules/export       导出规则（YAML）
//! - DELETE /v1/rules/clear        清空所有规则

use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::diagnosis::rule_manager::{DynamicRuleDef, RuleManager, RuleUpdates, RuleManagerStatus};

/// API 共享状态
pub struct RuleApiState {
    pub rule_manager: RuleManager,
}

impl RuleApiState {
    pub fn new(rule_manager: RuleManager) -> Self {
        Self { rule_manager }
    }
}

/// 创建规则管理路由
pub fn router(state: Arc<RuleApiState>) -> Router {
    Router::new()
        .route("/v1/rules", get(list_rules_handler).post(create_rule_handler))
        .route("/v1/rules/:rule_id", get(get_rule_handler).put(update_rule_handler).delete(delete_rule_handler))
        .route("/v1/rules/status", get(status_handler))
        .route("/v1/rules/reload", post(reload_defaults_handler))
        .route("/v1/rules/import", post(import_rules_handler))
        .route("/v1/rules/export", get(export_rules_handler))
        .route("/v1/rules/clear", delete(clear_rules_handler))
        .with_state(state)
}

/// 规则列表响应
#[derive(Debug, Serialize)]
pub struct ListRulesResponse {
    pub rules: Vec<DynamicRuleDef>,
    pub total: usize,
}

/// 创建规则请求
#[derive(Debug, Deserialize)]
pub struct CreateRuleRequest {
    pub rule: DynamicRuleDef,
}

/// 更新规则请求
#[derive(Debug, Deserialize)]
pub struct UpdateRuleRequest {
    pub name: Option<String>,
    pub threshold: Option<f64>,
    pub operator: Option<String>,
    pub conclusion_title: Option<String>,
    pub severity: Option<u8>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
}

/// 通用 API 响应包装
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// 列出所有规则
async fn list_rules_handler(
    State(state): State<Arc<RuleApiState>>,
) -> Json<ApiResponse<ListRulesResponse>> {
    let rules = state.rule_manager.list_rules().await;
    let total = rules.len();
    
    Json(ApiResponse::success(ListRulesResponse { rules, total }))
}

/// 获取单个规则
async fn get_rule_handler(
    State(state): State<Arc<RuleApiState>>,
    Path(rule_id): Path<String>,
) -> Json<ApiResponse<DynamicRuleDef>> {
    match state.rule_manager.get_rule(&rule_id).await {
        Some(rule) => Json(ApiResponse::success(rule)),
        None => Json(ApiResponse::error(format!("Rule not found: {}", rule_id))),
    }
}

/// 创建新规则
async fn create_rule_handler(
    State(state): State<Arc<RuleApiState>>,
    Json(request): Json<CreateRuleRequest>,
) -> Json<ApiResponse<DynamicRuleDef>> {
    let rule = request.rule;
    
    match state.rule_manager.add_rule(rule.clone()).await {
        Ok(_) => Json(ApiResponse::success(rule)),
        Err(e) => Json(ApiResponse::error(format!("Failed to create rule: {}", e))),
    }
}

/// 更新规则
async fn update_rule_handler(
    State(state): State<Arc<RuleApiState>>,
    Path(rule_id): Path<String>,
    Json(request): Json<UpdateRuleRequest>,
) -> Json<ApiResponse<DynamicRuleDef>> {
    let updates = RuleUpdates {
        name: request.name,
        threshold: request.threshold,
        operator: request.operator,
        conclusion_title: request.conclusion_title,
        severity: request.severity,
        description: request.description,
        enabled: request.enabled,
    };
    
    match state.rule_manager.update_rule(&rule_id, updates).await {
        Ok(_) => {
            // 返回更新后的规则
            match state.rule_manager.get_rule(&rule_id).await {
                Some(rule) => Json(ApiResponse::success(rule)),
                None => Json(ApiResponse::error("Rule updated but not found")),
            }
        }
        Err(e) => Json(ApiResponse::error(format!("Failed to update rule: {}", e))),
    }
}

/// 删除规则
async fn delete_rule_handler(
    State(state): State<Arc<RuleApiState>>,
    Path(rule_id): Path<String>,
) -> Json<ApiResponse<()>> {
    match state.rule_manager.remove_rule(&rule_id).await {
        Ok(_) => Json(ApiResponse::success(())),
        Err(e) => Json(ApiResponse::error(format!("Failed to delete rule: {}", e))),
    }
}

/// 重新加载默认规则
async fn reload_defaults_handler(
    State(state): State<Arc<RuleApiState>>,
) -> Json<ApiResponse<()>> {
    match state.rule_manager.reload_defaults().await {
        Ok(_) => Json(ApiResponse::success(())),
        Err(e) => Json(ApiResponse::error(format!("Failed to reload defaults: {}", e))),
    }
}

/// 清空所有规则
async fn clear_rules_handler(
    State(state): State<Arc<RuleApiState>>,
) -> Json<ApiResponse<()>> {
    match state.rule_manager.clear_all().await {
        Ok(_) => Json(ApiResponse::success(())),
        Err(e) => Json(ApiResponse::error(format!("Failed to clear rules: {}", e))),
    }
}

/// 导入规则请求
#[derive(Debug, Deserialize)]
pub struct ImportRulesRequest {
    pub yaml_content: String,
}

/// 导入规则响应
#[derive(Debug, Serialize)]
pub struct ImportRulesResponse {
    pub added: usize,
    pub updated: usize,
    pub errors: Vec<String>,
}

/// 导入规则（YAML）
async fn import_rules_handler(
    State(state): State<Arc<RuleApiState>>,
    Json(request): Json<ImportRulesRequest>,
) -> Json<ApiResponse<ImportRulesResponse>> {
    match state.rule_manager.import_yaml(&request.yaml_content).await {
        Ok(result) => {
            let response = ImportRulesResponse {
                added: result.added,
                updated: result.updated,
                errors: result.errors,
            };
            Json(ApiResponse::success(response))
        }
        Err(e) => Json(ApiResponse::error(format!("Failed to import rules: {}", e))),
    }
}

/// 导出规则响应
#[derive(Debug, Serialize)]
pub struct ExportRulesResponse {
    pub yaml_content: String,
}

/// 导出规则（YAML）
async fn export_rules_handler(
    State(state): State<Arc<RuleApiState>>,
) -> Json<ApiResponse<ExportRulesResponse>> {
    match state.rule_manager.export_yaml().await {
        Ok(yaml) => Json(ApiResponse::success(ExportRulesResponse { yaml_content: yaml })),
        Err(e) => Json(ApiResponse::error(format!("Failed to export rules: {}", e))),
    }
}

/// 获取规则管理器状态
async fn status_handler(
    State(state): State<Arc<RuleApiState>>,
) -> Json<ApiResponse<RuleManagerStatus>> {
    let status = state.rule_manager.status_report().await;
    Json(ApiResponse::success(status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnosis::rule_manager::RuleManager;
    use std::sync::Arc;

    fn create_test_state() -> Arc<RuleApiState> {
        let manager = RuleManager::new_empty();
        Arc::new(RuleApiState::new(manager))
    }

    // 注意：由于 axum 测试需要完整的 HTTP stack，
    // 这里的单元测试主要测试响应结构序列化
    // 集成测试可以通过 main.rs 中的集成测试进行

    #[test]
    fn test_api_response_success() {
        let response: ApiResponse<String> = ApiResponse::success("test".to_string());
        assert!(response.success);
        assert!(response.data.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_api_response_error() {
        let response: ApiResponse<String> = ApiResponse::error("test error");
        assert!(!response.success);
        assert!(response.data.is_none());
        assert!(response.error.is_some());
    }
}
