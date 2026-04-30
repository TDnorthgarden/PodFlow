#!/usr/bin/env python3
"""
Nuts BPFtrace Adapter - 脚本输出标准化转换工具

提供功能:
1. 验证bpftrace脚本输出是否符合标准
2. 将非标准输出转换为标准Evidence格式
3. 客户脚本适配器配置管理
"""

import json
import re
import sys
import subprocess
import argparse
from typing import Dict, List, Optional, Any
from dataclasses import dataclass
from enum import Enum


class OutputFormat(Enum):
    """输出格式类型"""
    JSON = "json"
    LOG_LINE = "log_line"
    CUSTOM = "custom"


@dataclass
class ValidationResult:
    """验证结果"""
    valid: bool
    errors: List[str]
    warnings: List[str]
    event_count: int
    metrics: Dict[str, Any]


@dataclass
class AdapterConfig:
    """适配器配置"""
    adapter_id: str
    source_type: str
    source_pattern: str
    target_schema: Dict[str, str]
    evidence_type: str
    field_mappings: Dict[str, str]


class BpftraceValidator:
    """bpftrace输出验证器"""
    
    REQUIRED_FIELDS = {'type', 'pid', 'comm', 'ts_ms'}
    VALID_EVENT_TYPES = {
        'start', 'end', 'stats', 'error',
        'tcp_connect', 'tcp_retransmit', 'tcp_reset',
        'io_complete', 'io_timeout',
        'cpu_throttle', 'oom_victim', 'memory_reclaim',
        'syscall_exit', 'wait_syscall', 'fs_syscall'
    }
    
    def __init__(self):
        self.results = []
        self.errors = []
        self.warnings = []
    
    def validate_event(self, line: str, line_num: int) -> bool:
        """验证单行事件"""
        try:
            event = json.loads(line)
        except json.JSONDecodeError as e:
            self.errors.append(f"Line {line_num}: JSON解析错误 - {e}")
            return False
        
        # 检查必需字段
        missing = self.REQUIRED_FIELDS - set(event.keys())
        if missing:
            self.errors.append(f"Line {line_num}: 缺少必需字段 {missing}")
            return False
        
        # 检查事件类型
        if event['type'] not in self.VALID_EVENT_TYPES:
            self.warnings.append(
                f"Line {line_num}: 未知事件类型 '{event['type']}'"
            )
        
        # 检查数据类型
        if not isinstance(event.get('pid'), int):
            self.errors.append(f"Line {line_num}: pid必须是整数")
        if not isinstance(event.get('ts_ms'), int):
            self.errors.append(f"Line {line_num}: ts_ms必须是整数")
        
        return True
    
    def validate_stream(self, lines: List[str]) -> ValidationResult:
        """验证整个输出流"""
        valid_count = 0
        has_start = False
        has_end = False
        event_types = set()
        
        for i, line in enumerate(lines, 1):
            line = line.strip()
            if not line:
                continue
            
            if self.validate_event(line, i):
                valid_count += 1
                try:
                    event = json.loads(line)
                    event_types.add(event['type'])
                    if event['type'] == 'start':
                        has_start = True
                    if event['type'] == 'end':
                        has_end = True
                except:
                    pass
        
        # 检查整体结构
        if not has_start:
            self.warnings.append("缺少start标记事件")
        if not has_end:
            self.warnings.append("缺少end标记事件")
        
        return ValidationResult(
            valid=len(self.errors) == 0,
            errors=self.errors,
            warnings=self.warnings,
            event_count=valid_count,
            metrics={
                'event_types': list(event_types),
                'has_start': has_start,
                'has_end': has_end
            }
        )


class OutputAdapter:
    """输出适配器 - 转换非标准格式"""
    
    def __init__(self, config: AdapterConfig):
        self.config = config
        self.pattern = re.compile(config.source_pattern)
    
    def adapt_line(self, line: str) -> Optional[str]:
        """转换单行输出"""
        match = self.pattern.match(line)
        if not match:
            return None
        
        # 提取字段
        groups = match.groupdict()
        
        # 构建标准事件
        event = {
            'type': self.config.evidence_type,
            'pid': int(groups.get('pid', 0)),
            'comm': groups.get('comm', 'unknown'),
            'ts_ms': self._parse_timestamp(groups.get('ts', '0')),
        }
        
        # 添加额外字段映射
        for target_field, source_expr in self.config.field_mappings.items():
            if source_expr.startswith('${') and source_expr.endswith('}'):
                # 简单字段引用
                source_field = source_expr[2:-1]
                if source_field in groups:
                    event[target_field] = groups[source_field]
            elif source_expr.startswith('parse_'):
                # 解析函数
                event[target_field] = self._apply_parse(source_expr, groups)
        
        return json.dumps(event)
    
    def _parse_timestamp(self, ts_str: str) -> int:
        """解析时间戳"""
        try:
            return int(ts_str)
        except:
            return 0
    
    def _apply_parse(self, expr: str, groups: Dict) -> Any:
        """应用解析函数"""
        # 简化实现
        return expr


def validate_script(script_path: str, timeout: int = 5) -> ValidationResult:
    """验证bpftrace脚本输出"""
    print(f"验证脚本: {script_path}")
    print(f"超时设置: {timeout}秒")
    
    # 运行脚本并捕获输出
    try:
        result = subprocess.run(
            ['sudo', 'bpftrace', '-e', f'include("{script_path}");'],
            capture_output=True,
            text=True,
            timeout=timeout
        )
    except subprocess.TimeoutExpired:
        return ValidationResult(
            valid=False,
            errors=["脚本执行超时"],
            warnings=[],
            event_count=0,
            metrics={}
        )
    except Exception as e:
        return ValidationResult(
            valid=False,
            errors=[f"执行错误: {e}"],
            warnings=[],
            event_count=0,
            metrics={}
        )
    
    # 解析输出
    lines = result.stdout.strip().split('\n')
    validator = BpftraceValidator()
    return validator.validate_stream(lines)


def print_validation_result(result: ValidationResult):
    """打印验证结果"""
    print("\n" + "="*60)
    print("验证结果")
    print("="*60)
    
    if result.valid:
        print("✅ 输出格式有效")
    else:
        print("❌ 输出格式无效")
    
    print(f"\n有效事件数: {result.event_count}")
    print(f"检测到事件类型: {', '.join(result.metrics.get('event_types', []))}")
    
    if result.errors:
        print("\n❌ 错误:")
        for error in result.errors:
            print(f"  - {error}")
    
    if result.warnings:
        print("\n⚠️  警告:")
        for warning in result.warnings:
            print(f"  - {warning}")
    
    print("\n建议:")
    if not result.metrics.get('has_start'):
        print("  - 在BEGIN探针中添加start标记事件")
    if not result.metrics.get('has_end'):
        print("  - 在END探针中添加end标记事件")
    if result.event_count > 0 and not result.errors:
        print("  - 输出格式符合标准，可以直接接入nuts-observer")


def main():
    parser = argparse.ArgumentParser(
        description='Nuts BPFtrace 脚本输出验证与适配工具'
    )
    subparsers = parser.add_subparsers(dest='command', help='命令')
    
    # validate 命令
    validate_parser = subparsers.add_parser('validate', help='验证脚本输出')
    validate_parser.add_argument('--script', required=True, help='脚本路径')
    validate_parser.add_argument('--timeout', type=int, default=5, help='超时时间')
    
    # adapt 命令
    adapt_parser = subparsers.add_parser('adapt', help='转换非标准输出')
    adapt_parser.add_argument('--config', required=True, help='适配器配置')
    adapt_parser.add_argument('--input', help='输入文件(默认stdin)')
    
    # template 命令
    template_parser = subparsers.add_parser('template', help='生成脚本模板')
    template_parser.add_argument('--type', required=True, 
                                choices=['network', 'block_io', 'cgroup_contention', 
                                        'syscall_latency', 'fs_stall', 'oom_events'],
                                help='证据类型')
    template_parser.add_argument('--output', help='输出文件')
    
    args = parser.parse_args()
    
    if args.command == 'validate':
        result = validate_script(args.script, args.timeout)
        print_validation_result(result)
        sys.exit(0 if result.valid else 1)
    
    elif args.command == 'adapt':
        print("适配器功能待实现")
    
    elif args.command == 'template':
        # 复制模板
        import shutil
        template_path = f'/root/nuts/scripts/bpftrace/templates/{args.type}.bt'
        if args.output:
            shutil.copy(template_path, args.output)
            print(f"模板已生成: {args.output}")
        else:
            with open(template_path) as f:
                print(f.read())
    
    else:
        parser.print_help()


if __name__ == '__main__':
    main()
