//! AI 适配层 - 将诊断证据转换为 AI 可理解的输入格式，并处理 AI 输出回填
//!
//! 核心功能：
//! 1. 将 Evidence + DiagnosisResult 转换为 AI 入参（结构化提示词）
//! 2. 解析 AI 输出并回填到诊断结果
//! 3. 支持降级策略（AI 不可用时保持核心链路）
//! 4. 异步AI增强（后台处理，不阻塞主链路）

pub mod async_bridge;
pub mod llm_client;

// 重新导出常用类型（AiEnhancedDiagnosis 在本模块定义）
pub use async_bridge::{AiResultStore, AiTask, AiTaskQueue};

use crate::types::diagnosis::{DiagnosisResult, Recommendation, AiStatus};
use crate::types::evidence::Evidence;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

/// AI 适配器配置
#[derive(Debug, Clone)]
pub struct AiAdapterConfig {
    /// AI 服务端点
    pub endpoint: String,
    /// API 密钥
    pub api_key: Option<String>,
    /// 请求超时（秒）
    pub timeout_secs: u64,
    /// 最大重试次数
    pub max_retries: u32,
    /// 降级模式：当 AI 不可用时如何处理
    pub fallback_mode: AiFallbackMode,
    /// 模型名称
    pub model: String,
}

impl Default for AiAdapterConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8000/v1/chat/completions".to_string(),
            api_key: None,
            timeout_secs: 60,
            max_retries: 2,
            fallback_mode: AiFallbackMode::KeepOriginal,
            model: "nuts-ai-diagnosis".to_string(),
        }
    }
}

/// AI 降级模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiFallbackMode {
    /// 保留原始诊断结果，仅添加 AI 解释（推荐）
    KeepOriginal,
    /// 降低置信度标记
    ReduceConfidence,
    /// 标记为待人工审核
    MarkForReview,
    /// 证据不足时跳过 AI，不做无意义调用
    SkipAi,
}

/// 证据充分性评估结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceSufficiency {
    /// 证据充分，可以调用 AI
    Sufficient,
    /// 证据不足，不建议调用 AI
    Insufficient(InsufficientReason),
}

/// 证据不足的原因
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsufficientReason {
    /// 没有任何证据
    EmptyEvidences,
    /// 所有证据都无有效指标和事件
    NoValidMetrics,
    /// 有效指标数量不足，无法支撑有意义的 AI 分析
    InsufficientMetrics,
    /// 证据条数不足，信息量不够
    InsufficientEvidenceCount,
    /// 规则引擎未产出任何结论，AI 缺乏分析基础
    NoConclusions,
    /// 结论置信度均足够高，无需 AI 增强
    HighConfidenceConclusions,
}

/// 证据充分性检查配置
#[derive(Debug, Clone)]
pub struct EvidenceCheckConfig {
    /// 证据条数下限：低于此数量认为信息量不足
    pub min_evidence_count: usize,
    /// 指标摘要中至少需要多少个有效（非零）指标
    pub min_valid_metrics: usize,
    /// 事件拓扑中至少需要多少个事件
    pub min_events: usize,
    /// 指标值意义性阈值：低于此绝对值的指标视为无意义（如 0.001ms 的延迟）
    pub metric_significance_threshold: f64,
    /// 有意义指标至少需要多少个才认为证据充分
    pub min_significant_metrics: usize,
    /// 低于此置信度才需要 AI 增强
    pub confidence_threshold: f64,
}

impl Default for EvidenceCheckConfig {
    fn default() -> Self {
        Self {
            min_evidence_count: 1,
            min_valid_metrics: 2,
            min_events: 1,
            metric_significance_threshold: 0.01,
            min_significant_metrics: 1,
            confidence_threshold: 0.7,
        }
    }
}

/// AI 适配器
#[derive(Clone)]
pub struct AiAdapter {
    config: AiAdapterConfig,
    evidence_check_config: EvidenceCheckConfig,
}

/// AI 输入（提示词上下文）
#[derive(Debug, Serialize)]
pub struct AiInput {
    /// 系统提示词
    pub system_prompt: String,
    /// 用户提示词（结构化证据和诊断）
    pub user_prompt: String,
    /// 证据列表（JSON 格式）
    pub evidence_context: serde_json::Value,
    /// 诊断结果（JSON 格式）
    pub diagnosis_context: serde_json::Value,
    /// 任务元数据
    pub metadata: AiInputMetadata,
}

/// AI 输入元数据
#[derive(Debug, Serialize)]
pub struct AiInputMetadata {
    pub task_id: String,
    pub schema_version: String,
    pub evidence_types: Vec<String>,
    pub target_pod: Option<String>,
    pub time_window_ms: i64,
}

/// AI 输出结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiOutput {
    /// AI 生成的解释
    pub explanation: String,
    /// 建议的排查路径
    pub troubleshooting_steps: Vec<String>,
    /// 根因分析
    pub root_cause_analysis: String,
    /// 置信度评估（AI 对结论的置信度）
    pub ai_confidence: f64,
    /// 需要关注的额外指标
    pub suggested_metrics: Vec<String>,
    /// 推荐工具或命令
    pub suggested_commands: Vec<String>,
}

/// 聊天完成响应结构（支持 OpenAI 和本地模型格式）
#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    choices: Option<Vec<ChatCompletionChoice>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,  // 本地模型可能直接返回 output
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>, // 本地模型可能直接返回 content
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<ChatCompletionUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    index: Option<i32>,
    message: Option<ChatCompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_tokens: Option<i32>,
}

/// AI 增强后的诊断结果
#[derive(Debug, Clone)]
pub struct AiEnhancedDiagnosis {
    /// 原始诊断结果
    pub original: DiagnosisResult,
    /// AI 输出
    pub ai_output: Option<AiOutput>,
    /// 增强后的诊断（合并 AI 建议）
    pub enhanced: DiagnosisResult,
    /// AI 调用状态
    pub ai_status: AiStatus,
    /// 处理耗时（毫秒）
    pub processing_ms: i64,
    /// 创建时间戳（用于 TTL）
    pub created_at: std::time::Instant,
}

impl AiAdapter {
    /// 创建新的 AI 适配器
    pub fn new(config: AiAdapterConfig) -> Self {
        Self {
            config,
            evidence_check_config: EvidenceCheckConfig::default(),
        }
    }

    /// 创建带证据检查配置的 AI 适配器
    pub fn with_evidence_check(config: AiAdapterConfig, evidence_check: EvidenceCheckConfig) -> Self {
        Self {
            config,
            evidence_check_config: evidence_check,
        }
    }

    /// 评估证据充分性
    ///
    /// 在调用 AI 前检查证据是否足够支撑有意义的分析，
    /// 避免在证据不足时浪费 AI 资源或产生无根据的建议。
    pub fn check_evidence_sufficiency(
        &self,
        diagnosis: &DiagnosisResult,
        evidences: &[Evidence],
    ) -> EvidenceSufficiency {
        // 1. 证据为空
        if evidences.is_empty() {
            return EvidenceSufficiency::Insufficient(InsufficientReason::EmptyEvidences);
        }

        // 2. 证据条数不足
        if evidences.len() < self.evidence_check_config.min_evidence_count {
            return EvidenceSufficiency::Insufficient(InsufficientReason::InsufficientEvidenceCount);
        }

        // 3. 检查是否有有效指标或事件（排除全空证据）
        let has_valid_evidence = evidences.iter().any(|e| {
            let valid_metrics = e.metric_summary.values().filter(|v| **v != 0.0).count();
            let valid_events = e.events_topology.len();
            valid_metrics >= self.evidence_check_config.min_valid_metrics
                || valid_events >= self.evidence_check_config.min_events
        });
        if !has_valid_evidence {
            return EvidenceSufficiency::Insufficient(InsufficientReason::NoValidMetrics);
        }

        // 4. 检查有意义的指标数量（绝对值过小的指标不具备分析价值）
        let significant_metric_count: usize = evidences.iter()
            .map(|e| {
                e.metric_summary.values()
                    .filter(|v| v.abs() >= self.evidence_check_config.metric_significance_threshold)
                    .count()
            })
            .sum();
        if significant_metric_count < self.evidence_check_config.min_significant_metrics {
            return EvidenceSufficiency::Insufficient(InsufficientReason::InsufficientMetrics);
        }

        // 5. 无结论时 AI 缺乏分析基础
        if diagnosis.conclusions.is_empty() {
            return EvidenceSufficiency::Insufficient(InsufficientReason::NoConclusions);
        }

        // 6. 所有结论置信度都足够高，无需 AI 增强
        let all_high_confidence = diagnosis.conclusions.iter()
            .all(|c| c.confidence >= self.evidence_check_config.confidence_threshold);
        if all_high_confidence {
            return EvidenceSufficiency::Insufficient(InsufficientReason::HighConfidenceConclusions);
        }

        EvidenceSufficiency::Sufficient
    }

    /// 对证据做摘要，避免全量序列化导致 token 过长
    ///
    /// 当证据数量超过阈值时，只保留关键指标和 Top-N 事件
    pub fn summarize_evidences(&self, evidences: &[Evidence], max_evidences: usize, max_events_per_evidence: usize) -> Vec<Evidence> {
        if evidences.len() <= max_evidences {
            // 数量未超限，但仍需裁剪每个证据的事件数
            return evidences.iter().map(|e| {
                let mut summarized = e.clone();
                if summarized.events_topology.len() > max_events_per_evidence {
                    summarized.events_topology.truncate(max_events_per_evidence);
                }
                summarized
            }).collect();
        }

        // 超限时：优先保留有有效指标的证据，其次按事件数量排序
        let mut indexed: Vec<(usize, usize, usize)> = evidences.iter().enumerate().map(|(i, e)| {
            let metric_count = e.metric_summary.values().filter(|v| **v != 0.0).count();
            let event_count = e.events_topology.len();
            (i, metric_count, event_count)
        }).collect();

        // 按有效指标数降序，再按事件数降序
        indexed.sort_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2))
        });

        indexed.into_iter()
            .take(max_evidences)
            .map(|(i, _, _)| {
                let mut e = evidences[i].clone();
                if e.events_topology.len() > max_events_per_evidence {
                    e.events_topology.truncate(max_events_per_evidence);
                }
                e
            })
            .collect()
    }

    /// 解析 AI 输出，支持多种格式的容错提取
    ///
    /// 优先尝试直接解析 JSON，失败后尝试从 markdown 代码块中提取 JSON
    fn parse_ai_output(&self, content: &str) -> Result<AiOutput, AiError> {
        // 1. 直接解析 JSON
        if let Ok(output) = serde_json::from_str::<AiOutput>(content) {
            return Ok(output);
        }

        // 2. 从 markdown 代码块中提取 JSON（如 ```json ... ```）
        let trimmed = content.trim();
        if let Some(json_str) = Self::extract_json_from_code_block(trimmed) {
            if let Ok(output) = serde_json::from_str::<AiOutput>(&json_str) {
                return Ok(output);
            }
        }

        // 3. 尝试找到内容中第一个 { 到最后一个 } 之间的 JSON
        if let Some(start) = content.find('{') {
            if let Some(end) = content.rfind('}') {
                if start < end {
                    let json_candidate = &content[start..=end];
                    if let Ok(output) = serde_json::from_str::<AiOutput>(json_candidate) {
                        return Ok(output);
                    }
                }
            }
        }

        Err(AiError::InvalidResponse(format!(
            "AI response is not valid JSON (attempted direct parse, code block extraction, and brace matching). Raw content (first 500 chars): {}",
            &content[..content.len().min(500)]
        )))
    }

    /// 从 markdown 代码块中提取 JSON 内容
    fn extract_json_from_code_block(content: &str) -> Option<String> {
        // 匹配 ```json ... ``` 或 ``` ... ```
        let patterns = ["```json", "```JSON", "```"];
        for pattern in patterns {
            if let Some(start_idx) = content.find(pattern) {
                let json_start = start_idx + pattern.len();
                // 跳过开头的换行
                let json_start = content[json_start..]
                    .find(|c: char| !c.is_whitespace())
                    .map(|i| json_start + i)
                    .unwrap_or(json_start);

                if let Some(end_idx) = content[json_start..].find("```") {
                    return Some(content[json_start..json_start + end_idx].trim().to_string());
                }
            }
        }
        None
    }

    /// 构建 AI 输入（提示词工程）
    ///
    /// 将证据和诊断结果转换为 AI 可理解的结构化提示词。
    /// 证据数量过多时会自动做摘要，避免 token 超限。
    pub fn build_input(&self, diagnosis: &DiagnosisResult, evidences: &[Evidence]) -> AiInput {
        // 对证据做摘要（最多 10 条证据，每条最多 20 个事件）
        let summarized_evidences = self.summarize_evidences(evidences, 10, 20);

        let system_prompt = self.build_system_prompt();
        let user_prompt = self.build_user_prompt(diagnosis, &summarized_evidences);

        let evidence_context = serde_json::to_value(&summarized_evidences)
            .unwrap_or_else(|_| json!({"error": "serialization failed"}));

        let diagnosis_context = serde_json::to_value(diagnosis)
            .unwrap_or_else(|_| json!({"error": "serialization failed"}));

        let evidence_types: Vec<String> = summarized_evidences
            .iter()
            .map(|e| e.evidence_type.clone())
            .collect();

        // 注意：time_window 在 Evidence 中，不在 DiagnosisResult 中
        // 这里使用默认值或从证据中计算
        let time_window_ms = 5000; // 默认 5 秒

        let metadata = AiInputMetadata {
            task_id: diagnosis.task_id.clone(),
            schema_version: "ai.v0.1".to_string(),
            evidence_types,
            target_pod: diagnosis.evidence_refs.first()
                .and_then(|r| r.scope_key.clone()),
            time_window_ms,
        };

        AiInput {
            system_prompt,
            user_prompt,
            evidence_context,
            diagnosis_context,
            metadata,
        }
    }

    /// 构建系统提示词
    fn build_system_prompt(&self) -> String {
        r#"你是一个专业的容器故障诊断专家。你的任务是分析系统采集的观测证据和诊断引擎的初步结论，提供详细的故障解释、根因分析和排查建议。

你需要：
1. 解释当前的诊断结论（为什么这些证据指向这些结论）
2. 分析可能的根因（从系统调用、I/O、网络等多维度）
3. 提供可执行的排查步骤（具体的命令或工具）
4. 指出需要额外关注的指标或日志
5. 评估当前结论的可信度

输出格式必须是 JSON：
{
    "explanation": "详细的故障解释",
    "troubleshooting_steps": ["步骤1", "步骤2"],
    "root_cause_analysis": "根因分析",
    "ai_confidence": 0.85,
    "suggested_metrics": ["metric1", "metric2"],
    "suggested_commands": ["command1", "command2"]
}

注意：
- ai_confidence 必须在 0.0 到 1.0 之间
- troubleshooting_steps 必须可执行
- suggested_commands 应该是 Linux 命令行可运行的命令
"#.to_string()
    }

    /// 构建用户提示词
    fn build_user_prompt(&self, diagnosis: &DiagnosisResult, evidences: &[Evidence]) -> String {
        let mut prompt = format!(
            "## 诊断任务\n\n任务 ID: {}\n",
            diagnosis.task_id
        );

        // 添加证据概览
        prompt.push_str("\n### 采集的证据\n\n");
        for evidence in evidences {
            prompt.push_str(&format!(
                "- **{}** (scope: {})\n",
                evidence.evidence_type,
                evidence.scope.scope_key
            ));
            
            // 关键指标
            if !evidence.metric_summary.is_empty() {
                prompt.push_str("  - 指标: ");
                let metrics: Vec<String> = evidence.metric_summary
                    .iter()
                    .map(|(k, v)| format!("{}={:.2}", k, v))
                    .collect();
                prompt.push_str(&metrics.join(", "));
                prompt.push('\n');
            }

            // 事件
            if !evidence.events_topology.is_empty() {
                prompt.push_str("  - 事件: ");
                let events: Vec<String> = evidence.events_topology
                    .iter()
                    .map(|e| format!("{}(severity={})", e.event_type, e.severity.unwrap_or(0)))
                    .collect();
                prompt.push_str(&events.join(", "));
                prompt.push('\n');
            }
        }

        // 添加诊断引擎结论
        prompt.push_str("\n### 诊断引擎结论\n\n");
        for (i, conclusion) in diagnosis.conclusions.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. {} (置信度: {:.2})\n",
                i + 1,
                conclusion.title,
                conclusion.confidence
            ));
            if let Some(details) = &conclusion.details {
                prompt.push_str(&format!("   详情: {}\n", details));
            }
        }

        // 添加建议
        prompt.push_str("\n### 建议\n\n");
        for (i, rec) in diagnosis.recommendations.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. {} (优先级: {})\n",
                i + 1,
                rec.action,
                rec.priority
            ));
        }

        prompt.push_str("\n---\n\n请基于以上信息，提供你的分析和建议。");
        prompt
    }

    /// 调用 AI 服务（OpenAI 格式兼容）
    /// 
    /// 支持 OpenAI API 格式及兼容的 LLM 服务（如 vLLM、LocalAI 等）
    /// 
    /// 注意：本地模型（如 llama.cpp、text-generation-inference）
    /// 通常不需要 API key，此时 api_key 应为 None
    pub async fn call_ai(&self, input: &AiInput) -> Result<AiOutput, AiError> {
        // 构建 HTTP 客户端
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .build()
            .map_err(|e| AiError::HttpError(format!("Failed to create HTTP client: {}", e)))?;

        // 构建请求体（本地模型格式 - input/system_prompt）
        let request_body = json!({
            "model": self.config.model,
            "system_prompt": &input.system_prompt,
            "input": &input.user_prompt,
            "temperature": 0.3,
        });

        // 构建请求（本地模型不需要 API key）
        let mut request_builder = client
            .post(&self.config.endpoint)
            .header("Content-Type", "application/json")
            .json(&request_body);
        
        // 如果配置了 API key，添加认证头
        if let Some(ref api_key) = self.config.api_key {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        // 发送请求
        let response = request_builder
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    AiError::Timeout
                } else {
                    AiError::HttpError(format!("Request failed: {}", e))
                }
            })?;

        // 检查响应状态
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AiError::HttpError(format!(
                "AI service returned error (HTTP {}): {}",
                status, error_text
            )));
        }

        // 解析响应
        let chat_response: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| AiError::SerializationError(format!("Failed to parse response: {}", e)))?;

        // 提取 AI 回复内容（支持多种响应格式）
        let content = chat_response
            .output  // 本地模型格式：直接返回 output
            .or(chat_response.content)  // 或者 content 字段
            .or_else(|| {
                // OpenAI 格式：从 choices[0].message.content 提取
                chat_response.choices.as_ref().and_then(|choices| {
                    choices.first().and_then(|c| {
                        c.message.as_ref().and_then(|m| m.content.clone())
                    })
                })
            })
            .ok_or_else(|| AiError::InvalidResponse("Empty response from AI".to_string()))?;

        // 解析 JSON 格式的 AI 输出（带容错：尝试从 markdown 代码块中提取）
        let ai_output: AiOutput = self.parse_ai_output(&content)?;

        tracing::info!(
            "AI call successful for task {} (confidence: {})",
            input.metadata.task_id,
            ai_output.ai_confidence
        );

        Ok(ai_output)
    }

    /// 增强诊断结果（合并 AI 建议）
    /// 
    /// 将 AI 输出合并到原始诊断结果中
    pub fn enhance_diagnosis(&self, original: &DiagnosisResult, ai_output: &AiOutput) -> DiagnosisResult {
        let mut enhanced = original.clone();

        // 更新每个结论，添加 AI 解释
        for conclusion in &mut enhanced.conclusions {
            // 将 AI 解释添加到 details
            let ai_explanation = json!({
                "ai_explanation": ai_output.explanation,
                "ai_confidence": ai_output.ai_confidence,
            });
            
            if let Some(existing) = &conclusion.details {
                // 合并现有详情和 AI 解释
                let mut merged = existing.clone();
                if let Some(obj) = merged.as_object_mut() {
                    obj.insert("ai_enhancement".to_string(), ai_explanation);
                }
                conclusion.details = Some(merged);
            } else {
                // 创建新的 details 对象，包含 ai_enhancement 键
                let wrapper = json!({
                    "ai_enhancement": ai_explanation
                });
                conclusion.details = Some(wrapper);
            }
        }

        // 添加 AI 推荐的排查步骤作为建议
        for step in &ai_output.troubleshooting_steps {
            enhanced.recommendations.push(Recommendation {
                priority: 5, // 中等优先级
                action: step.clone(),
                expected_impact: Some("辅助排查".to_string()),
                verification: Some("按照步骤执行后观察指标变化".to_string()),
            });
        }

        enhanced
    }

    /// 处理诊断（带 AI 增强）
    ///
    /// 完整的 AI 增强流程：
    /// 1. 证据充分性检查 -> 2. 构建输入 -> 3. 调用 AI -> 4. 合并结果 -> 5. 返回增强结果
    pub async fn process(&self, diagnosis: &DiagnosisResult, evidences: &[Evidence]) -> AiEnhancedDiagnosis {
        let start = chrono::Utc::now().timestamp_millis();

        // 证据充分性门控：证据不足时直接降级，不浪费 AI 资源
        match self.check_evidence_sufficiency(diagnosis, evidences) {
            EvidenceSufficiency::Sufficient => {}
            EvidenceSufficiency::Insufficient(ref reason) => {
                tracing::info!(
                    "[AI] Skipping AI enhancement for task {}: evidence insufficient ({:?})",
                    diagnosis.task_id, reason
                );
                let processing_ms = chrono::Utc::now().timestamp_millis() - start;
                return AiEnhancedDiagnosis {
                    original: diagnosis.clone(),
                    ai_output: None,
                    enhanced: diagnosis.clone(),
                    ai_status: AiStatus::SkippedInsufficientEvidence,
                    processing_ms,
                    created_at: std::time::Instant::now(),
                };
            }
        }

        // 构建输入
        let input = self.build_input(diagnosis, evidences);

        // 调用 AI
        match self.call_ai(&input).await {
            Ok(ai_output) => {
                // 成功：合并结果
                let enhanced = self.enhance_diagnosis(diagnosis, &ai_output);
                let processing_ms = chrono::Utc::now().timestamp_millis() - start;

                AiEnhancedDiagnosis {
                    original: diagnosis.clone(),
                    ai_output: Some(ai_output),
                    enhanced,
                    ai_status: AiStatus::Ok,
                    processing_ms,
                    created_at: std::time::Instant::now(),
                }
            }
            Err(_) => {
                // 失败：降级处理
                let enhanced = self.apply_fallback(diagnosis);
                let processing_ms = chrono::Utc::now().timestamp_millis() - start;

                AiEnhancedDiagnosis {
                    original: diagnosis.clone(),
                    ai_output: None,
                    enhanced,
                    ai_status: AiStatus::Unavailable,
                    processing_ms,
                    created_at: std::time::Instant::now(),
                }
            }
        }
    }

    /// 应用降级策略
    pub fn apply_fallback(&self, diagnosis: &DiagnosisResult) -> DiagnosisResult {
        let mut fallback = diagnosis.clone();

        match self.config.fallback_mode {
            AiFallbackMode::KeepOriginal => {
                // 什么都不做，保留原始结果
            }
            AiFallbackMode::ReduceConfidence => {
                // 降低所有结论的置信度
                for conclusion in &mut fallback.conclusions {
                    conclusion.confidence *= 0.8; // 降低 20%
                }
            }
            AiFallbackMode::MarkForReview => {
                // 标记所有结论为待人工审核
                for conclusion in &mut fallback.conclusions {
                    if let Some(ref mut details) = conclusion.details {
                        if let Some(obj) = details.as_object_mut() {
                            obj.insert("requires_manual_review".to_string(), serde_json::json!(true));
                            obj.insert("review_reason".to_string(), serde_json::json!("AI enhancement unavailable"));
                        }
                    } else {
                        conclusion.details = Some(serde_json::json!({
                            "requires_manual_review": true,
                            "review_reason": "AI enhancement unavailable"
                        }));
                    }
                }
                // 在 ai 字段中标记
                fallback.ai = Some(crate::types::diagnosis::AiInfo {
                    enabled: true,
                    status: crate::types::diagnosis::AiStatus::Failed,
                    summary: Some("AI enhancement unavailable, marked for manual review".to_string()),
                    version: None,
                    submitted_at_ms: None,
                    completed_at_ms: None,
                    processing_duration_ms: None,
                });
            }
            AiFallbackMode::SkipAi => {
                // 不调用 AI，直接返回原始结果（在 process() 中已提前处理）
            }
        }

        fallback
    }
}

/// AI 错误
#[derive(Debug)]
pub enum AiError {
    HttpError(String),
    Timeout,
    InvalidResponse(String),
    SerializationError(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::HttpError(s) => write!(f, "HTTP error: {}", s),
            AiError::Timeout => write!(f, "Request timeout"),
            AiError::InvalidResponse(s) => write!(f, "Invalid response: {}", s),
            AiError::SerializationError(s) => write!(f, "Serialization error: {}", s),
        }
    }
}

impl std::error::Error for AiError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::diagnosis::Conclusion;
    use crate::types::evidence::{PodInfo, TimeWindow, Scope, Attribution, CollectionMeta};
    use crate::types::diagnosis::EvidenceStrength;

    fn create_test_evidence() -> Evidence {
        Evidence {
            schema_version: "evidence.v0.2".to_string(),
            task_id: "test-task".to_string(),
            evidence_id: "test-evidence".to_string(),
            evidence_type: "block_io".to_string(),
            collection: CollectionMeta {
                collection_id: "test-collection".to_string(),
                collection_status: "success".to_string(),
                probe_id: "test-probe".to_string(),
                errors: vec![],
            },
            time_window: TimeWindow {
                start_time_ms: 1000,
                end_time_ms: 2000,
                collection_interval_ms: None,
            },
            scope: Scope {
                pod: Some(PodInfo {
                    uid: Some("pod-123".to_string()),
                    name: Some("test-pod".to_string()),
                    namespace: Some("default".to_string()),
                }),
                container_id: None,
                cgroup_id: Some("cgroup-123".to_string()),
                pid_scope: None,
                scope_key: "test-scope".to_string(),
                network_target: None,
            },
            selection: None,
            metric_summary: {
                let mut m = std::collections::HashMap::new();
                m.insert("io_latency_p99_ms".to_string(), 150.0);
                m
            },
            events_topology: vec![],
            top_calls: None,
            attribution: Attribution {
                status: "nri_mapped".to_string(),
                confidence: Some(0.9),
                source: Some("nri".to_string()),
                mapping_version: None,
            },
        }
    }

    #[test]
    fn test_build_input() {
        let config = AiAdapterConfig::default();
        let adapter = AiAdapter::new(config);

        let evidence = create_test_evidence();
        let diagnosis = DiagnosisResult {
            schema_version: "diagnosis.v0.2".to_string(),
            task_id: "test-task".to_string(),
            status: crate::types::diagnosis::DiagnosisStatus::Done,
            runtime: None,
            trigger: crate::types::diagnosis::TriggerInfo {
                trigger_type: "manual".to_string(),
                trigger_reason: "test".to_string(),
                trigger_time_ms: 2000,
                matched_condition: None,
                event_type: None,
            },
            evidence_refs: vec![],
            conclusions: vec![Conclusion {
                conclusion_id: "con-1".to_string(),
                title: "I/O 延迟异常".to_string(),
                confidence: 0.85,
                evidence_strength: EvidenceStrength::High,
                severity: Some(8),
                details: None,
            }],
            recommendations: vec![],
            traceability: crate::types::diagnosis::Traceability {
                references: vec![],
                engine_version: None,
            },
            ai: None,
        };

        let input = adapter.build_input(&diagnosis, &[evidence]);

        assert_eq!(input.metadata.task_id, "test-task");
        assert!(input.system_prompt.contains("故障诊断"));
        assert!(input.user_prompt.contains("诊断任务"));
        assert!(!input.metadata.evidence_types.is_empty());
    }

    #[test]
    fn test_enhance_diagnosis() {
        let config = AiAdapterConfig::default();
        let adapter = AiAdapter::new(config);

        let ai_output = AiOutput {
            explanation: "AI 解释".to_string(),
            troubleshooting_steps: vec!["步骤1".to_string(), "步骤2".to_string()],
            root_cause_analysis: "根因".to_string(),
            ai_confidence: 0.8,
            suggested_metrics: vec![],
            suggested_commands: vec![],
        };

        let diagnosis = DiagnosisResult {
            schema_version: "diagnosis.v0.2".to_string(),
            task_id: "test-task".to_string(),
            status: crate::types::diagnosis::DiagnosisStatus::Done,
            runtime: None,
            trigger: crate::types::diagnosis::TriggerInfo {
                trigger_type: "manual".to_string(),
                trigger_reason: "test".to_string(),
                trigger_time_ms: 2000,
                matched_condition: None,
                event_type: None,
            },
            evidence_refs: vec![],
            conclusions: vec![Conclusion {
                conclusion_id: "con-1".to_string(),
                title: "I/O 延迟异常".to_string(),
                confidence: 0.85,
                evidence_strength: EvidenceStrength::High,
                severity: Some(8),
                details: None,
            }],
            recommendations: vec![],
            traceability: crate::types::diagnosis::Traceability {
                references: vec![],
                engine_version: None,
            },
            ai: None,
        };

        let enhanced = adapter.enhance_diagnosis(&diagnosis, &ai_output);

        // 验证 AI 建议被添加
        assert_eq!(enhanced.recommendations.len(), 2);
        assert_eq!(enhanced.recommendations[0].action, "步骤1");

        // 验证结论被增强
        let details = enhanced.conclusions[0].details.as_ref().unwrap();
        assert!(details.get("ai_enhancement").is_some());
    }
}
