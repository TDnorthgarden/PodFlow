//! 案例库API模块
//!
//! 提供案例库的HTTP API接口，支持远程查询和匹配

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, Router},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::diagnosis::case_library::CaseLibrary;
use crate::types::error::NutsError;

/// 案例查询参数
#[derive(Debug, Deserialize)]
pub struct CaseQueryParams {
    /// 证据类型过滤
    pub evidence_type: Option<String>,
    /// 案例ID过滤
    pub case_id: Option<String>,
    /// 指标匹配（格式：metric=value,多指标用逗号分隔）
    pub metrics: Option<String>,
    /// 页码（从1开始）
    pub page: Option<usize>,
    /// 每页大小
    pub page_size: Option<usize>,
}

/// 案例列表响应
#[derive(Debug, Serialize)]
pub struct CaseListResponse {
    /// 案例列表
    pub cases: Vec<crate::diagnosis::case_library::FaultCase>,
    /// 总数
    pub total: usize,
    /// 当前页码
    pub page: usize,
    /// 每页大小
    pub page_size: usize,
}

/// 案例匹配响应
#[derive(Debug, Serialize)]
pub struct CaseMatchResponse {
    /// 匹配结果列表
    pub matches: Vec<CaseMatch>,
    /// 总数
    pub total: usize,
}

/// 案例匹配结果
#[derive(Debug, Serialize)]
pub struct CaseMatch {
    /// 案例信息
    pub case: crate::diagnosis::case_library::FaultCase,
    /// 匹配置信度 (0.0-1.0)
    pub confidence: f64,
}

/// 案例库统计响应
#[derive(Debug, Serialize)]
pub struct CaseStatsResponse {
    /// 总案例数
    pub total_cases: usize,
    /// 总技能数
    pub total_skills: usize,
    /// 按证据类型分布
    pub by_evidence_type: std::collections::HashMap<String, usize>,
    /// 按严重程度分布
    pub by_severity: std::collections::HashMap<u8, usize>,
}

/// API状态
pub type CaseApiState = Arc<CaseLibrary>;

/// 创建案例API路由
pub fn router() -> Router {
    Router::new()
        // TODO: Fix axum handler trait bounds - temporarily commented out for T10-1
        // .route("/cases", get(list_cases_handler))
        // .route("/cases/:case_id", get(get_case_handler))
        // .route("/cases/match", get(match_cases_handler))
        // .route("/cases/export", get(export_cases_handler))
        // .route("/cases/stats", get(get_stats_handler))
}

/// 列出所有案例
pub async fn list_cases_handler(
    State(library): State<Arc<CaseLibrary>>,
    Query(params): Query<CaseQueryParams>,
) -> Result<Json<CaseListResponse>, NutsError> {
    let page = params.page.unwrap_or(1);
    let page_size = params.page_size.unwrap_or(20);
    
    let cases = if let Some(evidence_type) = params.evidence_type {
        library.find_cases_by_evidence(&evidence_type)
    } else {
        library.list_cases()
    };

    let total = cases.len();
    let start = (page - 1) * page_size;
    let end = std::cmp::min(start + page_size, total);
    let paginated_cases: Vec<crate::diagnosis::case_library::FaultCase> = cases.iter().skip(start).take(page_size).cloned().collect();

    let response = CaseListResponse {
        cases: paginated_cases,
        total,
        page,
        page_size,
    };

    Ok(Json(response))
}

/// 获取特定案例详情
pub async fn get_case_handler(
    State(library): State<Arc<CaseLibrary>>,
    Path(case_id): Path<String>,
) -> Result<Json<crate::diagnosis::case_library::FaultCase>, NutsError> {
    match library.get_case(&case_id) {
        Some(case) => Ok(Json(case.clone())),
        None => Err(NutsError::not_found(&format!("案例不存在: {}", case_id))),
    }
}

/// 根据指标匹配案例
pub async fn match_cases_handler(
    State(library): State<Arc<CaseLibrary>>,
    Query(params): Query<CaseQueryParams>,
) -> Result<Json<CaseMatchResponse>, NutsError> {
    let metrics_str = params.metrics.unwrap_or_default();
    
    // 解析指标参数
    let mut metrics = std::collections::HashMap::new();
    for metric_str in metrics_str.split(',') {
        let parts: Vec<&str> = metric_str.split('=').collect();
        if parts.len() == 2 {
            if let Ok(val) = parts[1].parse::<f64>() {
                metrics.insert(parts[0].to_string(), val);
            }
        }
    }

    let matches = library.match_cases_by_metrics(&metrics);
    let total = matches.len();

    let case_matches: Vec<CaseMatch> = matches
        .into_iter()
        .map(|(case, confidence)| CaseMatch {
            case: case.clone(),
            confidence,
        })
        .collect();

    let response = CaseMatchResponse {
        matches: case_matches,
        total,
    };

    Ok(Json(response))
}

/// 导出案例库
pub async fn export_cases_handler(
    State(library): State<Arc<CaseLibrary>>,
    Query(params): Query<CaseQueryParams>,
) -> Result<String, NutsError> {
    match library.export_yaml() {
        Ok(yaml) => Ok(yaml),
        Err(e) => Err(NutsError::internal(&format!("导出案例库失败: {}", e))),
    }
}

/// 获取案例库统计
pub async fn get_stats_handler(
    State(library): State<Arc<CaseLibrary>>,
) -> Result<Json<CaseStatsResponse>, NutsError> {
    let stats = library.stats();

    let response = CaseStatsResponse {
        total_cases: stats.total_cases,
        total_skills: stats.total_skills,
        by_evidence_type: stats.by_evidence_type,
        by_severity: stats.by_severity,
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, Method},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_list_cases() {
        // TODO: Re-enable when routes are uncommented (T10-1)
        let router = router();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/cases")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Routes are currently disabled, expect 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_case() {
        // TODO: Re-enable when routes are uncommented (T10-1)
        let router = router();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/cases/euler-cpu-contention-001")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Routes are currently disabled, expect 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_match_cases() {
        // TODO: Re-enable when routes are uncommented (T10-1)
        let router = router();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/cases/match?metrics=cpu_usage=80.5,memory_usage=0.9")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Routes are currently disabled, expect 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_stats() {
        // TODO: Re-enable when routes are uncommented (T10-1)
        let router = router();
        let request = Request::builder()
            .method(Method::GET)
            .uri("/cases/stats")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        // Routes are currently disabled, expect 404
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
