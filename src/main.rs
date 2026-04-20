//! Nuts Observer - HTTP 服务主程序
//!
//! 纯服务端二进制，通过 HTTP API 提供诊断服务。
//! CLI 客户端请使用独立的 nuts-observer-cli。

use nuts_observer::api::condition::{ConditionTrigger};
use nuts_observer::api::nri::router as nri_router;
use nuts_observer::api::trigger::router as trigger_router;
use nuts_observer::api::health::{router as health_router, AppState};
use axum::Router;
use nuts_observer::collector::nri_mapping::NriMappingTable;
use nuts_observer::collector::oom_events::{OomEventListener, OomListenerConfig};
use nuts_observer::config::{Config, ConfigError};
use nuts_observer::ai::async_bridge::{start_ai_system, AiWorker, AiWorkerConfig};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // 服务器模式运行
    run_server().await;
}

/// 服务器模式运行
async fn run_server() {
    // 加载配置文件
    let config = match load_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}. Using default.", e);
            Config::default()
        }
    };

    // 基础日志初始化
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::new(&config.log_level)
        )
        .init();

    tracing::info!("Configuration loaded: log_level={}, ai_enabled={}, alert_enabled={}",
        config.log_level, config.ai.enabled, config.alert.enabled);

    // 创建共享的 NRI 映射表
    let nri_table = Arc::new(NriMappingTable::new());
    tracing::info!("NRI mapping table initialized");

    // 启动条件触发服务（从配置读取）
    for trigger_config in &config.condition_triggers {
        let nri_table_clone = Arc::clone(&nri_table);
        let config_def = trigger_config.clone();
        let cooldown_ms = config_def.cooldown_ms;
        tokio::spawn(async move {
            let trigger = ConditionTrigger::new(config_def.into(), Some(nri_table_clone))
                .with_cooldown(cooldown_ms);
            trigger.start().await;
        });
    }

    // 启动 OOM 事件监听器（异常联动触发）
    let oom_config = OomListenerConfig {
        enabled: true,
        evidence_types: vec!["block_io".to_string(), "syscall_latency".to_string(), "network".to_string()],
        collection_window_secs: 10,
        cooldown_secs: 60,
        server_url: format!("http://{}:{}", config.server.bind_address, config.server.port),
    };
    let nri_table_for_oom = Arc::clone(&nri_table);
    tokio::spawn(async move {
        let oom_listener = OomEventListener::new(oom_config, nri_table_for_oom);
        oom_listener.start().await;
    });
    tracing::info!("OOM event listener started (oom_kill monitoring)");

    // 初始化异步AI系统（如果启用）
    let (_ai_queue, _ai_store) = if config.ai.enabled {
        let worker_config = AiWorkerConfig {
            adapter_config: config.ai.clone().into(),
            max_concurrent: 3,
            queue_timeout_ms: 300_000,
            retry_limit: 3,
            poll_interval_ms: 100,
            cleanup_interval_secs: 300,
        };
        let (queue, store, rx) = start_ai_system(worker_config.clone());
        
        // 启动AI Worker后台任务
        let worker = AiWorker::new(worker_config, rx, Arc::clone(&store), queue.get_pending_tasks());
        tokio::spawn(async move {
            worker.run().await;
        });
        
        tracing::info!("AI async enhancement system started (enabled=true)");
        (Some(Arc::new(queue)), Some(store))
    } else {
        tracing::info!("AI async enhancement system disabled");
        (None, None)
    };

    // 创建应用状态
    let app_state = Arc::new(AppState::new(Arc::clone(&nri_table)));

    // 构建应用路由：触发器 + NRI Webhook + 健康检查
    let app = Router::new()
        .merge(trigger_router())
        .merge(nri_router(Arc::clone(&nri_table)))
        .merge(health_router(app_state));

    let addr = std::net::SocketAddr::from((
        parse_bind_address(&config.server.bind_address),
        config.server.port,
    ));
    tracing::info!("nuts-observer listening on {addr}");

    let listener = TcpListener::bind(&addr).await.expect("failed to bind");
    axum::serve(listener, app)
        .await
        .expect("server failed");
}

/// 加载配置文件
fn load_config() -> Result<Config, ConfigError> {
    // 尝试从多个路径加载配置文件
    let config_paths = vec![
        "nuts.yaml",
        "/etc/nuts/config.yaml",
        "config/nuts.yaml",
    ];

    for path in &config_paths {
        if std::path::Path::new(path).exists() {
            tracing::info!("Loading config from: {}", path);
            return Config::from_file(path);
        }
    }

    // 如果没有找到配置文件，检查环境变量
    if let Ok(config_path) = std::env::var("NUTS_CONFIG") {
        tracing::info!("Loading config from NUTS_CONFIG: {}", config_path);
        return Config::from_file(config_path);
    }

    // 返回默认配置
    tracing::warn!("No config file found, using default configuration");
    Ok(Config::default())
}

/// 解析绑定地址
fn parse_bind_address(addr: &str) -> std::net::IpAddr {
    addr.parse().unwrap_or_else(|_| std::net::Ipv4Addr::new(0, 0, 0, 0).into())
}


