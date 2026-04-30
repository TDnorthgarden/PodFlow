# M4 AI 适配与诊断解释设计

## 架构概览

```
┌─────────────────────────────────────────────────────────────────────────┐
│  AI 增强诊断流程                                                         │
│  ─────────────────────────────────────────────────────────────────────   │
│                                                                          │
│  Evidence + Diagnosis                                                   │
│      ↓                                                                  │
│  ┌─────────────────┐                                                    │
│  │ PromptBuilder   │ 构建结构化提示词                                   │
│  │   - 系统提示词   │                                                    │
│  │   - 证据摘要     │                                                    │
│  │   - 诊断上下文   │                                                    │
│  └────────┬────────┘                                                    │
│           ↓                                                             │
│  ┌─────────────────┐                                                    │
│  │ LLM Client      │ 调用大语言模型                                     │
│  │   - OpenAI       │                                                    │
│  │   - Claude       │                                                    │
│  │   - 本地模型     │                                                    │
│  └────────┬────────┘                                                    │
│           ↓                                                             │
│  ┌─────────────────┐                                                    │
│  │ ResponseParser  │ 解析 AI 输出                                       │
│  │   - 提取解释     │                                                    │
│  │   - 提取根因     │                                                    │
│  │   - 提取建议     │                                                    │
│  └────────┬────────┘                                                    │
│           ↓                                                             │
│  ┌─────────────────┐                                                    │
│  │ Diagnosis       │ 回填到诊断结果                                     │
│  │ Enhancement     │                                                    │
│  └─────────────────┘                                                    │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## 核心组件

### 1. 提示词构建器 (PromptBuilder)

**系统提示词模板**
```
你是一位专业的 Kubernetes 运维专家，擅长分析容器性能问题和故障诊断。
你的任务是根据提供的诊断证据，给出深入的技术分析和处理建议。

请按以下格式输出分析结果：
1. 问题解释（简明扼要）
2. 根因分析（技术细节）
3. 排查路径（具体步骤）
4. 处理建议（可操作建议）
5. AI置信度（0-1之间的数值）

约束：
- 基于提供的证据进行分析，不要假设不存在的信息
- 如果证据不足，明确指出需要补充哪些数据
- 给出具体的命令或配置参数，而不是泛泛而谈
```

**用户提示词模板**
```
## 诊断任务信息
- 任务ID: {task_id}
- 目标Pod: {pod_name}
- 分析时间窗口: {time_window}

## 采集的证据
{evidence_summary}

## 诊断引擎结论
{conclusions}

请基于以上信息进行分析...
```

### 2. LLM 客户端 (LlmClient)

支持多种 LLM 后端：
- **OpenAI** (GPT-4, GPT-3.5)
- **Anthropic Claude**
- **本地模型** (通过 Ollama/vLLM)
- **自定义端点** (兼容 OpenAI API)

### 3. 响应解析器 (ResponseParser)

解析 AI 输出，提取结构化数据：
```rust
pub struct ParsedAiResponse {
    pub explanation: String,
    pub root_cause: String,
    pub troubleshooting_steps: Vec<String>,
    pub recommendations: Vec<String>,
    pub ai_confidence: f64,
    pub raw_response: String,
}
```

### 4. 诊断增强器 (DiagnosisEnhancer)

将 AI 输出回填到诊断结果：
```rust
impl DiagnosisResult {
    pub fn enhance_with_ai(&mut self, ai_output: AiOutput) {
        // 添加 AI 解释
        self.ai_explanation = Some(ai_output.explanation);
        
        // 增强结论
        for conclusion in &mut self.conclusions {
            if let Some(root_cause) = &ai_output.root_cause_analysis {
                conclusion.details = Some(root_cause.clone());
            }
        }
        
        // 添加 AI 建议
        for step in ai_output.troubleshooting_steps {
            self.recommendations.push(Recommendation {
                title: step.clone(),
                description: step,
                source: RecommendationSource::Ai,
            });
        }
    }
}
```

## 异步处理架构

```
┌──────────────────────────────────────────────────────────────┐
│  同步诊断流程（快速响应）                                       │
│  ────────────────────────────────────────────────            │
│  采集 → 诊断引擎 → 返回基础结果（<1秒）                        │
│                    ↓                                           │
│              后台触发 AI 分析                                   │
└──────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────┐
│  异步 AI 增强流程（后台处理）                                   │
│  ────────────────────────────────────────────────              │
│  构建提示词 → 调用 LLM → 解析结果 → 更新诊断                   │
│       ↓                                                    │
│  通过 WebSocket/Callback 通知客户端结果更新                     │
└──────────────────────────────────────────────────────────────┘
```

## 降级策略

当 AI 服务不可用时：

1. **KeepOriginal** (默认)
   - 保留原始诊断结果
   - AI 分析失败不阻塞主流程
   - 记录 AI 失败日志

2. **ReduceConfidence**
   - 降低诊断置信度
   - 添加 "AI 分析失败" 标记

3. **MarkForReview**
   - 标记为需要人工审核
   - 不返回给调用方

## 配置示例

```yaml
# config/ai_adapter.yaml
ai_adapter:
  # LLM 配置
  llm:
    provider: "openai"  # openai, claude, local
    model: "gpt-4"
    endpoint: "https://api.openai.com/v1/chat/completions"
    api_key: "${OPENAI_API_KEY}"
    timeout_secs: 60
    max_retries: 2
    
  # 提示词配置
  prompt:
    system_prompt_template: "config/prompts/system.txt"
    user_prompt_template: "config/prompts/user.txt"
    max_evidence_length: 5000
    
  # 异步处理配置
  async:
    enabled: true
    worker_threads: 4
    queue_size: 100
    result_callback_url: "http://localhost:3000/v1/ai/callback"
    
  # 降级策略
  fallback_mode: "keep_original"  # keep_original, reduce_confidence, mark_for_review
```

## API 接口

### AI 分析请求
```http
POST /v1/ai/analyze
Content-Type: application/json

{
    "task_id": "task-123",
    "diagnosis_result": { ... },
    "evidences": [ ... ],
    "options": {
        "async": true,
        "priority": "high"
    }
}
```

### AI 分析响应（同步）
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
    "task_id": "task-123",
    "ai_enhanced": true,
    "ai_output": {
        "explanation": "检测到 CPU 资源争抢...",
        "root_cause_analysis": "多个容器同时运行计算密集型任务...",
        "troubleshooting_steps": [
            "1. 检查容器 CPU limit 设置",
            "2. 查看节点 CPU 使用率",
            "3. 分析容器调度策略"
        ],
        "ai_confidence": 0.92
    },
    "enhanced_diagnosis": { ... }
}
```

### AI 分析响应（异步）
```http
HTTP/1.1 202 Accepted
Content-Type: application/json

{
    "task_id": "task-123",
    "status": "processing",
    "callback_url": "/v1/ai/result/task-123"
}
```

## 性能指标

| 指标 | 目标值 | 说明 |
|------|--------|------|
| AI 调用延迟 | < 5s | 本地模型或缓存命中 |
| AI 调用延迟 | < 30s | 云端 API |
| 异步处理吞吐 | > 100 req/min | 队列处理速度 |
| 缓存命中率 | > 50% | 相似诊断复用结果 |

## 实现步骤

1. **Phase 1**: 基础 LLM 客户端
   - 实现 OpenAI 客户端
   - 实现提示词构建器
   - 基础响应解析

2. **Phase 2**: 诊断增强
   - 结果回填逻辑
   - 降级策略实现
   - 端到端测试

3. **Phase 3**: 异步处理
   - 后台任务队列
   - WebSocket/Callback 通知
   - 缓存机制

4. **Phase 4**: 优化
   - 多 LLM 后端支持
   - 提示词优化
   - 性能调优

## 验收标准

1. **功能完整**: 诊断结果可以通过 AI 增强
2. **降级可靠**: AI 失败不影响主诊断流程
3. **异步可用**: 支持后台处理和结果回调
4. **多后端**: 支持至少 2 种 LLM 后端
5. **性能达标**: 平均延迟 < 10s（本地模型）
