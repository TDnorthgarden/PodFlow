use crate::types::diagnosis::DiagnosisResult;
use crate::types::evidence::Evidence;
use serde_json;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub mod alert_adapter;

/// 告警平台配置
#[derive(Debug, Clone)]
pub struct AlertPlatformConfig {
    /// 告警平台 API 端点
    pub endpoint: String,
    /// API 密钥/Token
    pub api_key: Option<String>,
    /// 请求超时（秒）
    pub timeout_secs: u64,
    /// 重试次数
    pub max_retries: u32,
    /// 重试间隔（毫秒）
    pub retry_interval_ms: u64,
}

impl Default for AlertPlatformConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8080/api/v1/alerts".to_string(),
            api_key: None,
            timeout_secs: 30,
            max_retries: 3,
            retry_interval_ms: 1000,
        }
    }
}

/// 结果发布器 - 负责输出结构化日志和告警推送
pub struct ResultPublisher {
    output_dir: String,
    /// 告警平台配置（可选）
    alert_config: Option<AlertPlatformConfig>,
}

impl ResultPublisher {
    pub fn new(output_dir: &str) -> Self {
        Self {
            output_dir: output_dir.to_string(),
            alert_config: None,
        }
    }

    /// 使用告警平台配置创建发布器
    pub fn with_alert_config(output_dir: &str, config: AlertPlatformConfig) -> Self {
        Self {
            output_dir: output_dir.to_string(),
            alert_config: Some(config),
        }
    }

    /// 设置告警平台配置
    pub fn set_alert_config(&mut self, config: AlertPlatformConfig) {
        self.alert_config = Some(config);
    }

    /// 发布诊断结果到本地结构化日志
    pub fn publish_diagnosis(&self, diagnosis: &DiagnosisResult) -> Result<(), PublishError> {
        // 确保输出目录存在
        std::fs::create_dir_all(&self.output_dir)?;

        // 生成文件名
        let filename = format!("{}/diagnosis_{}.json", self.output_dir, diagnosis.task_id);
        let path = Path::new(&filename);

        // 序列化为 JSON
        let json = serde_json::to_string_pretty(diagnosis)?;

        // 写入文件
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;

        tracing::info!("Diagnosis result published to {}", filename);
        Ok(())
    }

    /// 发布证据到本地结构化日志
    pub fn publish_evidence(&self, evidence: &Evidence) -> Result<(), PublishError> {
        // 确保输出目录存在
        std::fs::create_dir_all(&self.output_dir)?;

        // 生成文件名
        let filename = format!("{}/evidence_{}_{}.json", self.output_dir, evidence.task_id, evidence.evidence_type);
        let path = Path::new(&filename);

        // 序列化为 JSON
        let json = serde_json::to_string_pretty(evidence)?;

        // 写入文件
        let mut file = File::create(path)?;
        file.write_all(json.as_bytes())?;

        tracing::info!("Evidence published to {}", filename);
        Ok(())
    }

    /// 生成告警平台 payload（第 1 周 PoC - 仅打印到日志）
    pub fn generate_alert_payload(&self, diagnosis: &DiagnosisResult) -> AlertPayload {
        // 构建告警 payload
        let payload = AlertPayload {
            payload_version: "alert_payload.v0.1".to_string(),
            task_id: diagnosis.task_id.clone(),
            trigger_time_ms: diagnosis.trigger.trigger_time_ms,
            status: format!("{:?}", diagnosis.status),
            conclusions_summary: diagnosis.conclusions.iter().map(|c| {
                ConclusionSummary {
                    conclusion_id: c.conclusion_id.clone(),
                    title: c.title.clone(),
                    confidence: c.confidence,
                    evidence_strength: format!("{:?}", c.evidence_strength),
                }
            }).collect(),
            top_evidence_types: diagnosis.evidence_refs.iter()
                .filter_map(|e| e.evidence_type.clone())
                .collect(),
            dedup_key: format!("{}-{}", diagnosis.task_id, diagnosis.trigger.trigger_time_ms),
        };

        // 第 1 周 PoC：打印到日志
        tracing::info!("Alert payload generated: {:?}", payload);

        payload
    }

    /// 推送到告警平台
    /// 
    /// 支持 HTTP POST 推送，包含重试机制和幂等性保障
    pub async fn push_to_alert_platform(&self, payload: &AlertPayload) -> Result<(), PublishError> {
        let config = match &self.alert_config {
            Some(c) => c,
            None => {
                tracing::warn!("Alert platform config not set, skipping push");
                return Ok(());
            }
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| PublishError {
                code: "HTTP_CLIENT_ERROR".to_string(),
                message: e.to_string(),
            })?;

        let payload_json = serde_json::to_string(payload)
            .map_err(|e| PublishError {
                code: "SERIALIZATION_ERROR".to_string(),
                message: e.to_string(),
            })?;

        // 重试逻辑
        let mut last_error = None;
        for attempt in 0..config.max_retries {
            let mut request = client
                .post(&config.endpoint)
                .header("Content-Type", "application/json")
                .header("X-Dedup-Key", &payload.dedup_key);

            // 添加 API 密钥认证
            if let Some(api_key) = &config.api_key {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            match request.body(payload_json.clone()).send().await {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        tracing::info!(
                            "Alert pushed successfully to {} (task_id: {}, status: {})",
                            config.endpoint,
                            payload.task_id,
                            status
                        );
                        return Ok(());
                    } else {
                        let error_msg = format!("HTTP {}: {}", status, response.text().await.unwrap_or_default());
                        tracing::warn!("Alert push failed (attempt {}): {}", attempt + 1, error_msg);
                        last_error = Some(PublishError {
                            code: format!("HTTP_{}", status.as_u16()),
                            message: error_msg,
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!("Alert push request failed (attempt {}): {}", attempt + 1, e);
                    last_error = Some(PublishError {
                        code: "REQUEST_ERROR".to_string(),
                        message: e.to_string(),
                    });
                }
            }

            // 等待后重试
            if attempt < config.max_retries - 1 {
                tokio::time::sleep(Duration::from_millis(config.retry_interval_ms)).await;
            }
        }

        // 所有重试都失败
        Err(last_error.unwrap_or_else(|| PublishError {
            code: "MAX_RETRIES_EXCEEDED".to_string(),
            message: "Failed to push alert after max retries".to_string(),
        }))
    }

    /// 一键发布：本地文件 + 告警平台推送
    pub async fn publish_all(&self, diagnosis: &DiagnosisResult, evidences: &[Evidence]) -> Result<PublishResult, PublishError> {
        let mut result = PublishResult {
            local_files: Vec::new(),
            alert_pushed: false,
            alert_error: None,
        };

        // 1. 发布本地结构化日志
        for evidence in evidences {
            match self.publish_evidence(evidence) {
                Ok(()) => {
                    let filename = format!("{}/evidence_{}_{}.json", self.output_dir, evidence.task_id, evidence.evidence_type);
                    result.local_files.push(filename);
                }
                Err(e) => {
                    tracing::warn!("Failed to publish evidence: {:?}", e);
                }
            }
        }

        match self.publish_diagnosis(diagnosis) {
            Ok(()) => {
                let filename = format!("{}/diagnosis_{}.json", self.output_dir, diagnosis.task_id);
                result.local_files.push(filename);
            }
            Err(e) => {
                tracing::warn!("Failed to publish diagnosis: {:?}", e);
            }
        }

        // 2. 推送告警平台
        let payload = self.generate_alert_payload(diagnosis);
        match self.push_to_alert_platform(&payload).await {
            Ok(()) => {
                result.alert_pushed = true;
            }
            Err(e) => {
                result.alert_error = Some(e);
            }
        }

        Ok(result)
    }
}

/// 发布结果汇总
#[derive(Debug)]
pub struct PublishResult {
    /// 生成的本地文件路径列表
    pub local_files: Vec<String>,
    /// 是否成功推送到告警平台
    pub alert_pushed: bool,
    /// 告警推送错误（如有）
    pub alert_error: Option<PublishError>,
}

#[derive(Debug)]
pub struct PublishError {
    pub code: String,
    pub message: String,
}

impl From<std::io::Error> for PublishError {
    fn from(e: std::io::Error) -> Self {
        Self {
            code: "IO_ERROR".to_string(),
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for PublishError {
    fn from(e: serde_json::Error) -> Self {
        Self {
            code: "SERIALIZATION_ERROR".to_string(),
            message: e.to_string(),
        }
    }
}

/// 告警平台 payload 结构
#[derive(Debug, serde::Serialize)]
pub struct AlertPayload {
    pub payload_version: String,
    pub task_id: String,
    pub trigger_time_ms: i64,
    pub status: String,
    pub conclusions_summary: Vec<ConclusionSummary>,
    pub top_evidence_types: Vec<String>,
    pub dedup_key: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ConclusionSummary {
    pub conclusion_id: String,
    pub title: String,
    pub confidence: f64,
    pub evidence_strength: String,
}

impl Default for ResultPublisher {
    fn default() -> Self {
        Self::new("/var/log/nuts")
    }
}
