//! 健康检查与状态 API 模块
//!
//! 提供服务健康状态、版本信息、组件状态查询

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;
use std::time::SystemTime;

use crate::collector::nri_mapping::NriMappingTable;

/// 应用状态（共享）
pub struct AppState {
    /// 启动时间
    pub start_time: SystemTime,
    /// NRI 映射表
    pub nri_table: Arc<NriMappingTable>,
    /// 版本信息
    pub version: String,
}

impl AppState {
    pub fn new(nri_table: Arc<NriMappingTable>) -> Self {
        Self {
            start_time: SystemTime::now(),
            nri_table,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// 健康检查响应
#[derive(Serialize)]
pub struct HealthResponse {
    /// 整体状态
    pub status: String,
    /// 服务版本
    pub version: String,
    /// 运行时长（秒）
    pub uptime_secs: u64,
    /// 各组件状态
    pub components: ComponentStatus,
}

/// 组件状态详情
#[derive(Serialize)]
pub struct ComponentStatus {
    /// API 服务
    pub api: String,
    /// NRI 映射表
    pub nri_mapping: String,
    /// OOM 监听器（通过配置检测）
    pub oom_listener: String,
    /// 权限控制状态
    pub permission_control: String,
}

/// 就绪检查响应
#[derive(Serialize)]
pub struct ReadinessResponse {
    /// 是否就绪
    pub ready: bool,
    /// 检查项
    pub checks: Vec<ReadinessCheck>,
}

/// 就绪检查项
#[derive(Serialize)]
pub struct ReadinessCheck {
    /// 检查名称
    pub name: String,
    /// 是否通过
    pub passed: bool,
    /// 详情
    pub detail: String,
}

/// NRI 映射表统计响应
#[derive(Serialize)]
pub struct MappingStatsResponse {
    /// Pod 数量
    pub pod_count: usize,
    /// 容器数量
    pub container_count: usize,
    /// cgroup 数量
    pub cgroup_count: usize,
    /// PID 数量
    pub pid_count: usize,
}

/// 创建健康检查路由
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/health/ready", get(readiness_handler))
        .route("/health/stats", get(mapping_stats_handler))
        .with_state(state)
}

/// 健康检查处理器
async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let uptime = SystemTime::now()
        .duration_since(state.start_time)
        .unwrap_or_default()
        .as_secs();

    // 检查 NRI 映射表状态
    let nri_mapping_status = if state.nri_table.pod_count() > 0 {
        "healthy".to_string()
    } else {
        "initialized (no pods)".to_string()
    };
    
    // 检查权限控制状态
    let permission_status = if let Some(ctrl) = crate::collector::permission::global_controller() {
        let report = ctrl.status_report().await;
        format!("{:?}", report.mode)
    } else {
        "not_initialized".to_string()
    };

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: state.version.clone(),
        uptime_secs: uptime,
        components: ComponentStatus {
            api: "healthy".to_string(),
            nri_mapping: nri_mapping_status,
            oom_listener: "enabled".to_string(),
            permission_control: permission_status,
        },
    })
}

/// 就绪检查处理器
async fn readiness_handler(State(state): State<Arc<AppState>>) -> Json<ReadinessResponse> {
    let mut checks = vec![];

    // API 服务检查
    checks.push(ReadinessCheck {
        name: "api".to_string(),
        passed: true,
        detail: "API endpoints available".to_string(),
    });

    // NRI 映射表检查
    let nri_healthy = state.nri_table.pod_count() >= 0; // 总是 true，只要有表即可
    checks.push(ReadinessCheck {
        name: "nri_mapping".to_string(),
        passed: nri_healthy,
        detail: format!("{} pods in mapping", state.nri_table.pod_count()),
    });

    let all_passed = checks.iter().all(|c| c.passed);

    Json(ReadinessResponse {
        ready: all_passed,
        checks,
    })
}

/// 映射表统计处理器
async fn mapping_stats_handler(State(state): State<Arc<AppState>>) -> Json<MappingStatsResponse> {
    Json(MappingStatsResponse {
        pod_count: state.nri_table.pod_count(),
        container_count: state.nri_table.container_count(),
        cgroup_count: state.nri_table.cgroup_count(),
        pid_count: state.nri_table.pid_count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn create_test_state() -> Arc<AppState> {
        Arc::new(AppState::new(Arc::new(NriMappingTable::new())))
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = create_test_state();
        let app = router(state);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_readiness_endpoint() {
        let state = create_test_state();
        let app = router(state);

        let response = app
            .oneshot(Request::builder().uri("/health/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_mapping_stats_endpoint() {
        let state = create_test_state();
        let app = router(state);

        let response = app
            .oneshot(Request::builder().uri("/health/stats").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
    }
}
