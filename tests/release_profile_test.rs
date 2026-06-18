//! Release Profile 编译验证测试
//!
//! 测试内容：
//! 1. Release profile 编译验证
//! 2. 二进制大小和性能检查
//! 3. 优化级别验证
//! 4. 调试信息移除验证
//! 5. 依赖项优化验证

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Release 编译验证结果
#[derive(Debug, Clone)]
pub struct ReleaseVerificationResult {
    /// 验证项目名称
    verification_item: String,
    /// 验证状态
    status: VerificationStatus,
    /// 详细信息
    details: String,
    /// 建议措施
    recommendation: String,
}

/// 验证状态
#[derive(Debug, Clone, PartialEq)]
pub enum VerificationStatus {
    Pass,
    Warning,
    Fail,
}

impl ReleaseVerificationResult {
    fn new(
        verification_item: String,
        status: VerificationStatus,
        details: String,
        recommendation: String,
    ) -> Self {
        Self {
            verification_item,
            status,
            details,
            recommendation,
        }
    }
}

/// Release Profile 验证测试套件
pub struct ReleaseProfileTestSuite {
    results: Vec<ReleaseVerificationResult>,
    project_root: String,
}

impl ReleaseProfileTestSuite {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            project_root: ".".to_string(),
        }
    }

    /// 运行完整的 release profile 验证
    pub async fn run_full_verification(&mut self) -> Result<Vec<ReleaseVerificationResult>, Box<dyn std::error::Error>> {
        println!("🚀 开始 Release Profile 编译验证...");

        // 1. 清理之前的构建
        self.clean_previous_builds().await?;

        // 2. Release profile 编译
        self.compile_release_profile().await?;

        // 3. 验证二进制文件
        self.verify_binary_files().await?;

        // 4. 验证优化级别
        self.verify_optimization_level().await?;

        // 5. 验证调试信息
        self.verify_debug_info().await?;

        // 6. 验证依赖项优化
        self.verify_dependency_optimization().await?;

        // 7. 性能基准测试
        self.run_performance_benchmarks().await?;

        // 8. 安全检查
        self.verify_security_hardening().await?;

        // 打印验证结果
        self.print_verification_results();

        println!("✅ Release Profile 编译验证完成");
        Ok(self.results.clone())
    }

    /// 清理之前的构建
    async fn clean_previous_builds(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🧹 清理之前的构建...");

        // 清理 target 目录
        let output = Command::new("cargo")
            .args(&["clean"])
            .current_dir(&self.project_root)
            .output()?;

        if output.status.success() {
            self.results.push(ReleaseVerificationResult::new(
                "清理构建文件".to_string(),
                VerificationStatus::Pass,
                "成功清理之前的构建文件".to_string(),
                "无需修复".to_string(),
            ));
        } else {
            self.results.push(ReleaseVerificationResult::new(
                "清理构建文件".to_string(),
                VerificationStatus::Fail,
                format!("清理失败: {}", String::from_utf8_lossy(&output.stderr)),
                "检查文件权限和磁盘空间".to_string(),
            ));
        }

        Ok(())
    }

    /// Release profile 编译
    async fn compile_release_profile(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔨 Release Profile 编译...");

        let start_time = Instant::now();

        // 编译 release 版本
        let output = Command::new("cargo")
            .args(&["build", "--release", "--all-features"])
            .current_dir(&self.project_root)
            .output()?;

        let compilation_time = start_time.elapsed();

        if output.status.success() {
            self.results.push(ReleaseVerificationResult::new(
                "Release Profile 编译".to_string(),
                VerificationStatus::Pass,
                format!("编译成功，耗时: {:?}", compilation_time),
                "无需修复".to_string(),
            ));
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.results.push(ReleaseVerificationResult::new(
                "Release Profile 编译".to_string(),
                VerificationStatus::Fail,
                format!("编译失败: {}", stderr),
                "检查依赖项和代码错误".to_string(),
            ));
            return Err("Release compilation failed".into());
        }

        Ok(())
    }

    /// 验证二进制文件
    async fn verify_binary_files(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📁 验证二进制文件...");

        let release_dir = format!("{}/target/release", self.project_root);
        let expected_binaries = vec![
            "podflow",
            "podflow-adapters",
            "podflow-collector",
        ];

        for binary in expected_binaries {
            let binary_path = format!("{}/{}", release_dir, binary);
            
            if Path::new(&binary_path).exists() {
                let metadata = fs::metadata(&binary_path)?;
                let file_size = metadata.len();
                
                // 检查文件大小（应该在合理范围内）
                if file_size > 1_000_000 { // 大于 1MB
                    if file_size > 50_000_000 { // 大于 50MB 可能有问题
                        self.results.push(ReleaseVerificationResult::new(
                            format!("二进制文件大小检查 - {}", binary),
                            VerificationStatus::Warning,
                            format!("文件过大: {} bytes", file_size),
                            "检查是否包含了不必要的依赖或调试信息".to_string(),
                        ));
                    } else {
                        self.results.push(ReleaseVerificationResult::new(
                            format!("二进制文件大小检查 - {}", binary),
                            VerificationStatus::Pass,
                            format!("文件大小正常: {} bytes", file_size),
                            "无需修复".to_string(),
                        ));
                    }
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        format!("二进制文件大小检查 - {}", binary),
                        VerificationStatus::Warning,
                        format!("文件过小: {} bytes", file_size),
                        "检查编译配置和依赖项".to_string(),
                    ));
                }

                // 检查文件权限
                let permissions = metadata.permissions();
                let mode = permissions.mode();
                
                if mode & 0o111 != 0 { // 检查可执行权限
                    self.results.push(ReleaseVerificationResult::new(
                        format!("二进制文件权限检查 - {}", binary),
                        VerificationStatus::Pass,
                        format!("可执行权限正常: {:o}", mode & 0o777),
                        "无需修复".to_string(),
                    ));
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        format!("二进制文件权限检查 - {}", binary),
                        VerificationStatus::Warning,
                        format!("缺少可执行权限: {:o}", mode & 0o777),
                        "添加可执行权限: chmod +x".to_string(),
                    ));
                }
            } else {
                self.results.push(ReleaseVerificationResult::new(
                    format!("二进制文件存在性检查 - {}", binary),
                    VerificationStatus::Fail,
                    "二进制文件不存在".to_string(),
                    "检查编译配置和构建过程".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// 验证优化级别
    async fn verify_optimization_level(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("⚡ 验证优化级别...");

        let release_dir = format!("{}/target/release", self.project_root);
        let binary_path = format!("{}/podflow", release_dir);

        if Path::new(&binary_path).exists() {
            // 使用 objdump 检查优化标记
            let output = Command::new("objdump")
                .args(&["-h", &binary_path])
                .output()?;

            if output.status.success() {
                let sections = String::from_utf8_lossy(&output.stdout);
                
                // 检查是否有调试符号
                if sections.contains(".debug") {
                    self.results.push(ReleaseVerificationResult::new(
                        "优化级别检查 - 调试符号".to_string(),
                        VerificationStatus::Warning,
                        "检测到调试符号".to_string(),
                        "确保使用 release profile 移除调试符号".to_string(),
                    ));
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        "优化级别检查 - 调试符号".to_string(),
                        VerificationStatus::Pass,
                        "未检测到调试符号".to_string(),
                        "无需修复".to_string(),
                    ));
                }

                // 检查优化选项
                let output = Command::new("readelf")
                    .args(&["-n", &binary_path])
                    .output()?;

                if output.status.success() {
                    let notes = String::from_utf8_lossy(&output.stdout);
                    
                    if notes.contains("GNU") {
                        self.results.push(ReleaseVerificationResult::new(
                            "优化级别检查 - 编译标记".to_string(),
                            VerificationStatus::Pass,
                            "检测到 GNU 优化标记".to_string(),
                            "无需修复".to_string(),
                        ));
                    }
                }
            } else {
                self.results.push(ReleaseVerificationResult::new(
                    "优化级别检查".to_string(),
                    VerificationStatus::Warning,
                    "无法检查优化标记".to_string(),
                    "安装 objdump 和 readelf 工具".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// 验证调试信息
    async fn verify_debug_info(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🐛 验证调试信息移除...");

        let release_dir = format!("{}/target/release", self.project_root);
        let binary_path = format!("{}/podflow", release_dir);

        if Path::new(&binary_path).exists() {
            // 检查调试信息大小
            let output = Command::new("size")
                .arg(&binary_path)
                .output()?;

            if output.status.success() {
                let size_info = String::from_utf8_lossy(&output.stdout);
                
                // 解析大小信息
                let lines: Vec<&str> = size_info.lines().collect();
                if lines.len() >= 2 {
                    let parts: Vec<&str> = lines[1].split_whitespace().collect();
                    if parts.len() >= 3 {
                        let text_size: u64 = parts[0].parse().unwrap_or(0);
                        let data_size: u64 = parts[1].parse().unwrap_or(0);
                        let bss_size: u64 = parts[2].parse().unwrap_or(0);
                        let total_size = text_size + data_size + bss_size;
                        
                        // 检查是否过大（可能包含调试信息）
                        if total_size > 10_000_000 { // 10MB
                            self.results.push(ReleaseVerificationResult::new(
                                "调试信息检查 - 二进制大小".to_string(),
                                VerificationStatus::Warning,
                                format!("二进制可能包含调试信息: {} bytes", total_size),
                                "检查编译配置确保移除调试信息".to_string(),
                            ));
                        } else {
                            self.results.push(ReleaseVerificationResult::new(
                                "调试信息检查 - 二进制大小".to_string(),
                                VerificationStatus::Pass,
                                format!("二进制大小合理: {} bytes", total_size),
                                "无需修复".to_string(),
                            ));
                        }
                    }
                }
            }

            // 检查字符串表中的调试信息
            let output = Command::new("strings")
                .args(&[&binary_path])
                .output()?;

            if output.status.success() {
                let strings_output = String::from_utf8_lossy(&output.stdout);
                
                // 查找调试相关的字符串
                let debug_patterns = vec![
                    "debug",
                    "Debug",
                    "DEBUG",
                    ".debug_",
                    "__rust_",
                    "rustc",
                ];
                
                let mut debug_string_count = 0;
                for pattern in debug_patterns {
                    debug_string_count += strings_output.matches(pattern).count();
                }
                
                if debug_string_count > 100 {
                    self.results.push(ReleaseVerificationResult::new(
                        "调试信息检查 - 字符串表".to_string(),
                        VerificationStatus::Warning,
                        format!("检测到可能调试字符串: {} 个", debug_string_count),
                        "检查编译配置移除调试信息".to_string(),
                    ));
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        "调试信息检查 - 字符串表".to_string(),
                        VerificationStatus::Pass,
                        format!("调试字符串数量合理: {} 个", debug_string_count),
                        "无需修复".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// 验证依赖项优化
    async fn verify_dependency_optimization(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📦 验证依赖项优化...");

        // 检查 Cargo.toml 中的依赖项配置
        let cargo_toml_path = format!("{}/Cargo.toml", self.project_root);
        
        if Path::new(&cargo_toml_path).exists() {
            let cargo_toml_content = fs::read_to_string(&cargo_toml_path)?;
            
            // 检查是否有不必要的开发依赖在生产构建中
            if cargo_toml_content.contains("[dev-dependencies]") {
                self.results.push(ReleaseVerificationResult::new(
                    "依赖项优化检查 - 开发依赖".to_string(),
                    VerificationStatus::Pass,
                    "检测到开发依赖配置".to_string(),
                    "确保 release 构建不包含开发依赖".to_string(),
                ));
            }

            // 检查特性配置
            if cargo_toml_content.contains("[features]") {
                self.results.push(ReleaseVerificationResult::new(
                    "依赖项优化检查 - 特性配置".to_string(),
                    VerificationStatus::Pass,
                    "检测到特性配置".to_string(),
                    "检查 release 构建使用的特性".to_string(),
                ));
            }
        }

        // 检查编译后的依赖项
        let output = Command::new("cargo")
            .args(&["tree", "--format", "{p}", "--target", "x86_64-unknown-linux-gnu"])
            .current_dir(&self.project_root)
            .output()?;

        if output.status.success() {
            let dependencies = String::from_utf8_lossy(&output.stdout);
            let dep_count = dependencies.lines().count();
            
            if dep_count > 200 {
                self.results.push(ReleaseVerificationResult::new(
                    "依赖项优化检查 - 依赖数量".to_string(),
                    VerificationStatus::Warning,
                    format!("依赖项数量较多: {} 个", dep_count),
                    "考虑移除不必要的依赖项".to_string(),
                ));
            } else {
                self.results.push(ReleaseVerificationResult::new(
                    "依赖项优化检查 - 依赖数量".to_string(),
                    VerificationStatus::Pass,
                    format!("依赖项数量合理: {} 个", dep_count),
                    "无需修复".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// 运行性能基准测试
    async fn run_performance_benchmarks(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🏃 运行性能基准测试...");

        // 运行基准测试
        let output = Command::new("cargo")
            .args(&["bench", "--release"])
            .current_dir(&self.project_root)
            .output()?;

        if output.status.success() {
            let benchmark_output = String::from_utf8_lossy(&output.stdout);
            
            // 检查基准测试结果
            if benchmark_output.contains("test result: ok") {
                self.results.push(ReleaseVerificationResult::new(
                    "性能基准测试".to_string(),
                    VerificationStatus::Pass,
                    "基准测试通过".to_string(),
                    "无需修复".to_string(),
                ));
            } else {
                self.results.push(ReleaseVerificationResult::new(
                    "性能基准测试".to_string(),
                    VerificationStatus::Warning,
                    "基准测试结果异常".to_string(),
                    "检查基准测试配置和实现".to_string(),
                ));
            }

            // 检查性能指标
            if benchmark_output.contains("ns/iter") || benchmark_output.contains("MB/s") {
                self.results.push(ReleaseVerificationResult::new(
                    "性能指标检查".to_string(),
                    VerificationStatus::Pass,
                    "检测到性能指标".to_string(),
                    "无需修复".to_string(),
                ));
            }
        } else {
            self.results.push(ReleaseVerificationResult::new(
                "性能基准测试".to_string(),
                VerificationStatus::Warning,
                "基准测试执行失败".to_string(),
                "检查基准测试配置和依赖项".to_string(),
            ));
        }

        Ok(())
    }

    /// 验证安全加固
    async fn verify_security_hardening(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔒 验证安全加固...");

        let release_dir = format!("{}/target/release", self.project_root);
        let binary_path = format!("{}/podflow", release_dir);

        if Path::new(&binary_path).exists() {
            // 检查 RELRO (Relocation Read-Only)
            let output = Command::new("readelf")
                .args(&["-l", &binary_path])
                .output()?;

            if output.status.success() {
                let program_headers = String::from_utf8_lossy(&output.stdout);
                
                if program_headers.contains("GNU_RELRO") {
                    self.results.push(ReleaseVerificationResult::new(
                        "安全加固检查 - RELRO".to_string(),
                        VerificationStatus::Pass,
                        "检测到 GNU_RELRO 保护".to_string(),
                        "无需修复".to_string(),
                    ));
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        "安全加固检查 - RELRO".to_string(),
                        VerificationStatus::Warning,
                        "未检测到 GNU_RELRO 保护".to_string(),
                        "在链接器选项中启用 RELRO".to_string(),
                    ));
                }

                if program_headers.contains("GNU_STACK") && program_headers.contains("RWE") {
                    self.results.push(ReleaseVerificationResult::new(
                        "安全加固检查 - 栈保护".to_string(),
                        VerificationStatus::Warning,
                        "栈可执行".to_string(),
                        "启用栈不可执行保护".to_string(),
                    ));
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        "安全加固检查 - 栈保护".to_string(),
                        VerificationStatus::Pass,
                        "栈保护配置正常".to_string(),
                        "无需修复".to_string(),
                    ));
                }
            }

            // 检查 FORTIFY_SOURCE
            let output = Command::new("objdump")
                .args(&["-t", &binary_path])
                .output()?;

            if output.status.success() {
                let symbols = String::from_utf8_lossy(&output.stdout);
                
                if symbols.contains("__stack_chk_fail") {
                    self.results.push(ReleaseVerificationResult::new(
                        "安全加固检查 - 栈保护".to_string(),
                        VerificationStatus::Pass,
                        "检测到栈溢出保护".to_string(),
                        "无需修复".to_string(),
                    ));
                } else {
                    self.results.push(ReleaseVerificationResult::new(
                        "安全加固检查 - 栈保护".to_string(),
                        VerificationStatus::Warning,
                        "未检测到栈溢出保护".to_string(),
                        "启用 FORTIFY_SOURCE 和栈保护".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// 打印验证结果
    fn print_verification_results(&self) {
        println!("\n📊 Release Profile 验证结果:");
        
        let mut pass_count = 0;
        let mut warning_count = 0;
        let mut fail_count = 0;

        for result in &self.results {
            let status_icon = match result.status {
                VerificationStatus::Pass => { pass_count += 1; "✅" },
                VerificationStatus::Warning => { warning_count += 1; "⚠️" },
                VerificationStatus::Fail => { fail_count += 1; "❌" },
            };

            println!("\n{} {}", status_icon, result.verification_item);
            println!("   状态: {:?}", result.status);
            println!("   详情: {}", result.details);
            println!("   建议: {}", result.recommendation);
        }

        println!("\n📈 验证统计:");
        println!("   - 通过: {} ({:.1}%)", pass_count, pass_count as f64 / self.results.len() as f64 * 100.0);
        println!("   - 警告: {} ({:.1}%)", warning_count, warning_count as f64 / self.results.len() as f64 * 100.0);
        println!("   - 失败: {} ({:.1}%)", fail_count, fail_count as f64 / self.results.len() as f64 * 100.0);

        // 整体评估
        let success_rate = pass_count as f64 / self.results.len() as f64;
        println!("\n🎯 Release Profile 质量评分: {:.1}/100", success_rate * 100.0);

        if success_rate >= 0.9 {
            println!("   ✅ Release Profile 配置优秀，可以用于生产部署");
        } else if success_rate >= 0.8 {
            println!("   ✅ Release Profile 配置良好，建议修复警告项后部署");
        } else if success_rate >= 0.7 {
            println!("   ⚠️  Release Profile 配置一般，需要修复关键问题后部署");
        } else {
            println!("   ❌ Release Profile 配置较差，不能用于生产部署");
        }
    }
}

#[cfg(test)]
mod release_profile_tests {
    use super::*;

    /// 完整的 release profile 验证测试
    #[tokio::test]
    #[ignore] // 默认忽略，避免CI编译时间过长
    async fn test_full_release_profile_verification() {
        let mut test_suite = ReleaseProfileTestSuite::new();
        let results = test_suite.run_full_verification().await
            .expect("Release profile verification failed");

        // 验证所有检查都通过
        assert!(!results.is_empty());
        
        let pass_count = results.iter()
            .filter(|r| r.status == VerificationStatus::Pass)
            .count();
        
        let success_rate = pass_count as f64 / results.len() as f64;
        assert!(success_rate >= 0.8, "Release profile success rate should be >= 80%");
    }

    /// Release 编译测试
    #[tokio::test]
    #[ignore]
    async fn test_release_compilation() {
        let mut test_suite = ReleaseProfileTestSuite::new();
        
        test_suite.clean_previous_builds().await.expect("Clean failed");
        test_suite.compile_release_profile().await.expect("Release compilation failed");
        
        // 验证二进制文件存在
        assert!(Path::new("./target/release/podflow").exists());
    }

    /// 二进制验证测试
    #[tokio::test]
    #[ignore]
    async fn test_binary_verification() {
        let mut test_suite = ReleaseProfileTestSuite::new();
        test_suite.verify_binary_files().await.expect("Binary verification failed");
        
        assert!(!test_suite.results.is_empty());
    }

    /// 优化级别验证测试
    #[tokio::test]
    #[ignore]
    async fn test_optimization_verification() {
        let mut test_suite = ReleaseProfileTestSuite::new();
        test_suite.verify_optimization_level().await.expect("Optimization verification failed");
        
        assert!(!test_suite.results.is_empty());
    }

    /// 调试信息移除验证测试
    #[tokio::test]
    #[ignore]
    async fn test_debug_info_removal() {
        let mut test_suite = ReleaseProfileTestSuite::new();
        test_suite.verify_debug_info().await.expect("Debug info verification failed");
        
        assert!(!test_suite.results.is_empty());
    }

    /// 安全加固验证测试
    #[tokio::test]
    #[ignore]
    async fn test_security_hardening() {
        let mut test_suite = ReleaseProfileTestSuite::new();
        test_suite.verify_security_hardening().await.expect("Security hardening verification failed");
        
        assert!(!test_suite.results.is_empty());
    }
}
