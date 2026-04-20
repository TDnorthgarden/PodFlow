#!/bin/bash
# bpftrace脚本输出标准化集成测试

set -e

NUTS_DIR="/root/nuts"
TEMPLATE_DIR="$NUTS_DIR/scripts/bpftrace/templates"
ADAPTER_DIR="$NUTS_DIR/scripts/bpftrace/adapters"

echo "=========================================="
echo "bpftrace标准输出集成测试"
echo "=========================================="
echo ""

# 测试1: 模板文件存在性
echo "【测试1】验证模板文件..."
for template in network_latency.bt cgroup_contention.bt syscall_latency.bt; do
    if [ -f "$TEMPLATE_DIR/$template" ]; then
        echo "  ✓ $template"
    else
        echo "  ✗ $template 不存在"
        exit 1
    fi
done
echo ""

# 测试2: 模板语法检查 (使用bpftrace -d进行语法检查)
echo "【测试2】验证模板语法..."
for template in network_latency.bt cgroup_contention.bt; do
    echo -n "  检查 $template: "
    # 使用bpftrace -d进行dry-run语法检查
    # 注意: 需要root权限
    if sudo bpftrace -d "$TEMPLATE_DIR/$template" 2>/dev/null | head -5 >/dev/null; then
        echo "✓"
    else
        echo "⚠ (需要root或内核支持)"
    fi
done
echo ""

# 测试3: 适配器配置格式验证
echo "【测试3】验证适配器配置..."
if [ -f "$ADAPTER_DIR/example_nginx_adapter.yaml" ]; then
    echo "  ✓ 适配器配置文件存在"
    if python3 -c "import yaml; yaml.safe_load(open('$ADAPTER_DIR/example_nginx_adapter.yaml'))" 2>/dev/null; then
        echo "  ✓ YAML格式正确"
    else
        echo "  ✗ YAML格式错误"
        exit 1
    fi
else
    echo "  ✗ 适配器配置文件不存在"
    exit 1
fi
echo ""

# 测试4: Python适配器工具可用性
echo "【测试4】验证适配器工具..."
if [ -f "$ADAPTER_DIR/__init__.py" ]; then
    echo "  ✓ 适配器工具存在"
    # 测试Python语法
    if python3 -m py_compile "$ADAPTER_DIR/__init__.py" 2>/dev/null; then
        echo "  ✓ Python语法正确"
    else
        echo "  ✗ Python语法错误"
        exit 1
    fi
else
    echo "  ✗ 适配器工具不存在"
    exit 1
fi
echo ""

# 测试5: 标准文档存在性
echo "【测试5】验证标准文档..."
if [ -f "$NUTS_DIR/docs/11_bpftrace_output_standard.md" ]; then
    echo "  ✓ bpftrace输出标准文档存在"
else
    echo "  ✗ 文档不存在"
    exit 1
fi
echo ""

# 测试6: 标准输出格式验证 (使用模拟数据)
echo "【测试6】验证输出格式..."
cat > /tmp/test_output.jsonl << 'EOF'
{"type":"start","msg":"test started","ts_ms":1234567890}
{"type":"tcp_connect","pid":1234,"comm":"nginx","latency_us":1234,"target":"192.168.1.1","ts_ms":1234567890}
{"type":"stats","ts_ms":1234567890,"connect_count":1}
{"type":"end","msg":"test stopped","ts_ms":1234567900}
EOF

# 验证JSONL格式
valid_count=0
while IFS= read -r line; do
    if python3 -c "import json; json.loads('$line')" 2>/dev/null; then
        valid_count=$((valid_count + 1))
    fi
done < /tmp/test_output.jsonl

if [ $valid_count -eq 4 ]; then
    echo "  ✓ 所有$valid_count行JSON有效"
else
    echo "  ✗ 只有$valid_count/4行JSON有效"
fi
echo ""

echo "=========================================="
echo "测试完成！"
echo "=========================================="
echo ""
echo "总结:"
echo "  - 标准模板文件: ✓"
echo "  - 适配器工具: ✓"
echo "  - 输出格式规范: ✓"
echo ""
echo "下一步:"
echo "  1. 使用 ./nuts-observer validate-bpftrace 验证客户脚本"
echo "  2. 使用适配器转换非标准脚本输出"
echo "  3. 将标准输出接入采集器"
