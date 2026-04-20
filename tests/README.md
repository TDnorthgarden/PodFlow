# 集成测试说明

## 测试结构

```
tests/
├── api_integration_test.rs    # API 端点集成测试
└── README.md                  # 本文件
```

## 测试覆盖

### 1. NRI Webhook 集成测试 (3 项通过 ✅)

- `test_nri_webhook_add_event` - 测试接收 NRI ADD 事件
- `test_nri_webhook_delete_event` - 测试接收 NRI DELETE 事件
- `test_nri_webhook_unknown_event` - 测试未知事件类型处理

### 2. 触发诊断集成测试 (2 项需环境 ⚠️)

- `test_trigger_endpoint` - 测试手动触发诊断（需要 bpftrace）
- `test_full_pipeline_nri_to_diagnosis` - 测试完整链路（需要 bpftrace）

## 运行测试

### 运行所有测试
```bash
cargo test
```

### 仅运行单元测试
```bash
cargo test --lib
```

### 仅运行 NRI Webhook 测试
```bash
cargo test test_nri_webhook
```

### 运行所有集成测试（需要 root + bpftrace）
```bash
sudo cargo test --test api_integration_test
```

## 环境要求

### NRI Webhook 测试
- 无需特殊权限
- 自动通过 ✅

### Trigger 集成测试
- 需要 root 权限（sudo）
- 需要 bpftrace 已安装
- 需要 bpftrace 脚本存在于 `scripts/bpftrace/` 目录

在当前容器/测试环境（无 bpftrace），Trigger 测试会失败，这是预期行为。

## 测试结果摘要

| 测试类型 | 通过 | 失败 | 说明 |
|---------|-----|------|------|
| 单元测试 | 13 | 0 | 核心逻辑完整 ✅ |
| NRI Webhook | 3 | 0 | 归属映射 API 完整 ✅ |
| Trigger | 0 | 2 | 需真实 bpftrace 环境 |
| **总计** | **16** | **2** | 核心功能可用 |

## CI/CD 建议

在 CI 环境中：
1. 运行 `cargo test --lib` 确保单元测试通过
2. NRI Webhook 测试自动通过，验证 API 契约
3. Trigger 集成测试需在专用测试环境（带 bpftrace）中运行
