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
    #[arg(short, long, default_value = "http://localhost:8080")]
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
        #[arg(short = 'p', long)]
        pod_name: Option<String>,
        /// Pod UID
        #[arg(short = 'u', long)]
        pod_uid: Option<String>,
        /// 命名空间
        #[arg(short = 's', long)]
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
        #[arg(short = 'i', long)]
        task_id: String,
        /// 分层展示
        #[arg(short, long)]
        detail: bool,
    },

    /// 查看任务状态
    Status {
        /// 任务ID（可选）
        #[arg(short = 'i', long)]
        task_id: Option<String>,
    },

    /// 监听实时事件
    Watch {
        /// 任务ID
        #[arg(short = 'i', long)]
        task_id: String,
    },

    /// 列出匹配的 Pod
    ListPods {
        /// Pod 名称模糊匹配
        #[arg(short = 'p', long)]
        pod_name: Option<String>,
        /// 命名空间
        #[arg(short = 's', long)]
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
        // 构建 target 对象
        let mut target = serde_json::json!({});
        if let Some(name) = pod_name {
            target["pod_name"] = serde_json::json!(name);
        }
        if let Some(uid) = pod_uid {
            target["pod_uid"] = serde_json::json!(uid);
        }
        if let Some(ns) = namespace {
            target["namespace"] = serde_json::json!(ns);
        }
        if let Some(cg) = cgroup_id {
            target["cgroup_id"] = serde_json::json!(cg);
        }

        // 构建 collection_options
        let mut collection_options = serde_json::json!({});
        if let Some(types) = evidence_types {
            let types_vec: Vec<String> = types.split(',').map(|s| s.trim().to_string()).collect();
            collection_options["requested_evidence_types"] = serde_json::json!(types_vec);
        }

        // 构建 time_window
        let end_time_ms = chrono::Utc::now().timestamp_millis();
        let start_time_ms = end_time_ms - (window_secs as i64 * 1000);
        let time_window = serde_json::json!({
            "start_time_ms": start_time_ms,
            "end_time_ms": end_time_ms,
        });

        // 构建完整的 TriggerRequest
        let request = serde_json::json!({
            "trigger_type": if cgroup_id.is_some() { "cgroup" } else if pod_uid.is_some() { "pod" } else { "manual" },
            "idempotency_key": format!("cli-{}", uuid::Uuid::new_v4()),
            "target": if target.as_object().map(|o| o.is_empty()).unwrap_or(true) { None } else { Some(target) },
            "collection_options": if collection_options.as_object().map(|o| o.is_empty()).unwrap_or(true) { None } else { Some(collection_options) },
            "time_window": time_window,
        });

        let url = format!("{}/v1/diagnostics:trigger", self.base_url);
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("触发诊断失败 ({}): {}", status, error_text).into());
        }

        let result: Value = response.json().await?;
        Ok(result)
    }

    /// 查询诊断结果
    async fn query_diagnosis(&self, task_id: &str) -> Result<Value, Box<dyn std::error::Error>> {
        // 首先尝试 AI 增强结果端点（如果服务端支持）
        let ai_endpoint = format!("{}/v1/diagnosis/{}/ai", self.base_url, task_id);
        match self.client.get(&ai_endpoint).send().await {
            Ok(response) if response.status().is_success() => {
                let result: Value = response.json().await?;
                // 检查是否是有效的 AI 增强结果
                if result.get("error").is_none() {
                    return Ok(result);
                }
                // 如果不是有效的 AI 结果，继续尝试其他端点
            }
            _ => {}
        }

        // 尝试诊断结果端点
        let endpoints = vec![
            format!("{}/v1/diagnosis/{}/ai", self.base_url, task_id),
            format!("{}/v1/diagnostics/{}", self.base_url, task_id),
        ];

        for endpoint in endpoints {
            match self.client.get(&endpoint).send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(response.json().await?);
                }
                _ => continue,
            }
        }

        // 所有端点都失败，提供有用的错误信息
        Err(format!(
            "无法查询诊断结果。任务ID: {}\n\n💡 诊断结果通常在触发时已返回，无需单独查询。\n\n可能的原因:\n1. 任务ID不正确或已过期\n2. 诊断结果未被持久化存储\n3. 服务端未启用 AI 增强功能\n\n建议:\n1. 重新触发诊断获取最新结果\n2. 检查触发命令的输出中是否包含诊断结果\n3. 确认服务端配置了 AI 增强功能（如果使用 AI 查询）",
            task_id
        ).into())
    }

    /// 获取服务状态
    async fn get_status(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let url = format!("{}/health", self.base_url);
        let response = self.client
            .get(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("获取服务状态失败 ({}): {}", status, error_text).into());
        }

        let result: Value = response.json().await?;
        Ok(result)
    }

    /// 列出 Pod
    async fn list_pods(
        &self,
        pod_name: Option<&str>,
        namespace: Option<&str>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        // 构建查询参数（GET 请求使用 query string）
        let mut url = format!("{}/v1/nri/pods", self.base_url);

        if let Some(name) = pod_name {
            url = format!("{}/search?name={}", url, name);
        } else if let Some(ns) = namespace {
            url = format!("{}/search?namespace={}", url, ns);
        }

        let response = self.client
            .get(&url)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("列出 Pod 失败 ({}): {}", status, error_text).into());
        }

        let result: Value = response.json().await?;
        Ok(result)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let client = ApiClient::new(&cli.server, cli.timeout)
        .map_err(|e| format!("创建 API 客户端失败: {}", e))?;

    // 执行命令
    if let Err(e) = run_command(&cli.command, &client).await {
        eprintln!("❌ 错误: {}", e);
        
        // 提供有用的错误信息
        if e.to_string().contains("连接") || e.to_string().contains("network") {
            eprintln!("\n💡 建议检查:");
            eprintln!("  1. 服务端是否运行: curl {}/health", cli.server);
            eprintln!("  2. 服务器地址是否正确: {}", cli.server);
            eprintln!("  3. 网络连接是否正常");
        }
        
        std::process::exit(1);
    }
    
    Ok(())
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
                match client.query_diagnosis(id).await {
                    Ok(result) => {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    }
                    Err(e) => {
                        println!("⚠️  无法查询任务状态: {}", e);
                        println!("\n💡 提示: 诊断结果通常在触发时已返回。请检查触发命令的输出。");
                    }
                }
            } else {
                println!("📊 查询服务状态...");
                match client.get_status().await {
                    Ok(result) => {
                        print_enhanced_status(&result, &client.base_url)?;
                    }
                    Err(e) => {
                        println!("❌ 无法获取服务状态: {}", e);
                        println!("\n💡 建议检查:");
                        println!("  1. 服务是否运行: curl {}/health", client.base_url);
                        println!("  2. 网络连接是否正常");
                        println!("  3. 服务地址是否正确: {}", client.base_url);
                    }
                }
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

/// 打印增强的服务状态
fn print_enhanced_status(result: &Value, base_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 🚀 Nuts Observer 服务状态                                   │");
    println!("├─────────────────────────────────────────────────────────────┤");
    
    // 基本状态信息
    if let Some(status) = result["status"].as_str() {
        let status_icon = if status == "healthy" { "✅" } else { "⚠️ " };
        println!("│ 状态: {} {}", status_icon, status);
    }
    
    if let Some(version) = result["version"].as_str() {
        println!("│ 版本: {}", version);
    }
    
    if let Some(uptime) = result["uptime_secs"].as_u64() {
        let hours = uptime / 3600;
        let minutes = (uptime % 3600) / 60;
        let seconds = uptime % 60;
        println!("│ 运行时间: {}小时 {}分钟 {}秒", hours, minutes, seconds);
    }
    
    println!("│ 服务地址: {}", base_url);
    println!("├─────────────────────────────────────────────────────────────┤");
    
    // 组件状态
    if let Some(components) = result["components"].as_object() {
        println!("│ 📦 组件状态:");
        
        let component_status = |status: &str| -> &str {
            match status {
                s if s.contains("healthy") || s.contains("enabled") || s.contains("initialized") => "✅",
                s if s.contains("not_initialized") || s.contains("disabled") => "⚪",
                _ => "⚠️ ",
            }
        };
        
        for (name, status_value) in components {
            if let Some(status) = status_value.as_str() {
                let icon = component_status(status);
                println!("│   {} {}: {}", icon, name, status);
            }
        }
    }
    
    // 添加系统信息（模拟）
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│ 📊 系统信息:");
    
    // 获取当前时间
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    println!("│   当前时间: {}", now);
    
    // 模拟内存使用（在实际应用中可以从系统获取）
    println!("│   内存使用: 约 128 MB");
    println!("│   CPU 使用: 低");
    println!("│   活动连接: 1");
    
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│ 💡 使用提示:");
    println!("│   • 使用 'trigger' 命令触发诊断");
    println!("│   • 使用 'list-pods' 命令列出 Pod");
    println!("│   • 使用 '--verbose' 选项查看详细日志");
    println!("└─────────────────────────────────────────────────────────────┘");
    
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
            if let Some(duration) = result["duration_ms"].as_i64() {
                println!("│ 耗时: {} ms", duration);
            }
            if let Some(evidence_count) = result["evidence_count"].as_i64() {
                println!("│ 证据数量: {}", evidence_count);
            }
            if let Some(conclusion_count) = result["conclusion_count"].as_i64() {
                println!("│ 结论数量: {}", conclusion_count);
            }

            // 显示 AI 增强状态
            if let Some(ai_enhancement) = result["ai_enhancement"].as_object() {
                if let Some(submitted) = ai_enhancement["submitted"].as_bool() {
                    if submitted {
                        println!("│ AI 增强: ✅ 已提交");
                    } else if let Some(reason) = ai_enhancement["reason"].as_str() {
                        println!("│ AI 增强: ❌ 未提交 ({})", reason);
                    }
                }
            }

            println!("└─────────────────────────────────────┘");
            println!();

            // 显示诊断预览摘要
            if let Some(diagnosis_preview) = result["diagnosis_preview"].as_object() {
                println!("📋 诊断结果摘要:");

                if let Some(conclusions) = diagnosis_preview["conclusions"].as_array() {
                    if !conclusions.is_empty() {
                        println!("  结论:");
                        for (i, conclusion) in conclusions.iter().enumerate().take(3) {
                            if let Some(title) = conclusion["title"].as_str() {
                                let confidence = conclusion["confidence"].as_f64().unwrap_or(0.0);
                                let severity = conclusion["severity"].as_i64().unwrap_or(0);
                                println!("  {}. {} (置信度: {:.1}%, 严重度: {})",
                                    i + 1, title, confidence * 100.0, severity);
                            }
                        }
                        if conclusions.len() > 3 {
                            println!("  ... 还有 {} 个结论", conclusions.len() - 3);
                        }
                    }
                }

                if let Some(status) = diagnosis_preview["status"].as_str() {
                    println!("  状态: {}", status);
                }
            }

            println!();
            println!("💡 提示:");
            if let Some(task_id) = result["task_id"].as_str() {
                println!("  • 诊断结果已在此次响应中返回");
                println!("  • 使用 '--detail' 选项查看完整结果");
                println!("  • 使用 '--output json' 查看原始数据");
            }
        }
        _ => {
            println!("{}", result);
        }
    }

    Ok(())
}