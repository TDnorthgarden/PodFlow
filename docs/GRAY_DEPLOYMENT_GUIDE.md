# PodFlow 灰度部署指南

## 概述

灰度部署是在正式发布前的小规模验证阶段，通过在有限数量的节点上部署新版本，收集运行数据和用户反馈，降低发布风险。

## 部署前准备

### 1. 环境评估

#### 节点选择标准
- **集群规模**: 选择具有代表性的节点（不同配置、不同负载）
- **网络环境**: 测试网络隔离和延迟情况
- **存储类型**: 包含SSD和HDD的混合环境
- **应用类型**: 运行不同类型的工作负载

#### 评估清单
```bash
# 节点信息收集脚本
#!/bin/bash

NODES=(
    "node1-prod.example.com"
    "node2-staging.example.com" 
    "node3-test.example.com"
)

echo "=== 节点环境评估 ==="
for node in "${NODES[@]}"; do
    echo "检查节点: $node"
    
    # 系统信息
    ssh $node "uname -a"
    echo "内核版本: $(ssh $node 'uname -r')"
    
    # 资源情况
    ssh $node "free -h"
    echo "内存信息: $(ssh $node 'free -h' | grep 'Mem:')"
    
    # 网络配置
    ssh $node "ip addr show"
    echo "网络接口: $(ssh $node 'ip addr show' | grep 'inet ')"
    
    # 当前负载
    ssh $node "top -bn1 | head -20"
    echo "CPU负载: $(ssh $node 'top -bn1 | head -20')"
    
    echo "---"
done
```

### 2. 版本准备

#### 构建验证版本
```bash
# 构建灰度版本
git checkout -b release-v0.1.0-rc1
cargo build --release

# 验证构建结果
if [ $? -eq 0 ]; then
    echo "✅ 构建成功"
else
    echo "❌ 构建失败"
    exit 1
fi

# 创建版本标签
git tag v0.1.0-rc1

# 推送到测试仓库
git push origin v0.1.0-rc1
```

#### 镜像准备
```bash
# 创建灰度镜像
docker build -t podflow:v0.1.0-rc1 .

# 推送到镜像仓库
docker push your-registry.com/podflow:v0.1.0-rc1

# 验证镜像
docker run --rm podflow:v0.1.0-rc1 --version
```

### 3. 配置文件准备

#### 灰度配置模板
```yaml
# /etc/podflow/gray-deploy.yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: podflow-gray
  namespace: monitoring
  labels:
    app: podflow
    version: v0.1.0-rc1
    deployment-type: gray
spec:
  selector:
    matchLabels:
      name: podflow-gray
  template:
    metadata:
      labels:
        name: podflow-gray
        version: v0.1.0-rc1
        deployment-type: gray
    spec:
      serviceAccountName: podflow
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        privileged: true
      containers:
      - name: podflow
        image: your-registry.com/podflow:v0.1.0-rc1
        imagePullPolicy: IfNotPresent
        env:
        - name: PODFLOW_DEPLOYMENT
          value: "gray"
        - name: PODFLOW_LOG_LEVEL
          value: "info"
        - name: PODFLOW_GRAY_MODE
          value: "true"
        - name: PODFLOW_SERVER
          value: "http://podflow-api:8080"
        volumeMounts:
          - name: host-filesystem
            mountPath: /host
          - name: config-volume
            mountPath: /etc/podflow
          - name: log-volume
            mountPath: /var/log/podflow
      volumes:
      - name: config-volume
        configMap:
          name: podflow-gray-config
      - name: host-filesystem
          hostPath:
            path: /
            type: DirectoryOrCreate
      - name: log-volume
        hostPath:
            path: /var/log/podflow
            type: DirectoryOrCreate
      updateStrategy:
        type: RollingUpdate
        rollingUpdate:
          maxUnavailable: 1
          maxSurge: 1
```

## 部署流程

### 1. 蓝绿部署

#### 部署脚本
```bash
#!/bin/bash

set -e

# 配置变量
GRAY_NODES=("node1-prod.example.com" "node2-staging.example.com")
NAMESPACE="monitoring"
IMAGE_TAG="v0.1.0-rc1"
REGISTRY="your-registry.com"

echo "=== 开始灰度部署 ==="
echo "部署节点数量: ${#GRAY_NODES[@]}"
echo "镜像标签: $IMAGE_TAG"

# 部署到每个节点
for i in "${!GRAY_NODES[@]}"; do
    node=${GRAY_NODES[$i]}
    echo "部署到节点: $node"
    
    # 使用kubectl部署
    kubectl apply -f -n $NAMESPACE << EOF
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: podflow-gray
  namespace: $NAMESPACE
  labels:
    app: podflow
    version: $IMAGE_TAG
    deployment-type: gray
spec:
  selector:
    matchLabels:
      name: podflow-gray
  template:
    metadata:
      labels:
        name: podflow-gray
        version: $IMAGE_TAG
        deployment-type: gray
    spec:
      serviceAccountName: podflow
      securityContext:
        runAsUser: 1000
        runAsGroup: 1000
        privileged: true
      containers:
      - name: podflow
        image: $REGISTRY/podflow:$IMAGE_TAG
        imagePullPolicy: IfNotPresent
        env:
        - name: PODFLOW_DEPLOYMENT
          value: "gray"
        - name: PODFLOW_LOG_LEVEL
          value: "info"
        - name: PODFLOW_GRAY_MODE
          value: "true"
        - name: PODFLOW_SERVER
          value: "http://podflow-api:8080"
        volumeMounts:
          - name: host-filesystem
            mountPath: /host
          - name: config-volume
            mountPath: /etc/podflow
          - name: log-volume
            mountPath: /var/log/podflow
      volumes:
      - name: config-volume
        configMap:
          name: podflow-gray-config
      - name: host-filesystem
          hostPath:
            path: /
            type: DirectoryOrCreate
      - name: log-volume
        hostPath:
            path: /var/log/podflow
            type: DirectoryOrCreate
      updateStrategy:
        type: RollingUpdate
        rollingUpdate:
          maxUnavailable: 1
          maxSurge: 1
EOF

    # 验证部署
    if [ $? -eq 0 ]; then
        echo "✅ 节点 $node 部署成功"
    else
        echo "❌ 节点 $node 部署失败"
        DEPLOYMENT_FAILED=true
    fi
    
    # 等待部署完成
    sleep 10
done

# 检查部署结果
if [ "${DEPLOYMENT_FAILED}" = "true" ]; then
    echo "❌ 部署过程中存在失败"
    exit 1
fi

echo "=== 灰度部署完成 ==="
```

### 2. 监控配置

#### 监控脚本
```bash
#!/bin/bash

# 灰度监控脚本
monitor_gray_deployment() {
    local namespace="monitoring"
    
    echo "=== 灰度部署监控 ==="
    echo "开始时间: $(date)"
    
    # 检查部署状态
    kubectl get daemonset -n $namespace podflow-gray
    
    # 检查Pod状态
    echo "Pod状态:"
    kubectl get pods -n $namespace -l app=podflow-gray
    
    # 检查服务状态
    echo "服务状态:"
    kubectl get svc -n $namespace
    
    # 检查关键指标
    echo "关键指标:"
    for node in "${GRAY_NODES[@]}"; do
        echo "节点 $node 指标:"
        kubectl exec -n $namespace -l app=podflow-gray -- kubectl get pods -o jsonpath='{.items[0].status.containerStatuses[0].state}' | grep -E "Running|Pending|Failed" || echo "Unknown"
    done
    
    echo "当前时间: $(date)"
}

# 持续监控
while true; do
    monitor_gray_deployment
    sleep 30
done
```

## 监控指标

### 关键监控指标

#### 部署成功率
- **目标**: 100% 部署成功
- **监控周期**: 每5分钟检查一次

#### 性能指标
- **CPU使用率**: 正常 < 70%
- **内存使用率**: 正常 < 80%
- **网络延迟**: P99 < 100ms
- **错误率**: < 1%

#### 业务指标
- **诊断成功率**: > 95%
- **响应时间**: 平均 < 2s
- **用户反馈**: 收集并分析

### 数据收集

#### 日志收集
```bash
# 收集灰度日志
collect_gray_logs() {
    local start_time=$(date +%s)
    local log_file="/var/log/podflow/gray-deployment-${start_time}.log"
    
    for node in "${GRAY_NODES[@]}"; do
        echo "=== 收集节点 $node 日志 ===" >> $log_file
        kubectl logs -n monitoring -l app=podflow-gray --since=5m >> $log_file
        echo "节点 $node 日志收集完成" >> $log_file
    done
    
    echo "日志收集完成: $log_file"
}
```

#### 用户反馈收集
```bash
# 用户反馈收集脚本
collect_feedback() {
    local feedback_file="/var/log/podflow/gray-feedback-${start_time}.log"
    
    echo "=== 用户反馈收集 ===" >> $feedback_file
    echo "反馈渠道: 灰度监控群组" >> $feedback_file
    echo "反馈时间: $(date)" >> $feedback_file
    
    # 模拟用户反馈收集
    echo "收集性能指标..." >> $feedback_file
    echo "收集错误报告..." >> $feedback_file
    echo "收集用户体验反馈..." >> $feedback_file
    
    echo "反馈收集完成: $feedback_file"
}
```

## 回滚策略

### 1. 自动回滚条件

#### 回滚触发条件
- **错误率**: > 5%
- **性能下降**: > 20%
- **严重错误**: 系统崩溃或数据丢失

### 2. 回滚流程

#### 回滚脚本
```bash
#!/bin/bash

# 灰度回滚脚本
rollback_gray_deployment() {
    local namespace="monitoring"
    
    echo "=== 开始灰度回滚 ==="
    echo "回滚时间: $(date)"
    
    # 回滚到上一个稳定版本
    kubectl apply -f -n $NAMESPACE << EOF
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: podflow-gray
  namespace: $NAMESPACE
  labels:
    app: podflow
    version: v0.1.0-stable
    deployment-type: gray
spec:
      selector:
        matchLabels:
          name: podflow-gray
      template:
        metadata:
          labels:
            name: podflow-gray
            version: v0.1.0-stable
            deployment-type: gray
        spec:
          serviceAccountName: podflow
          securityContext:
            runAsUser: 1000
            runAsGroup: 1000
            privileged: true
          containers:
      - name: podflow
        image: your-registry.com/podflow:v0.1.0-stable
        imagePullPolicy: IfNotPresent
        env:
        - name: PODFLOW_DEPLOYMENT
          value: "gray"
        - name: PODFLOW_LOG_LEVEL
          value: "info"
        - name: PODFLOW_GRAY_MODE
          value: "true"
        - name: PODFLOW_ROLLBACK
          value: "true"
        volumeMounts:
          - name: host-filesystem
            mountPath: /host
          - name: config-volume
            mountPath: /etc/podflow
          - name: log-volume
            mountPath: /var/log/podflow
      volumes:
      - name: config-volume
        configMap:
          name: podflow-gray-config
      - name: host-filesystem
          hostPath:
            path: /
            type: DirectoryOrCreate
      - name: log-volume
        hostPath:
          path: /var/log/podflow
            type: DirectoryOrCreate
      updateStrategy:
        type: RollingUpdate
        rollingUpdate:
          maxUnavailable: 1
          maxSurge: 1
EOF

    # 验证回滚
    if [ $? -eq 0 ]; then
        echo "✅ 灰度回滚成功"
    else
        echo "❌ 灰度回滚失败"
        exit 1
    fi
    
    echo "=== 灰度回滚完成 ==="
}
```

## 验证清单

### 部署前检查
- [ ] 镜像构建成功
- [ ] 配置文件准备完成
- [ ] 节点环境评估完成
- [ ] 网络连通性测试通过

### 部署中检查
- [ ] Pod启动成功
- [ ] 健康检查通过
- [ ] 关键指标正常
- [ ] 错误日志无异常

### 部署后验证
- [ ] 功能测试通过
- [ ] 性能测试通过
- [ ] 用户反馈收集
- [ ] 监控指标稳定

## 风险控制

### 风险评估
```yaml
# 灰度部署风险评估
risk_assessment:
  # 技术风险
  technical_risks:
    - "新版本稳定性未知"
    - "配置兼容性问题"
    - "性能回归风险"
  
  # 业务风险
  business_risks:
    - "诊断准确性影响"
    - "用户体验下降"
    - "服务可用性风险"
  
  # 缓解措施
  mitigation_measures:
    - "蓝绿部署支持"
    - "快速回滚机制"
    - "监控告警强化"
    - "用户反馈渠道"
    - "技术支持团队待命"
```

## 时间计划

### 灰度部署时间线
```bash
# 时间计划
DAY1: 环境准备和评估
DAY2: 灰度部署开始
DAY3: 灰度监控和收集反馈
DAY4: 评估和决策
DAY5: 全量部署或回滚

# 每日任务
DAILY_TASKS=(
    "DAY1: 完成节点环境评估"
    "DAY2: 完成灰度部署配置准备"
    "DAY3: 开始灰度部署到2个节点"
    "DAY4: 监控灰度部署状态，收集性能数据"
    "DAY5: 分析用户反馈和系统指标"
)
```

## 总结

灰度部署是确保版本质量的关键环节，通过小规模验证可以及早发现和解决问题，降低全量部署的风险。
