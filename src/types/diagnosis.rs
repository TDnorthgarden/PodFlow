use serde::{Deserialize, Serialize};

/// Diagnosis Result Schema v0.2
/// 对应 docs/02_schemas.md 第 3 节
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResult {
    pub schema_version: String, // e.g., "diagnosis.v0.2"
    pub task_id: String,
    pub status: DiagnosisStatus,
    #[serde(default)]
    pub runtime: Option<RuntimeInfo>,
    pub trigger: TriggerInfo,
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub conclusions: Vec<Conclusion>,
    #[serde(default)]
    pub recommendations: Vec<Recommendation>,
    pub traceability: Traceability,
    #[serde(default)]
    pub ai: Option<AiInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosisStatus {
    Running,
    Done,
    Failed,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    #[serde(default)]
    pub started_time_ms: Option<i64>,
    #[serde(default)]
    pub finished_time_ms: Option<i64>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerInfo {
    pub trigger_type: String, // manual | condition | event
    pub trigger_reason: String,
    pub trigger_time_ms: i64,
    #[serde(default)]
    pub matched_condition: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>, // e.g., OOM
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub evidence_id: String,
    #[serde(default)]
    pub evidence_type: Option<String>,
    #[serde(default)]
    pub scope_key: Option<String>,
    #[serde(default)]
    pub role: Option<String>, // primary | support | context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conclusion {
    pub conclusion_id: String,
    pub title: String,
    pub confidence: f64, // 0~1
    pub evidence_strength: EvidenceStrength,
    #[serde(default)]
    pub severity: Option<u8>, // 0~10
    #[serde(default)]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStrength {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub priority: u32, // 越小越优先
    pub action: String,
    #[serde(default)]
    pub expected_impact: Option<String>,
    #[serde(default)]
    pub verification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Traceability {
    pub references: Vec<TraceabilityRef>,
    #[serde(default)]
    pub engine_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceabilityRef {
    pub conclusion_id: String,
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub reasoning_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiInfo {
    pub enabled: bool,
    pub status: AiStatus,
    #[serde(default)]
    pub summary: Option<String>,
    /// AI增强版本：v1=初始诊断, v2=AI增强后
    #[serde(default)]
    pub version: Option<String>,
    /// AI任务提交时间戳
    #[serde(default)]
    pub submitted_at_ms: Option<i64>,
    /// AI任务完成时间戳
    #[serde(default)]
    pub completed_at_ms: Option<i64>,
    /// AI处理耗时（毫秒）
    #[serde(default)]
    pub processing_duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiStatus {
    Ok,
    Timeout,
    Unavailable,
    Failed,
    SkippedInsufficientEvidence,
}
