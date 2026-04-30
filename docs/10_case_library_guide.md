# 案例库扩展指南

## 概述

nuts-observer案例库现已支持从YAML配置文件加载扩展案例，与witty-ops-cases对接。

## 当前状态

- **默认案例**: 4个（硬编码）
- **扩展案例**: 5个（YAML配置）
- **总计**: 9个案例

### 案例分类

| 分类 | 案例数 | 来源 |
|------|--------|------|
| CPU相关 | 2 | 默认+euler-cpu-steal |
| Memory相关 | 2 | 默认+euler-memory-leak |
| Network相关 | 2 | 默认+euler-conntrack |
| Storage相关 | 2 | 默认+euler-io-hang |
| System相关 | 1 | euler-kernel-hungtask |

## 从witty-ops-cases批量导入

### 步骤1: 克隆witty-ops-cases仓库

```bash
git clone https://gitcode.com/wenjunryou/witty-ops-cases.git /tmp/witty-cases
cd /tmp/witty-cases
```

### 步骤2: 分析案例结构

witty-ops-cases中的案例通常包含:
- 问题描述
- 现象/症状
- 根因分析
- 解决方案
- 相关指标

### 步骤3: 转换为nuts格式

创建转换脚本 `/tmp/convert_cases.py`:

```python
#!/usr/bin/env python3
"""
将witty-ops-cases转换为nuts-observer案例库格式
"""

import yaml
import json
import os
import re
from pathlib import Path

def extract_metrics_from_description(desc):
    """从描述中提取可能的指标"""
    metrics = []
    # 匹配"xxx超过yyy"模式
    patterns = [
        (r'(\w+)\s*超过\s*(\d+)%?\s*(\w*)', 'threshold'),
        (r'(\w+)\s*大于\s*(\d+)', 'threshold'),
        (r'(\w+)\s*高于\s*(\d+)', 'threshold'),
    ]
    for pattern, mtype in patterns:
        matches = re.findall(pattern, desc)
        for match in matches:
            if isinstance(match, tuple):
                metric_name = match[0]
                threshold = match[1]
                metrics.append({
                    'metric_name': metric_name,
                    'operator': '>',
                    'threshold': float(threshold),
                    'description': f"{metric_name}超过{threshold}"
                })
    return metrics

def convert_case(case_data, case_id):
    """转换单个案例"""
    nuts_case = {
        'case_id': case_id,
        'title': case_data.get('title', '未知案例'),
        'description': case_data.get('description', ''),
        'evidence_types': infer_evidence_types(case_data),
        'metric_patterns': extract_metrics_from_description(
            case_data.get('description', '') + 
            case_data.get('symptoms', '')
        ),
        'skills': generate_skills(case_data),
        'root_causes': [
            {
                'description': cause,
                'confidence': 0.7,
                'verification': '查看相关指标'
            }
            for cause in case_data.get('root_causes', [])
        ] or [{'description': '未知原因', 'confidence': 0.5, 'verification': '需进一步分析'}],
        'remediation': [
            {
                'step': i+1,
                'action': step,
                'expected_outcome': '问题解决',
                'risk': None
            }
            for i, step in enumerate(case_data.get('solutions', []))
        ] or [{'step': 1, 'action': '查看详细文档', 'expected_outcome': '找到解决方案', 'risk': None}],
        'references': case_data.get('links', []),
        'severity': case_data.get('severity', 7),
        'confidence': 0.75
    }
    return nuts_case

def infer_evidence_types(case_data):
    """根据内容推断证据类型"""
    desc = case_data.get('description', '').lower()
    types = set()
    
    if any(kw in desc for kw in ['cpu', 'throttle', 'steal', 'cfs_quota']):
        types.add('cgroup_contention')
    if any(kw in desc for kw in ['memory', 'oom', 'memory_pressure', 'rss']):
        types.add('cgroup_contention')
        types.add('oom_events')
    if any(kw in desc for kw in ['network', 'tcp', 'packet', 'latency', 'dns', 'conntrack']):
        types.add('network')
    if any(kw in desc for kw in ['io', 'disk', 'block', 'filesystem', 'd_state']):
        types.add('block_io')
        types.add('fs_stall')
    if any(kw in desc for kw in ['syscall', '系统调用']):
        types.add('syscall_latency')
    if any(kw in desc for kw in ['kernel', 'hung_task', 'softlock', 'hardlock']):
        types.add('cgroup_contention')
    
    return list(types) or ['cgroup_contention']

def generate_skills(case_data):
    """生成诊断技能"""
    skills = []
    desc = case_data.get('description', '').lower()
    
    # 根据内容生成技能
    if 'cpu' in desc:
        skills.append({
            'skill_id': f'check_{case_data.get("id", "case")}_cpu',
            'name': '检查CPU指标',
            'description': '分析CPU相关指标',
            'required_evidence': ['cgroup_contention'],
            'check_method': {'ThresholdCheck': {'metric': 'cpu_usage', 'operator': '>', 'threshold': 80.0}},
            'metrics': ['cpu_usage', 'cpu_throttle']
        })
    if 'memory' in desc or 'oom' in desc:
        skills.append({
            'skill_id': f'check_{case_data.get("id", "case")}_memory',
            'name': '检查内存指标',
            'description': '分析内存使用情况',
            'required_evidence': ['cgroup_contention'],
            'check_method': {'ThresholdCheck': {'metric': 'memory_usage', 'operator': '>', 'threshold': 80.0}},
            'metrics': ['memory_usage', 'memory_pressure']
        })
    if 'network' in desc:
        skills.append({
            'skill_id': f'check_{case_data.get("id", "case")}_network',
            'name': '检查网络指标',
            'description': '分析网络延迟和丢包',
            'required_evidence': ['network'],
            'check_method': {'ThresholdCheck': {'metric': 'latency_p99', 'operator': '>', 'threshold': 100.0}},
            'metrics': ['latency_p99', 'packet_loss']
        })
    
    return skills or [{
        'skill_id': f'analyze_{case_data.get("id", "case")}',
        'name': '案例诊断分析',
        'description': '基于案例特征进行诊断',
        'required_evidence': ['cgroup_contention'],
        'check_method': {'PatternMatch': {'pattern': case_data.get('id', 'unknown')}},
        'metrics': ['case_indicator']
    }]

def main():
    """主函数"""
    # 读取witty-ops-cases
    witty_cases_dir = '/tmp/witty-cases'
    output_file = '/root/nuts/cases/imported_cases.yaml'
    
    imported_cases = []
    
    # 遍历目录查找案例文件
    for root, dirs, files in os.walk(witty_cases_dir):
        for file in files:
            if file.endswith(('.md', '.yaml', '.json')):
                filepath = os.path.join(root, file)
                try:
                    with open(filepath, 'r', encoding='utf-8') as f:
                        content = f.read()
                    
                    # 尝试解析YAML/JSON
                    try:
                        case_data = yaml.safe_load(content)
                    except:
                        try:
                            case_data = json.loads(content)
                        except:
                            continue
                    
                    if not isinstance(case_data, dict):
                        continue
                    
                    # 生成案例ID
                    case_id = f"witty-{Path(file).stem}"
                    
                    # 转换案例
                    nuts_case = convert_case(case_data, case_id)
                    imported_cases.append(nuts_case)
                    print(f"✓ 导入: {nuts_case['title']}")
                    
                except Exception as e:
                    print(f"✗ 跳过 {file}: {e}")
    
    # 生成YAML输出
    if imported_cases:
        output = {'cases': imported_cases}
        with open(output_file, 'w', encoding='utf-8') as f:
            yaml.dump(output, f, allow_unicode=True, sort_keys=False)
        print(f"\n✅ 成功导入 {len(imported_cases)} 个案例到 {output_file}")
    else:
        print("\n⚠️ 未找到可导入的案例")

if __name__ == '__main__':
    main()
```

### 步骤4: 运行转换

```bash
python3 /tmp/convert_cases.py
```

### 步骤5: 验证并合并

```bash
# 查看生成的案例
cat /root/nuts/cases/imported_cases.yaml | head -50

# 合并到主案例文件（手动或使用脚本）
cat /root/nuts/cases/cases.yaml /root/nuts/cases/imported_cases.yaml > /root/nuts/cases/all_cases.yaml
```

### 步骤6: 重新加载案例库

```bash
./nuts-observer case stats
```

## 案例文件结构

```yaml
cases:
  - case_id: "唯一标识"
    title: "案例标题"
    description: "详细描述"
    evidence_types: ["相关证据类型"]
    metric_patterns:
      - metric_name: "指标名"
        operator: ">"
        threshold: 100.0
        description: "描述"
    skills:
      - skill_id: "技能ID"
        name: "技能名称"
        description: "技能描述"
        required_evidence: ["所需证据"]
        check_method: !ThresholdCheck  # 或 !PatternMatch, !TrendAnalysis, !CorrelationCheck
          metric: "指标名"
          operator: ">"
          threshold: 100.0
        metrics: ["相关指标列表"]
    root_causes:
      - description: "根因描述"
        confidence: 0.8
        verification: "验证方法"
    remediation:
      - step: 1
        action: "修复动作"
        expected_outcome: "预期结果"
        risk: "风险提示（可选）"
    references:
      - "参考链接"
    severity: 7  # 1-10
    confidence: 0.85  # 0.0-1.0
```

## 常用命令

```bash
# 查看案例统计
./nuts-observer case stats

# 列出所有案例
./nuts-observer case list

# 按证据类型过滤
./nuts-observer case list --evidence-type network

# 查看案例详情
./nuts-observer case show euler-io-hang-001

# 根据指标匹配案例
./nuts-observer case match --metrics "cpu_usage=85,memory_usage=90"

# 导出案例库
./nuts-observer case export --file backup_cases.yaml
```

## 扩展建议

### 从witty-ops-cases重点导入

1. **CPU类**: cpu-softlock, cpu-hardlock, cpu-frequency
2. **Memory类**: memory-fragmentation, memory-compaction
3. **Network类**: network-packet-loss, network-dns-delay
4. **Storage类**: storage-filesystem-corruption, storage-thin-pool-full
5. **System类**: kernel-rcu-stall, systemd-service-failed

### 质量验证清单

导入案例时验证:
- [ ] 案例ID唯一且不重复
- [ ] evidence_types与采集器对应
- [ ] metric_patterns有可用的指标
- [ ] 至少包含1个诊断skill
- [ ] 根因分析有验证方法
- [ ] 修复步骤可操作
- [ ] severity在1-10范围内

## 参考

- witty-ops-cases: https://gitcode.com/wenjunryou/witty-ops-cases
- nuts案例配置: `/root/nuts/cases/cases.yaml`
- 案例库源码: `/root/nuts/src/diagnosis/case_library.rs`
