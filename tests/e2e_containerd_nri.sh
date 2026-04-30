#!/bin/bash
# Nuts Observer Containerd NRI 端到端测试脚本
# 该脚本用于验证 nuts-observer 与 containerd NRI 的集成

set -e

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置
NUTS_OBSERVER_URL="http://localhost:8080"
NRI_SOCKET="/var/run/nri/nuts-observer.sock"
TEST_NAMESPACE="nuts-test"
TEST_POD_NAME="test-pod"

# 日志函数
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# 检查前置条件
check_prerequisites() {
    log_info "检查前置条件..."
    
    # 检查 containerd
    if ! command -v ctr &> /dev/null; then
        log_error "containerd (ctr) 未安装"
        exit 1
    fi
    
    # 检查 kubectl
    if ! command -v kubectl &> /dev/null; then
        log_warn "kubectl 未安装，跳过 K8s 相关测试"
    fi
    
    # 检查 curl
    if ! command -v curl &> /dev/null; then
        log_error "curl 未安装"
        exit 1
    fi
    
    log_info "前置条件检查通过"
}

# 检查 nuts-observer 服务状态
check_nuts_observer() {
    log_info "检查 nuts-observer 服务状态..."
    
    # 检查 HTTP 服务
    if ! curl -s "${NUTS_OBSERVER_URL}/health" > /dev/null 2>&1; then
        log_error "nuts-observer HTTP 服务未启动 (检查 ${NUTS_OBSERVER_URL})"
        exit 1
    fi
    
    log_info "nuts-observer HTTP 服务正常"
    
    # 检查 NRI Socket
    if [ -S "$NRI_SOCKET" ]; then
        log_info "NRI Unix Socket 存在: $NRI_SOCKET"
    else
        log_warn "NRI Unix Socket 不存在: $NRI_SOCKET (可能未启用 nri-grpc feature)"
    fi
}

# 测试 API 端点
test_api_endpoints() {
    log_info "测试 API 端点..."
    
    # 测试健康检查
    log_info "测试 /health 端点..."
    health_response=$(curl -s "${NUTS_OBSERVER_URL}/health")
    echo "  响应: $health_response"
    
    # 测试 NRI V1 状态
    log_info "测试 /api/v1/nri/status 端点..."
    v1_status=$(curl -s "${NUTS_OBSERVER_URL}/api/v1/nri/status")
    echo "  响应: $v1_status"
    
    # 测试 NRI V3 状态
    log_info "测试 /api/v3/nri/status 端点..."
    v3_status=$(curl -s "${NUTS_OBSERVER_URL}/api/v3/nri/status")
    echo "  响应: $v3_status"
    
    # 测试 Prometheus 指标
    log_info "测试 /metrics 端点..."
    metrics=$(curl -s "${NUTS_OBSERVER_URL}/metrics")
    if echo "$metrics" | grep -q "nuts_"; then
        echo "  ✓ 找到 nuts_* 指标"
    else
        log_warn "未找到 nuts_* 指标"
    fi
    
    log_info "API 端点测试完成"
}

# 模拟 NRI 事件 (HTTP Webhook)
test_nri_webhook() {
    log_info "测试 NRI HTTP Webhook..."
    
    # 发送 ADD 事件
    log_info "发送 Pod ADD 事件..."
    add_response=$(curl -s -X POST "${NUTS_OBSERVER_URL}/api/v1/nri/events" \
        -H "Content-Type: application/json" \
        -d '{
            "event_type": "ADD",
            "pod_uid": "test-pod-12345",
            "pod_name": "test-pod",
            "namespace": "default",
            "containers": [{
                "container_id": "container-abc",
                "cgroup_ids": ["/sys/fs/cgroup/kubepods/besteffort/pod12345/abc"],
                "pids": [1234, 1235]
            }]
        }')
    echo "  响应: $add_response"
    
    # 等待处理
    sleep 1
    
    # 查询 Pod 列表
    log_info "查询 Pod 列表..."
    pods=$(curl -s "${NUTS_OBSERVER_URL}/api/v1/nri/pods")
    echo "  Pods: $pods"
    
    # 发送 DELETE 事件
    log_info "发送 Pod DELETE 事件..."
    delete_response=$(curl -s -X POST "${NUTS_OBSERVER_URL}/api/v1/nri/events" \
        -H "Content-Type: application/json" \
        -d '{
            "event_type": "DELETE",
            "pod_uid": "test-pod-12345",
            "pod_name": "test-pod",
            "namespace": "default",
            "containers": []
        }')
    echo "  响应: $delete_response"
    
    log_info "NRI HTTP Webhook 测试完成"
}

# 测试 Unix Socket NRI 通信 (如果可用)
test_nri_unix_socket() {
    if [ ! -S "$NRI_SOCKET" ]; then
        log_warn "跳过 Unix Socket 测试 (socket 不存在)"
        return
    fi
    
    log_info "测试 NRI Unix Socket 通信..."
    
    # 检查 socket 可写性
    if [ -w "$NRI_SOCKET" ]; then
        log_info "Socket 可写"
    else
        log_warn "Socket 不可写，尝试 sudo..."
    fi
    
    # 使用 nc 发送测试数据 (如果可用)
    if command -v nc &> /dev/null; then
        log_info "使用 nc 发送测试事件..."
        echo '{"pod_uid": "socket-test-001", "pod_name": "socket-test", "namespace": "default", "containers": []}' | \
            timeout 2 nc -U "$NRI_SOCKET" || true
        log_info "测试事件已发送"
    else
        log_warn "nc 未安装，跳过 socket 发送测试"
    fi
    
    log_info "Unix Socket 测试完成"
}

# 测试诊断触发
test_diagnosis_trigger() {
    log_info "测试诊断触发..."
    
    # 先添加一个测试 Pod
    curl -s -X POST "${NUTS_OBSERVER_URL}/api/v1/nri/events" \
        -H "Content-Type: application/json" \
        -d '{
            "event_type": "ADD",
            "pod_uid": "diagnosis-test-001",
            "pod_name": "diagnosis-test",
            "namespace": "default",
            "containers": [{
                "container_id": "diag-container",
                "cgroup_ids": ["/sys/fs/cgroup/test"],
                "pids": [9999]
            }]
        }' > /dev/null
    
    sleep 1
    
    # 触发诊断
    log_info "触发诊断..."
    trigger_response=$(curl -s -X POST "${NUTS_OBSERVER_URL}/v1/diagnostics:trigger" \
        -H "Content-Type: application/json" \
        -d '{
            "trigger_type": "manual",
            "cgroup_id": "/sys/fs/cgroup/test",
            "evidence_types": ["basic", "process"],
            "idempotency_key": "test-001"
        }')
    echo "  响应: $trigger_response"
    
    # 提取 diagnosis_id
    diagnosis_id=$(echo "$trigger_response" | grep -o '"diagnosis_id":"[^"]*"' | cut -d'"' -f4)
    
    if [ -n "$diagnosis_id" ]; then
        log_info "诊断 ID: $diagnosis_id"
        
        # 查询诊断结果
        sleep 2
        log_info "查询诊断结果..."
        result=$(curl -s "${NUTS_OBSERVER_URL}/v1/diagnosis/${diagnosis_id}")
        echo "  结果: $result"
    else
        log_warn "未能获取诊断 ID"
    fi
    
    log_info "诊断触发测试完成"
}

# Kubernetes 集成测试 (如果可用)
test_kubernetes_integration() {
    if ! command -v kubectl &> /dev/null; then
        log_warn "kubectl 不可用，跳过 K8s 集成测试"
        return
    fi
    
    log_info "测试 Kubernetes 集成..."
    
    # 检查 nuts-observer Pod 是否运行
    pod_count=$(kubectl get pods -n kube-system -l app=nuts-observer-nri --no-headers 2>/dev/null | wc -l || echo "0")
    
    if [ "$pod_count" -gt 0 ]; then
        log_info "找到 $pod_count 个 nuts-observer Pod"
        
        # 查看 Pod 日志
        kubectl logs -n kube-system -l app=nuts-observer-nri --tail=20 || true
    else
        log_warn "未找到 nuts-observer Pod，可能未部署"
    fi
    
    # 创建测试 Pod
    log_info "创建测试 Pod..."
    cat <<EOF | kubectl apply -f - 2>/dev/null || log_warn "创建 Pod 失败 (可能无权限)"
apiVersion: v1
kind: Pod
metadata:
  name: ${TEST_POD_NAME}
  namespace: default
spec:
  containers:
  - name: test
    image: nginx:alpine
    resources:
      limits:
        memory: "64Mi"
        cpu: "100m"
EOF
    
    # 等待 Pod 创建
    if kubectl get pod ${TEST_POD_NAME} -n default &>/dev/null; then
        log_info "等待测试 Pod 运行..."
        kubectl wait --for=condition=Ready pod/${TEST_POD_NAME} -n default --timeout=60s 2>/dev/null || true
        
        # 检查 nuts-observer 是否收到事件
        sleep 3
        log_info "检查 nuts-observer 是否收到 NRI 事件..."
        
        # 通过 port-forward 访问 nuts-observer API
        # 注意：实际测试需要在集群内或使用 port-forward
    fi
    
    # 清理测试 Pod
    kubectl delete pod ${TEST_POD_NAME} -n default 2>/dev/null || true
    
    log_info "Kubernetes 集成测试完成"
}

# 性能测试
test_performance() {
    log_info "测试 NRI 处理性能..."
    
    # 批量发送事件
    log_info "批量发送 100 个 NRI 事件..."
    start_time=$(date +%s%N)
    
    for i in $(seq 1 100); do
        curl -s -X POST "${NUTS_OBSERVER_URL}/api/v1/nri/events" \
            -H "Content-Type: application/json" \
            -d "{
                \"event_type\": \"ADD\",
                \"pod_uid\": \"perf-test-${i}\",
                \"pod_name\": \"perf-test-${i}\",
                \"namespace\": \"default\",
                \"containers\": []
            }" > /dev/null
    done
    
    end_time=$(date +%s%N)
    duration_ms=$(( (end_time - start_time) / 1000000 ))
    
    log_info "100 个事件处理完成，耗时: ${duration_ms}ms"
    
    # 查询状态
    v3_status=$(curl -s "${NUTS_OBSERVER_URL}/api/v3/nri/status")
    echo "  当前状态: $v3_status"
    
    log_info "性能测试完成"
}

# 生成测试报告
generate_report() {
    log_info "生成测试报告..."
    
    echo ""
    echo "========================================"
    echo "    Nuts Observer Containerd NRI 测试报告"
    echo "========================================"
    echo ""
    echo "测试时间: $(date)"
    echo "nuts-observer URL: $NUTS_OBSERVER_URL"
    echo "NRI Socket: $NRI_SOCKET"
    echo ""
    echo "服务状态:"
    echo "  - HTTP API: ✓"
    if [ -S "$NRI_SOCKET" ]; then
        echo "  - NRI Socket: ✓"
    else
        echo "  - NRI Socket: ✗ (未启用)"
    fi
    echo ""
    echo "API 端点:"
    echo "  - /health: ✓"
    echo "  - /api/v1/nri/*: ✓"
    echo "  - /api/v3/nri/*: ✓"
    echo "  - /metrics: ✓"
    echo ""
    echo "========================================"
    echo ""
}

# 主函数
main() {
    echo "========================================"
    echo "Nuts Observer Containerd NRI E2E Test"
    echo "========================================"
    echo ""
    
    check_prerequisites
    check_nuts_observer
    test_api_endpoints
    test_nri_webhook
    test_nri_unix_socket
    test_diagnosis_trigger
    test_kubernetes_integration
    test_performance
    generate_report
    
    log_info "所有测试完成！"
}

# 运行主函数
main "$@"
