//! 诊断案例库模块 - 基于欧拉witty-ops-cases建立case-to-skill映射
//!
//! 目标：沉淀openEuler社区故障案例的诊断知识，建立标准化的
//! 案例-技能映射，支持诊断规则自动生成和AI知识增强。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 故障案例定义
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FaultCase {
    /// 案例唯一ID
    pub case_id: String,
    /// 案例标题
    pub title: String,
    /// 故障描述
    pub description: String,
    /// 相关证据类型
    pub evidence_types: Vec<String>,
    /// 关键指标模式
    pub metric_patterns: Vec<MetricPattern>,
    /// 诊断技能映射
    pub skills: Vec<DiagnosisSkill>,
    /// 根因分析
    pub root_causes: Vec<RootCause>,
    /// 修复建议
    pub remediation: Vec<RemediationStep>,
    /// 参考链接
    pub references: Vec<String>,
    /// 严重程度 (1-10)
    pub severity: u8,
    /// 置信度 (0.0-1.0)
    pub confidence: f64,
}

/// 指标模式 - 用于匹配故障特征
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetricPattern {
    /// 指标名称
    pub metric_name: String,
    /// 操作符
    pub operator: String,
    /// 阈值
    pub threshold: f64,
    /// 描述
    pub description: String,
}

impl MetricPattern {
    /// 转换为规则定义的阈值格式
    pub fn to_threshold_rule(&self, evidence_type: &str) -> crate::config::ThresholdRuleDef {
        crate::config::ThresholdRuleDef {
            metric_name: self.metric_name.clone(),
            evidence_type: evidence_type.to_string(),
            operator: self.operator.clone(),
            threshold: self.threshold,
            description: self.description.clone(),
        }
    }
}

/// 诊断技能 - 可复用的诊断能力
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiagnosisSkill {
    /// 技能ID
    pub skill_id: String,
    /// 技能名称
    pub name: String,
    /// 技能描述
    pub description: String,
    /// 需要采集的证据类型
    pub required_evidence: Vec<String>,
    /// 检查方法
    pub check_method: CheckMethod,
    /// 相关指标
    pub metrics: Vec<String>,
}

/// 检查方法
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum CheckMethod {
    /// 阈值检查
    ThresholdCheck {
        metric: String,
        operator: String,
        threshold: f64,
    },
    /// 模式匹配
    PatternMatch {
        pattern: String,
    },
    /// 相关性分析
    CorrelationCheck {
        metrics: Vec<String>,
        correlation_type: String,
    },
    /// 趋势分析
    TrendAnalysis {
        metric: String,
        window_secs: u64,
        trend_type: String,
    },
}

/// 根因分析
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RootCause {
    /// 根因描述
    pub description: String,
    /// 置信度
    pub confidence: f64,
    /// 验证方法
    pub verification: String,
}

/// 修复步骤
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemediationStep {
    /// 步骤序号
    pub step: u32,
    /// 操作描述
    pub action: String,
    /// 预期效果
    pub expected_outcome: String,
    /// 风险提示
    pub risk: Option<String>,
}

/// 案例库配置（用于YAML反序列化）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CasesConfig {
    pub cases: Vec<FaultCase>,
    #[serde(default)]
    pub evidence_to_skills: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub metric_thresholds: HashMap<String, MetricThreshold>,
}

/// 指标阈值定义
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetricThreshold {
    pub warning: f64,
    pub critical: f64,
}

/// 案例库统计信息
#[derive(Debug, Clone, Serialize)]
pub struct CaseLibraryStats {
    pub total_cases: usize,
    pub by_evidence_type: HashMap<String, usize>,
    pub by_severity: HashMap<u8, usize>,
    pub total_skills: usize,
}

/// 案例库错误类型
#[derive(Debug)]
pub enum CaseLibraryError {
    IoError(std::io::Error),
    ParseError(String),
    NotFound(String),
}

impl std::fmt::Display for CaseLibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaseLibraryError::IoError(e) => write!(f, "IO error: {}", e),
            CaseLibraryError::ParseError(e) => write!(f, "Parse error: {}", e),
            CaseLibraryError::NotFound(id) => write!(f, "Case not found: {}", id),
        }
    }
}

impl std::error::Error for CaseLibraryError {}

/// 案例库
pub struct CaseLibrary {
    /// 案例存储
    cases: HashMap<String, FaultCase>,
    /// 技能索引
    skill_index: HashMap<String, Vec<String>>, // skill_id -> case_ids
    /// 证据类型索引
    evidence_index: HashMap<String, Vec<String>>, // evidence_type -> case_ids
}

impl CaseLibrary {
    /// 创建新的案例库并加载默认案例和文件案例
    pub fn new() -> Self {
        let mut library = Self {
            cases: HashMap::new(),
            skill_index: HashMap::new(),
            evidence_index: HashMap::new(),
        };
        // 先加载硬编码的默认案例
        library.load_default_cases();
        // 再尝试从配置文件加载扩展案例（尝试多个可能的路径）
        let possible_paths = [
            "cases/cases.yaml",
            "/root/nuts/cases/cases.yaml",
            "./cases/cases.yaml",
            "../cases/cases.yaml",
        ];
        
        let mut loaded = false;
        for path in &possible_paths {
            match library.load_cases_from_file(path) {
                Ok(_) => {
                    loaded = true;
                    break;
                }
                Err(e) => {
                    eprintln!("[DEBUG] Failed to load from {}: {}", path, e);
                }
            }
        }
        
        if !loaded {
            eprintln!("[INFO] No external cases loaded, using {} default cases", library.cases.len());
        }
        
        library
    }

    /// 从YAML文件加载案例
    pub fn load_cases_from_file(&mut self, path: &str) -> Result<(), CaseLibraryError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| CaseLibraryError::IoError(e))?;
        
        let config: CasesConfig = serde_yaml::from_str(&content)
            .map_err(|e| CaseLibraryError::ParseError(e.to_string()))?;
        
        let loaded_count = config.cases.len();
        for case in config.cases {
            self.register_case(case);
        }
        
        tracing::info!("Loaded {} cases from {}", loaded_count, path);
        Ok(())
    }

    /// 重新加载案例库（支持热更新）
    pub fn reload(&mut self) -> Result<(), CaseLibraryError> {
        // 清空现有索引
        self.cases.clear();
        self.skill_index.clear();
        self.evidence_index.clear();
        
        // 重新加载
        self.load_default_cases();
        if let Err(e) = self.load_cases_from_file("cases/cases.yaml") {
            tracing::warn!("Failed to reload cases from file: {}", e);
        }
        
        tracing::info!("Case library reloaded, total cases: {}", self.cases.len());
        Ok(())
    }

    /// 获取案例统计信息（供外部调用）
    pub fn stats(&self) -> CaseLibraryStats {
        self.get_stats()
    }

    /// 获取案例统计信息
    pub fn get_stats(&self) -> CaseLibraryStats {
        let total_cases = self.cases.len();
        let mut by_evidence_type: HashMap<String, usize> = HashMap::new();
        let mut by_severity: HashMap<u8, usize> = HashMap::new();
        
        for case in self.cases.values() {
            // 按证据类型统计
            for etype in &case.evidence_types {
                *by_evidence_type.entry(etype.clone()).or_insert(0) += 1;
            }
            // 按严重程度统计
            *by_severity.entry(case.severity).or_insert(0) += 1;
        }
        
        CaseLibraryStats {
            total_cases,
            by_evidence_type,
            by_severity,
            total_skills: self.skill_index.len(),
        }
    }

    /// 加载默认的欧拉社区案例
    fn load_default_cases(&mut self) {
        // 案例1: 容器CPU资源争抢
        let case1 = FaultCase {
            case_id: "euler-cpu-contention-001".to_string(),
            title: "容器CPU throttle导致服务延迟升高".to_string(),
            description: "多个容器共享CPU核心时，由于cgroup的cpu.cfs_quota限制，
                         触发CPU throttle，导致服务响应延迟P99异常升高".to_string(),
            evidence_types: vec!["cgroup_contention".to_string()],
            metric_patterns: vec![
                MetricPattern {
                    metric_name: "cpu_throttle_rate".to_string(),
                    operator: ">".to_string(),
                    threshold: 5.0,
                    description: "CPU throttle率超过5%".to_string(),
                },
                MetricPattern {
                    metric_name: "cpu_usage_percent".to_string(),
                    operator: ">".to_string(),
                    threshold: 80.0,
                    description: "CPU使用率超过80%".to_string(),
                },
            ],
            skills: vec![
                DiagnosisSkill {
                    skill_id: "check_cpu_throttle".to_string(),
                    name: "检查CPU throttle".to_string(),
                    description: "检查cgroup的cpu.stat中的nr_throttled和throttled_usec".to_string(),
                    required_evidence: vec!["cgroup_contention".to_string()],
                    check_method: CheckMethod::ThresholdCheck {
                        metric: "cpu_throttle_rate".to_string(),
                        operator: ">".to_string(),
                        threshold: 5.0,
                    },
                    metrics: vec!["cpu_throttle_rate".to_string(), "cpu_throttle_count".to_string()],
                },
            ],
            root_causes: vec![
                RootCause {
                    description: "cgroup cpu.cfs_quota_us限制过于严格".to_string(),
                    confidence: 0.8,
                    verification: "检查/sys/fs/cgroup/cpu/cpu.cfs_quota_us".to_string(),
                },
                RootCause {
                    description: "容器CPU limit设置过低".to_string(),
                    confidence: 0.7,
                    verification: "检查容器spec.resources.limits.cpu".to_string(),
                },
            ],
            remediation: vec![
                RemediationStep {
                    step: 1,
                    action: "临时提升容器CPU limit".to_string(),
                    expected_outcome: "throttle率下降，延迟恢复".to_string(),
                    risk: Some("可能影响同节点其他容器".to_string()),
                },
                RemediationStep {
                    step: 2,
                    action: "调整cfs_quota或切换为cpu-shares".to_string(),
                    expected_outcome: "更公平的CPU分配".to_string(),
                    risk: None,
                },
            ],
            references: vec![
                "https://gitee.com/openeuler/witty-ops-cases/cpu-contention".to_string(),
            ],
            severity: 7,
            confidence: 0.85,
        };

        // 案例2: 内存压力导致OOM
        let case2 = FaultCase {
            case_id: "euler-oom-pressure-001".to_string(),
            title: "容器内存压力触发OOM Kill".to_string(),
            description: "容器内存使用量接近limit，触发内核OOM killer，
                         导致业务进程被强制终止".to_string(),
            evidence_types: vec!["cgroup_contention".to_string(), "oom_events".to_string()],
            metric_patterns: vec![
                MetricPattern {
                    metric_name: "memory_usage_percent".to_string(),
                    operator: ">=".to_string(),
                    threshold: 90.0,
                    description: "内存使用率超过90%".to_string(),
                },
                MetricPattern {
                    metric_name: "memory_pressure_score".to_string(),
                    operator: ">".to_string(),
                    threshold: 50.0,
                    description: "内存压力分数超过50".to_string(),
                },
            ],
            skills: vec![
                DiagnosisSkill {
                    skill_id: "check_memory_pressure".to_string(),
                    name: "检查内存压力".to_string(),
                    description: "检查memory.pressure文件中的avg10指标".to_string(),
                    required_evidence: vec!["cgroup_contention".to_string()],
                    check_method: CheckMethod::ThresholdCheck {
                        metric: "memory_pressure_avg10".to_string(),
                        operator: ">".to_string(),
                        threshold: 80.0,
                    },
                    metrics: vec!["memory_usage_percent".to_string(), "memory_pressure_score".to_string()],
                },
                DiagnosisSkill {
                    skill_id: "detect_oom_kill".to_string(),
                    name: "检测OOM Kill事件".to_string(),
                    description: "监听dmesg中的oom_kill事件".to_string(),
                    required_evidence: vec!["oom_events".to_string()],
                    check_method: CheckMethod::PatternMatch {
                        pattern: "oom_kill".to_string(),
                    },
                    metrics: vec!["oom_kill_count".to_string()],
                },
            ],
            root_causes: vec![
                RootCause {
                    description: "容器内存limit设置不足".to_string(),
                    confidence: 0.9,
                    verification: "对比container_memory_working_set_bytes和memory.limit_in_bytes".to_string(),
                },
                RootCause {
                    description: "应用存在内存泄漏".to_string(),
                    confidence: 0.6,
                    verification: "观察memory_usage趋势是否持续上升".to_string(),
                },
            ],
            remediation: vec![
                RemediationStep {
                    step: 1,
                    action: "临时增加容器内存limit".to_string(),
                    expected_outcome: "OOM停止，服务稳定".to_string(),
                    risk: Some("可能导致节点内存耗尽".to_string()),
                },
                RemediationStep {
                    step: 2,
                    action: "分析应用内存使用，修复泄漏".to_string(),
                    expected_outcome: "长期解决方案".to_string(),
                    risk: None,
                },
            ],
            references: vec![
                "https://gitee.com/openeuler/witty-ops-cases/oom-analysis".to_string(),
            ],
            severity: 9,
            confidence: 0.9,
        };

        // 案例3: 网络延迟异常
        let case3 = FaultCase {
            case_id: "euler-network-latency-001".to_string(),
            title: "容器网络延迟P99异常升高".to_string(),
            description: "网络延迟P99超过100ms，可能原因包括：
                         1) TCP重传率升高 2) 网络队列拥塞 3) DNS解析延迟".to_string(),
            evidence_types: vec!["network".to_string()],
            metric_patterns: vec![
                MetricPattern {
                    metric_name: "latency_p99_ms".to_string(),
                    operator: ">".to_string(),
                    threshold: 100.0,
                    description: "网络延迟P99超过100ms".to_string(),
                },
                MetricPattern {
                    metric_name: "retransmit_rate".to_string(),
                    operator: ">".to_string(),
                    threshold: 1.0,
                    description: "TCP重传率超过1%".to_string(),
                },
            ],
            skills: vec![
                DiagnosisSkill {
                    skill_id: "check_tcp_retransmit".to_string(),
                    name: "检查TCP重传".to_string(),
                    description: "使用bpftrace跟踪tcp_retransmit_skb".to_string(),
                    required_evidence: vec!["network".to_string()],
                    check_method: CheckMethod::ThresholdCheck {
                        metric: "tcp_retransmit_count".to_string(),
                        operator: ">".to_string(),
                        threshold: 10.0,
                    },
                    metrics: vec!["retransmit_rate".to_string(), "tcp_rto".to_string()],
                },
            ],
            root_causes: vec![
                RootCause {
                    description: "网络丢包导致TCP重传".to_string(),
                    confidence: 0.75,
                    verification: "对比packet_drop_count和retransmit_count".to_string(),
                },
                RootCause {
                    description: "网络队列长度不足".to_string(),
                    confidence: 0.6,
                    verification: "检查netdev_budget和tx_queue_len".to_string(),
                },
            ],
            remediation: vec![
                RemediationStep {
                    step: 1,
                    action: "调整TCP拥塞控制算法".to_string(),
                    expected_outcome: "重传率下降".to_string(),
                    risk: None,
                },
                RemediationStep {
                    step: 2,
                    action: "增加网络队列长度".to_string(),
                    expected_outcome: "减少丢包".to_string(),
                    risk: Some("可能增加延迟抖动".to_string()),
                },
            ],
            references: vec![
                "https://gitee.com/openeuler/witty-ops-cases/network-latency".to_string(),
            ],
            severity: 6,
            confidence: 0.75,
        };

        // 案例4: 磁盘I/O延迟
        let case4 = FaultCase {
            case_id: "euler-io-latency-001".to_string(),
            title: "块设备I/O延迟导致应用卡顿".to_string(),
            description: "块设备I/O延迟P99超过100ms，
                         可能是存储设备性能瓶颈或I/O队列深度不足".to_string(),
            evidence_types: vec!["block_io".to_string()],
            metric_patterns: vec![
                MetricPattern {
                    metric_name: "io_latency_p99_ms".to_string(),
                    operator: ">".to_string(),
                    threshold: 100.0,
                    description: "I/O延迟P99超过100ms".to_string(),
                },
                MetricPattern {
                    metric_name: "io_timeout_count".to_string(),
                    operator: ">".to_string(),
                    threshold: 0.0,
                    description: "存在I/O超时".to_string(),
                },
            ],
            skills: vec![
                DiagnosisSkill {
                    skill_id: "check_io_latency".to_string(),
                    name: "检查块设备I/O延迟".to_string(),
                    description: "使用bpftrace跟踪blk_account_io_start/done".to_string(),
                    required_evidence: vec!["block_io".to_string()],
                    check_method: CheckMethod::ThresholdCheck {
                        metric: "io_latency_p99_ms".to_string(),
                        operator: ">".to_string(),
                        threshold: 100.0,
                    },
                    metrics: vec!["io_latency_p50_ms".to_string(), "io_latency_p99_ms".to_string()],
                },
            ],
            root_causes: vec![
                RootCause {
                    description: "存储设备性能不足".to_string(),
                    confidence: 0.7,
                    verification: "检查设备型号和fio基准测试结果".to_string(),
                },
                RootCause {
                    description: "I/O队列深度不足".to_string(),
                    confidence: 0.6,
                    verification: "检查/sys/block/{dev}/queue/nr_requests".to_string(),
                },
            ],
            remediation: vec![
                RemediationStep {
                    step: 1,
                    action: "增加I/O队列深度".to_string(),
                    expected_outcome: "提高并发I/O能力".to_string(),
                    risk: Some("可能增加内存占用".to_string()),
                },
                RemediationStep {
                    step: 2,
                    action: "升级存储设备或切换高性能存储类".to_string(),
                    expected_outcome: "从根本上解决延迟问题".to_string(),
                    risk: None,
                },
            ],
            references: vec![
                "https://gitee.com/openeuler/witty-ops-cases/io-latency".to_string(),
            ],
            severity: 7,
            confidence: 0.8,
        };

        // 注册所有案例
        self.register_case(case1);
        self.register_case(case2);
        self.register_case(case3);
        self.register_case(case4);
    }

    /// 注册单个案例并更新索引
    fn register_case(&mut self, case: FaultCase) {
        let case_id = case.case_id.clone();
        
        // 更新证据类型索引
        for etype in &case.evidence_types {
            self.evidence_index
                .entry(etype.clone())
                .or_default()
                .push(case_id.clone());
        }
        
        // 更新技能索引
        for skill in &case.skills {
            self.skill_index
                .entry(skill.skill_id.clone())
                .or_default()
                .push(case_id.clone());
        }
        
        // 存储案例
        self.cases.insert(case_id, case);
    }

    /// 根据ID获取案例
    pub fn get_case(&self, case_id: &str) -> Option<&FaultCase> {
        self.cases.get(case_id)
    }

    /// 列出所有案例
    pub fn list_cases(&self) -> Vec<&FaultCase> {
        self.cases.values().collect()
    }

    /// 根据证据类型查找相关案例
    pub fn find_cases_by_evidence(&self, evidence_type: &str) -> Vec<&FaultCase> {
        self.evidence_index
            .get(evidence_type)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.cases.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 根据指标模式匹配案例
    pub fn match_cases_by_metrics(&self, metrics: &HashMap<String, f64>) -> Vec<(&FaultCase, f64)> {
        let mut matches = Vec::new();
        
        for case in self.cases.values() {
            let mut match_score = 0.0;
            let total_patterns = case.metric_patterns.len() as f64;
            
            for pattern in &case.metric_patterns {
                if let Some(value) = metrics.get(&pattern.metric_name) {
                    let matched = match pattern.operator.as_str() {
                        ">" => *value > pattern.threshold,
                        "<" => *value < pattern.threshold,
                        ">=" => *value >= pattern.threshold,
                        "<=" => *value <= pattern.threshold,
                        "==" => (*value - pattern.threshold).abs() < 0.001,
                        _ => false,
                    };
                    
                    if matched {
                        // 根据超出阈值的程度计算匹配分数
                        let ratio = if pattern.threshold > 0.0 {
                            (*value / pattern.threshold).abs()
                        } else {
                            1.0
                        };
                        match_score += ratio.min(2.0); // 最高2倍权重
                    }
                }
            }
            
            if match_score > 0.0 && total_patterns > 0.0 {
                let confidence = (match_score / total_patterns).min(1.0);
                matches.push((case, confidence));
            }
        }
        
        // 按置信度排序
        matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        matches
    }

    /// 导出YAML格式的案例库
    pub fn export_yaml(&self) -> Result<String, serde_yaml::Error> {
        let cases: Vec<&FaultCase> = self.cases.values().collect();
        serde_yaml::to_string(&cases)
    }
}

impl Default for CaseLibrary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_library_load_defaults() {
        let library = CaseLibrary::new();
        assert!(library.cases.len() >= 4);
    }

    #[test]
    fn test_find_cases_by_evidence() {
        let library = CaseLibrary::new();
        let cases = library.find_cases_by_evidence("cgroup_contention");
        assert!(!cases.is_empty());
    }

    #[test]
    fn test_match_cases_by_metrics() {
        let library = CaseLibrary::new();
        let mut metrics = HashMap::new();
        metrics.insert("cpu_throttle_rate".to_string(), 10.0); // 超过阈值5.0
        
        let matches = library.match_cases_by_metrics(&metrics);
        assert!(!matches.is_empty());
        
        // 第一个匹配应该是置信度最高的
        let (case, confidence) = &matches[0];
        assert!(*confidence > 0.0);
        assert!(case.case_id.contains("cpu-contention"));
    }

    #[test]
    fn test_get_case() {
        let library = CaseLibrary::new();
        let case = library.get_case("euler-cpu-contention-001");
        assert!(case.is_some());
        assert_eq!(case.unwrap().case_id, "euler-cpu-contention-001");
    }

    #[test]
    fn test_export_yaml() {
        let library = CaseLibrary::new();
        let yaml = library.export_yaml();
        assert!(yaml.is_ok());
        let yaml_str = yaml.unwrap();
        assert!(yaml_str.contains("case_id"));
    }
}
