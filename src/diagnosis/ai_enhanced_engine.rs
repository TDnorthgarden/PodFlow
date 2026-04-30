//! AI 增强诊断引擎
//!
//! 集成 AI 分析到诊断流程，支持异步后台处理。
//! 异步处理已统一使用 async_bridge 的 AiTaskQueue，不再维护独立通道。

use crate::ai::{
    AiAdapter, AiAdapterConfig, AiEnhancedDiagnosis, AiFallbackMode,
    EvidenceSufficiency, InsufficientReason, EvidenceCheckConfig,
    async_bridge::{AiTaskQueue, AiTask, AiTaskPriority, AiResultStore},
    llm_client::LlmConfig,
};
use crate::diagnosis::engine::RuleEngine;
use crate::types::diagnosis::{DiagnosisResult, DiagnosisStatus, AiStatus};
use crate::types::evidence::Evidence;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// AI 增强诊断引擎
pub struct AiEnhancedEngine {
    /// 基础规则引擎
    rule_engine: RuleEngine,
    /// AI 适配器
    ai_adapter: Option<AiAdapter>,
    /// 统一的异步任务队列（async_bridge）
    ai_task_queue: Option<Arc<AiTaskQueue>>,
    /// AI 结果存储（异步任务完成后的结果）
    ai_results: Arc<AiResultStore>,
    /// 证据检查配置
    evidence_check_config: EvidenceCheckConfig,
}

/// AI 增强诊断配置
#[derive(Debug, Clone)]
pub struct AiEngineConfig {
    /// 是否启用 AI 增强
    pub enabled: bool,
    /// AI 适配器配置
    pub ai_config: Option<AiAdapterConfig>,
    /// LLM 配置
    pub llm_config: Option<LlmConfig>,
    /// 是否启用异步处理
    pub enable_async: bool,
    /// 异步工作线程数
    pub worker_threads: usize,
    /// AI 结果 TTL（秒），默认 3600（1小时）
    pub result_ttl_secs: u64,
    /// 证据检查配置
    pub evidence_check_config: Option<EvidenceCheckConfig>,
}

impl Default for AiEngineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ai_config: None,
            llm_config: None,
            enable_async: true,
            worker_threads: 2,
            result_ttl_secs: 3600,
            evidence_check_config: None,
        }
    }
}

impl AiEnhancedEngine {
    /// 创建新的 AI 增强诊断引擎
    pub fn new(rule_engine: RuleEngine, config: AiEngineConfig) -> Self {
        let evidence_check_config = config.evidence_check_config.clone().unwrap_or_default();

        let ai_adapter = if config.enabled {
            if let Some(ai_config) = config.ai_config {
                Some(AiAdapter::with_evidence_check(ai_config, evidence_check_config.clone()))
            } else {
                warn!("AI enabled but no config provided");
                None
            }
        } else {
            None
        };

        Self {
            rule_engine,
            ai_adapter,
            ai_task_queue: None,
            ai_results: Arc::new(AiResultStore::new()),
            evidence_check_config,
        }
    }

    /// 绑定统一的异步任务队列（由外部 async_bridge 管理）
    pub fn with_task_queue(mut self, queue: Arc<AiTaskQueue>, result_store: Arc<AiResultStore>) -> Self {
        self.ai_task_queue = Some(queue);
        self.ai_results = result_store;
        self
    }

    /// 快速创建（从环境变量读取配置）
    pub fn from_env(rule_engine: RuleEngine) -> Self {
        let enabled = std::env::var("NUTS_AI_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let api_key = std::env::var("NUTS_AI_API_KEY").ok();
        let endpoint = std::env::var("NUTS_AI_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:8000/v1/chat/completions".to_string());
        let result_ttl_secs = std::env::var("NUTS_AI_RESULT_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        let config = AiEngineConfig {
            enabled,
            ai_config: if enabled {
                Some(AiAdapterConfig {
                    endpoint,
                    api_key,
                    timeout_secs: 60,
                    max_retries: 2,
                    fallback_mode: AiFallbackMode::KeepOriginal,
                    model: "nuts-ai-diagnosis".to_string(),
                })
            } else {
                None
            },
            llm_config: None,
            enable_async: true,
            worker_threads: 2,
            result_ttl_secs,
            evidence_check_config: None,
        };

        Self::new(rule_engine, config)
    }

    /// 诊断（同步快速响应 + 可选异步 AI 增强）
    pub async fn diagnose(&self, evidences: &[Evidence]) -> DiagnosisResult {
        // 1. 基础诊断（快速规则引擎）
        let base_diagnosis = self.rule_engine.diagnose(evidences);

        // 2. 检查是否需要 AI 增强（证据充分性门控）
        if let Some(ref adapter) = self.ai_adapter {
            match adapter.check_evidence_sufficiency(&base_diagnosis, evidences) {
                EvidenceSufficiency::Sufficient => {
                    if let Some(ref queue) = self.ai_task_queue {
                        // 异步处理：通过统一的 async_bridge 队列提交
                        let max_severity = base_diagnosis.conclusions.iter()
                            .filter_map(|c| c.severity)
                            .max()
                            .unwrap_or(0);
                        let priority = if max_severity >= 9 {
                            AiTaskPriority::Critical
                        } else if max_severity >= 7 {
                            AiTaskPriority::High
                        } else {
                            AiTaskPriority::Normal
                        };

                        let task = AiTask::new(
                            base_diagnosis.task_id.clone(),
                            base_diagnosis.clone(),
                            evidences.to_vec(),
                            priority,
                        );

                        match queue.submit(task).await {
                            Ok(_) => {
                                info!(
                                    "AI analysis queued for task {} (priority={:?})",
                                    base_diagnosis.task_id, priority
                                );
                            }
                            Err(e) => {
                                warn!("Failed to submit AI task: {}", e);
                            }
                        }
                        base_diagnosis
                    } else {
                        // 同步处理：直接调用 AI
                        match adapter.process(&base_diagnosis, evidences).await {
                            enhanced_result if enhanced_result.ai_status == AiStatus::Ok => {
                                enhanced_result.enhanced
                            }
                            _ => {
                                warn!("AI analysis failed or skipped, returning base diagnosis");
                                base_diagnosis
                            }
                        }
                    }
                }
                EvidenceSufficiency::Insufficient(reason) => {
                    info!(
                        "AI enhancement skipped for task {}: evidence insufficient ({:?})",
                        base_diagnosis.task_id, reason
                    );
                    base_diagnosis
                }
            }
        } else {
            base_diagnosis
        }
    }

    /// 获取 AI 增强的诊断（如果已完成）
    pub async fn get_ai_enhanced_diagnosis(&self, task_id: &str) -> Option<AiEnhancedDiagnosis> {
        self.ai_results.get(task_id).await
    }

    /// 列出所有 AI 增强诊断结果
    pub async fn list_ai_diagnoses(&self) -> Vec<(String, AiEnhancedDiagnosis)> {
        self.ai_results.list_all().await
    }

    /// 按状态查询 AI 诊断结果
    pub async fn find_by_status(&self, status: AiStatus) -> Vec<(String, AiEnhancedDiagnosis)> {
        self.ai_results.list_all().await
            .into_iter()
            .filter(|(_, v)| v.ai_status == status)
            .collect()
    }

    /// 健康检查
    pub async fn health_check(&self) -> AiEngineHealth {
        let ai_healthy = self.ai_adapter.is_some();
        let queue_healthy = self.ai_task_queue.is_some();

        AiEngineHealth {
            rule_engine_healthy: true,
            ai_adapter_healthy: ai_healthy,
            async_queue_healthy: queue_healthy,
        }
    }
}

/// AI 引擎健康状态
#[derive(Debug, Clone)]
pub struct AiEngineHealth {
    pub rule_engine_healthy: bool,
    pub ai_adapter_healthy: bool,
    pub async_queue_healthy: bool,
}

impl AiEngineHealth {
    pub fn all_healthy(&self) -> bool {
        self.rule_engine_healthy && self.ai_adapter_healthy && self.async_queue_healthy
    }
}

/// 诊断结果增强器
pub struct DiagnosisEnhancer;

impl DiagnosisEnhancer {
    /// 使用 AI 输出增强诊断结果
    pub fn enhance(diagnosis: &mut DiagnosisResult, ai_output: &crate::ai::AiOutput) {
        // 添加 AI 解释
        if let Some(ref mut ai_info) = diagnosis.ai {
            // 更新现有 AI 信息
            ai_info.summary = Some(ai_output.explanation.clone());
            ai_info.completed_at_ms = Some(chrono::Utc::now().timestamp_millis());
        } else {
            // 创建新的 AI 信息
            diagnosis.ai = Some(crate::types::diagnosis::AiInfo {
                enabled: true,
                status: crate::types::diagnosis::AiStatus::Ok,
                summary: Some(ai_output.explanation.clone()),
                version: Some("ai-model".to_string()),
                submitted_at_ms: Some(chrono::Utc::now().timestamp_millis()),
                completed_at_ms: Some(chrono::Utc::now().timestamp_millis()),
                processing_duration_ms: Some(0),
            });
        }

        // 增强结论
        for conclusion in &mut diagnosis.conclusions {
            // 如果 AI 提供了更详细的根因分析，添加进去
            if !ai_output.root_cause_analysis.is_empty() {
                let enhanced_details = format!(
                    "{}",
                    ai_output.root_cause_analysis
                );
                conclusion.details = Some(serde_json::json!(enhanced_details));
            }
        }

        // 添加 AI 建议
        for step in &ai_output.troubleshooting_steps {
            diagnosis.recommendations.push(crate::types::diagnosis::Recommendation {
                action: step.clone(),
                priority: 1,
                expected_impact: Some("AI建议".to_string()),
                verification: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_engine_config_default() {
        let config = AiEngineConfig::default();
        assert!(!config.enabled);
        assert!(config.enable_async);
        assert_eq!(config.worker_threads, 2);
    }

    #[test]
    fn test_ai_health() {
        let health = AiEngineHealth {
            rule_engine_healthy: true,
            ai_adapter_healthy: true,
            async_queue_healthy: true,
        };
        assert!(health.all_healthy());

        let unhealthy = AiEngineHealth {
            rule_engine_healthy: true,
            ai_adapter_healthy: false,
            async_queue_healthy: true,
        };
        assert!(!unhealthy.all_healthy());
    }
}
