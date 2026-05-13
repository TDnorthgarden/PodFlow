//! Nuts Observer CLI - 命令行客户端入口
//!
//! 此二进制文件是 CLI 交互入口，通过 HTTP API 与 nuts-observer 服务通信。
//! 实际命令逻辑在 crate::cli 模块中统一实现。

use nuts_observer::cli::{Cli, run};
use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        eprintln!("❌ 错误: {}", e);
        std::process::exit(1);
    }
}
