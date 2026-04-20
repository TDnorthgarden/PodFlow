//! Nuts Observer CLI - 独立的命令行客户端
//!
//! 此二进制文件专门用于 CLI 交互，通过 HTTP API 与 nuts-observer 服务通信。
//! 无需特权运行，可独立部署到用户工作站。

use clap::{Parser, Subcommand};
use serde_json::Value;
use std::time::Duration;

/// Nuts Observer CLI - 容器性能诊断客户端
#[derive(Parser)]
#[command(name = "nuts-observer-cli")]
#[command(about = "Nuts Observer 命令行客户端 - 连接远程诊断服务")]
#[command(version)]
struct Cli {
    /// 服务端地址
    #[arg(short, long, default_value = "http://localhost:3000")]
    server: String,

    /// 请求超时（秒）
    #[arg(short, long, default_value = "30")]
    timeout: u64,

    /// 详细输出
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 触发诊断任务
    Trigger {
        /// Pod 名称（模糊匹配）
        #[arg(short, long)]
        pod_name: Option<String>,
        /// Pod UID
        #[arg(short, long)]
        pod_uid: Option<String>,
        /// 命名空间
        #[arg(short, long)]
        namespace: Option<String>,
        /// cgroup ID
        #[arg(long)]
        cgroup_id: Option<String>,
        /// 证据类型（逗号分隔）
        #[arg(short, long)]
        evidence_types: Option<String>,
        /// 采集时长（秒）
        #[arg(short, long, default_value = "60")]
        window_secs: u64,
        /// 输出格式
        #[arg(short, long, default_value = "table")]
        output: String,
        /// 分层展示
        #[arg(short, long)]
        detail: bool,
    },

    /// 查询诊断结果
    Query {
        /// 任务ID
        #[arg(short, long)]
        task_id: String,
        /// 分层展示
        #[arg(short, long)]
        detail: bool,
    },

    /// 查看任务状态
    Status {
        /// 任务ID（可选）
        #[arg(short, long)]
        task_id: Option<String>,
    },

    /// 监听实时事件
    Watch {
        /// 任务ID
        #[arg(short, long)]
        task_id: String,
    },

    /// 列出匹配的 Pod
    ListPods {
        /// Pod 名称模糊匹配
        #[arg(short, long)]
        pod_name: Option<String>,
        /// 命名空间
        #[arg(short, long)]
        namespace: Option<String>,
    },

    /// 查看系统配置
    Config,

    /// 案例库命令
    #[command(subcommand)]
    Case(CaseCommands),
}

#[derive(Subcommand)]
enum CaseCommands {
    /// 列出所有案例
    List,
    /// 查看案例详情
    Show {
        /// 案例ID
        case_id: String,
    },
    /// 匹配案例
    Match {
        /// 证据类型
        evidence_type: String,
    },
    /// 案例统计
    Stats,
}

/// API 客户端
struct ApiClient {
    client: reqwest::Client,
    base_url: String,
}

impl ApiClient {
    fn new(base_url: &str, timeout_secs: u64) -> Result<Self, Box<dyn std::error::Error>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// 触发诊断
    async fn trigger_diagnosis(
        &self,
        pod_name: Option<&str>,
        pod_uid: Option<&str>,
        namespace: Option<&str>,
        cgroup_id: Option<&str>,
        evidence_types: Option<&str>,
        window_secs: u64,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut request = serde_json::json!({
            "window_secs": window_secs,
        });

        if let Some(name) = pod_name {
            request["pod_name"] = serde_json::json!(name);
        }
        if let Some(uid) = pod_uid {
            request["pod_uid"] = serde_json::json!(uid);
        }
        if let Some(ns) = namespace {
            request["namespace"] = serde_json::json!(ns);
        }
        if let Some(cg) = cgroup_id {
            request["cgroup_id"] = serde_json::json!(cg);
        }
        if let Some(types) = evidence_types {
            let types_vec: Vec<String> = types.split(',').map(|s| s.trim().to_string()).collect();
            request["evidence_types"] = serde_json::json!(types_vec);
        }

        let response = self.client
            .post(format!("{}/v1/diagnosis/trigger", self.base_url))
            .json(&request)
            .send()
            .await?;

        let result: Value = response.json().await?;
        Ok(result)
    }

    /// 查询诊断结果
    async fn query_diagnosis(&self, task_id: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client
            .get(format!("{}/v1/diagnosis/result/{}", self.base_url, task_id))
            .send()
            .await?;

        let result: Value = response.json().await?;
        Ok(result)
    }

    /// 获取服务状态
    async fn get_status(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;

        let result: Value = response.json().await?;
        Ok(result)
    }

    /// 列出 Pod
    async fn list_pods(
        &self,
        pod_name: Option<&str>,
        namespace: Option<&str>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut request = serde_json::json!({});
        
        if let Some(name) = pod_name {
            request["pod_name"] = serde_json::json!(name);
        }
        if let Some(ns) = namespace {
            request["namespace"] = serde_json::json!(ns);
        }

        let response = self.client
            .post(format!("{}/v1/pods/list", self.base_url))
            .json(&request)
            .send()
            .await?;

        let result: Value = response.json().await?;
        Ok(result)
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(if cli.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .init();

    // 创建 API 客户端
    let client = match ApiClient::new(&cli.server, cli.timeout) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ 创建 API 客户端失败: {}", e);
            std::process::exit(1);
        }
    };

    // 执行命令
    if let Err(e) = run_command(&cli.command, &client).await {
        eprintln!("❌ 错误: {}", e);
        std::process::exit(1);
    }
}

async fn run_command(command: &Commands, client: &ApiClient) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Trigger {
            pod_name,
            pod_uid,
            namespace,
            cgroup_id,
            evidence_types,
            window_secs,
            output,
            detail,
        } => {
            println!("🚀 触发诊断任务...");
            
            let result = client.trigger_diagnosis(
                pod_name.as_deref(),
                pod_uid.as_deref(),
                namespace.as_deref(),
                cgroup_id.as_deref(),
                evidence_types.as_deref(),
                *window_secs,
            ).await?;

            if *detail {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_trigger_result(&result, output)?;
            }
        }

        Commands::Query { task_id, detail } => {
            println!("🔍 查询诊断结果: {}", task_id);
            
            let result = client.query_diagnosis(task_id).await?;

            if *detail {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        }

        Commands::Status { task_id } => {
            if let Some(id) = task_id {
                println!("📊 查询任务状态: {}", id);
                let result = client.query_diagnosis(id).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("📊 查询服务状态...");
                let result = client.get_status().await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        }

        Commands::Watch { task_id } => {
            println!("👁️  监听任务: {} (按 Ctrl+C 退出)", task_id);
            println!("⚠️  注意: 实时监听功能需要 WebSocket 支持，当前为轮询模式");
            
            loop {
                let result = client.query_diagnosis(task_id).await?;
                print!("\r[{}] 状态: {}", 
                    chrono::Local::now().format("%H:%M:%S"),
                    result["status"].as_str().unwrap_or("unknown")
                );
                
                if result["status"] == "Done" || result["status"] == "Failed" {
                    println!("\n✅ 任务完成");
                    break;
                }
                
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }

        Commands::ListPods { pod_name, namespace } => {
            println!("📦 列出匹配的 Pod...");
            let result = client.list_pods(pod_name.as_deref(), namespace.as_deref()).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }

        Commands::Config => {
            println!("⚙️  当前配置:");
            println!("  服务端: {}", client.base_url);
            // 这里可以添加更多配置信息
        }

        Commands::Case(case_cmd) => {
            match case_cmd {
                CaseCommands::List => {
                    println!("📚 案例库列表 (需要通过服务端查询)");
                    // 调用 /v1/cases/list 或类似端点
                }
                CaseCommands::Show { case_id } => {
                    println!("📖 案例详情: {}", case_id);
                }
                CaseCommands::Match { evidence_type } => {
                    println!("🔍 匹配案例: {}", evidence_type);
                }
                CaseCommands::Stats => {
                    println!("📊 案例统计");
                }
            }
        }
    }

    Ok(())
}

/// 打印触发诊断结果
fn print_trigger_result(result: &Value, format: &str) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(result)?);
        }
        "table" => {
            println!("┌─────────────────────────────────────┐");
            println!("│ ✅ 诊断任务已触发                    │");
            println!("├─────────────────────────────────────┤");
            
            if let Some(task_id) = result["task_id"].as_str() {
                println!("│ 任务ID: {}", task_id);
            }
            if let Some(status) = result["status"].as_str() {
                println!("│ 状态: {}", status);
            }
            if let Some(msg) = result["message"].as_str() {
                println!("│ 消息: {}", msg);
            }
            
            println!("└─────────────────────────────────────┘");
            println!();
            println!("💡 使用以下命令查看结果:");
            if let Some(task_id) = result["task_id"].as_str() {
                println!("   nuts-observer-cli query -t {}", task_id);
                println!("   nuts-observer-cli status -t {}", task_id);
                println!("   nuts-observer-cli watch -t {}", task_id);
            }
        }
        _ => {
            println!("{}", result);
        }
    }
    
    Ok(())
}
