//! Nuts Adapters CLI - Python adapters 工具
//!
//! 此二进制文件专门用于处理 Python adapters，包括：
//! - adapt 命令：应用 adapter 配置转换数据
//! - _apply_parse 核心逻辑：解析和转换数据的核心功能

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::process::Command;

/// Nuts Adapters CLI - Python adapters 工具
#[derive(Parser)]
#[command(name = "nuts-adapters")]
#[command(about = "Nuts Adapters 工具 - 处理 Python adapters 和数据转换")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 应用 adapter 配置转换数据
    Adapt {
        /// Adapter 配置文件路径
        #[arg(short, long)]
        config: String,
        /// 输入数据文件路径
        #[arg(short, long)]
        input: String,
        /// 输出数据文件路径
        #[arg(short, long)]
        output: String,
        /// 输入格式 (json, yaml, csv)
        #[arg(short = 'f', long, default_value = "json")]
        input_format: String,
        /// 输出格式 (json, yaml, csv)
        #[arg(short = 't', long, default_value = "json")]
        output_format: String,
        /// 详细输出
        #[arg(short, long)]
        verbose: bool,
    },
    /// 验证 adapter 配置
    Validate {
        /// Adapter 配置文件路径
        #[arg(short, long)]
        config: String,
        /// 详细输出
        #[arg(short, long)]
        verbose: bool,
    },
    /// 列出可用的 adapter 模板
    ListTemplates {
        /// 过滤模板类型
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// 生成 adapter 配置模板
    GenerateTemplate {
        /// Adapter 类型
        #[arg(short, long)]
        adapter_type: String,
        /// 输出文件路径
        #[arg(short, long)]
        output: String,
        /// 模板参数
        #[arg(short, long)]
        params: Vec<String>,
    },
}

/// Adapter 配置结构
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdapterConfig {
    /// Adapter 名称
    pub name: String,
    /// Adapter 类型
    pub adapter_type: String,
    /// 描述
    pub description: Option<String>,
    /// 输入格式配置
    pub input_format: InputFormatConfig,
    /// 输出格式配置
    pub output_format: OutputFormatConfig,
    /// 字段映射配置
    pub field_mappings: HashMap<String, FieldMapping>,
    /// 过滤器配置
    pub filters: Option<Vec<FilterConfig>>,
    /// 聚合配置
    pub aggregations: Option<Vec<AggregationConfig>>,
    /// Python 脚本路径（用于自定义转换）
    pub python_script: Option<String>,
}

/// 输入格式配置
#[derive(Debug, Clone, Deserialize, Serialize)]
struct InputFormatConfig {
    /// 格式类型 (json, yaml, csv, custom)
    pub format: String,
    /// 分隔符 (CSV)
    pub delimiter: Option<String>,
    /// 是否有标题行 (CSV)
    pub header: Option<bool>,
    /// 自定义解析器
    pub custom_parser: Option<String>,
}

/// 输出格式配置
#[derive(Debug, Clone, Deserialize, Serialize)]
struct OutputFormatConfig {
    /// 格式类型 (json, yaml, csv, custom)
    pub format: String,
    /// 分隔符 (CSV)
    pub delimiter: Option<String>,
    /// 是否包含标题行 (CSV)
    pub header: Option<bool>,
    /// 自定义格式化器
    pub custom_formatter: Option<String>,
}

/// 字段映射配置
#[derive(Debug, Clone, Deserialize, Serialize)]
struct FieldMapping {
    /// 源字段名
    pub source_field: String,
    /// 目标字段名
    pub target_field: String,
    /// 数据类型转换
    pub type_conversion: Option<String>,
    /// 默认值
    pub default_value: Option<Value>,
    /// 是否必需
    pub required: Option<bool>,
    /// 验证规则
    pub validation: Option<ValidationRule>,
}

/// 验证规则
#[derive(Debug, Clone, Deserialize, Serialize)]
struct ValidationRule {
    /// 规则类型
    pub rule_type: String,
    /// 规则参数
    pub params: HashMap<String, Value>,
}

/// 过滤器配置
#[derive(Debug, Clone, Deserialize, Serialize)]
struct FilterConfig {
    /// 过滤器类型
    pub filter_type: String,
    /// 字段名
    pub field: String,
    /// 过滤条件
    pub condition: Value,
}

/// 聚合配置
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AggregationConfig {
    /// 聚合类型
    pub aggregation_type: String,
    /// 分组字段
    pub group_by: Vec<String>,
    /// 聚合字段
    pub aggregate_field: String,
    /// 目标字段名
    pub target_field: String,
}

/// Adapter 执行结果
#[derive(Debug, Clone)]
struct AdapterResult {
    /// 处理的记录数
    pub processed_records: usize,
    /// 成功的记录数
    pub successful_records: usize,
    /// 失败的记录数
    pub failed_records: usize,
    /// 错误信息
    pub errors: Vec<String>,
    /// 输出数据
    pub output_data: Value,
}

/// Python Adapter 核心逻辑
struct PythonAdapter {
    config: AdapterConfig,
}

impl PythonAdapter {
    /// 创建新的 Python Adapter
    fn new(config: AdapterConfig) -> Self {
        Self { config }
    }

    /// 应用解析和转换逻辑
    fn apply_parse(&self, input_data: Value) -> Result<AdapterResult, String> {
        let mut result = AdapterResult {
            processed_records: 0,
            successful_records: 0,
            failed_records: 0,
            errors: Vec::new(),
            output_data: Value::Array(Vec::new()),
        };

        // 根据输入格式解析数据
        let parsed_data = self.parse_input(&input_data)?;
        
        // 应用过滤器
        let filtered_data = self.apply_filters(&parsed_data)?;
        
        // 应用字段映射
        let mapped_data = self.apply_field_mappings(&filtered_data)?;
        
        // 应用聚合
        let aggregated_data = self.apply_aggregations(&mapped_data)?;
        
        // 应用 Python 脚本（如果有）
        let final_data = if let Some(python_script) = &self.config.python_script {
            self.apply_python_script(&aggregated_data, python_script)?
        } else {
            aggregated_data
        };

        result.output_data = final_data;
        result.successful_records = self.count_records(&result.output_data);
        result.processed_records = result.successful_records + result.failed_records;

        Ok(result)
    }

    /// 解析输入数据
    fn parse_input(&self, input_data: &Value) -> Result<Value, String> {
        match self.config.input_format.format.as_str() {
            "json" => Ok(input_data.clone()),
            "yaml" => {
                // 如果输入是字符串，尝试解析为 YAML
                match input_data.as_str() {
                    Some(yaml_str) => {
                        serde_yaml::from_str(yaml_str)
                            .map_err(|e| format!("YAML parsing error: {}", e))
                    }
                    None => Ok(input_data.clone()),
                }
            }
            "csv" => self.parse_csv(input_data),
            _ => Err(format!("Unsupported input format: {}", self.config.input_format.format)),
        }
    }

    /// 解析 CSV 数据
    fn parse_csv(&self, input_data: &Value) -> Result<Value, String> {
        match input_data.as_str() {
            Some(csv_str) => {
                let mut records = Vec::new();
                
                // 使用改进的CSV解析器，支持引号转义
                let parsed_lines = self.parse_csv_lines(csv_str)?;
                
                                
                if parsed_lines.is_empty() {
                    return Ok(Value::Array(records));
                }

                let _delimiter = self.config.input_format.delimiter.as_deref().unwrap_or(",");
                let has_header = self.config.input_format.header.unwrap_or(true);
                
                let headers: Vec<String> = if has_header {
                    parsed_lines[0].iter().map(|s| s.clone()).collect()
                } else {
                    (0..parsed_lines[0].len())
                        .map(|i| format!("column_{}", i))
                        .collect()
                };

                                let start_idx = if has_header { 1 } else { 0 };
                
                for line in parsed_lines.iter().skip(start_idx) {
                    let mut record = serde_json::Map::new();
                    
                    for (i, value) in line.iter().enumerate() {
                        if i < headers.len() {
                            // 尝试智能类型转换
                            let json_value = if let Ok(int_val) = value.parse::<i64>() {
                                Value::Number(int_val.into())
                            } else if let Ok(float_val) = value.parse::<f64>() {
                                Value::Number(serde_json::Number::from_f64(float_val).unwrap())
                            } else if let Ok(bool_val) = value.parse::<bool>() {
                                Value::Bool(bool_val)
                            } else {
                                Value::String(value.clone())
                            };
                            record.insert(headers[i].clone(), json_value);
                        }
                    }
                    
                                        records.push(Value::Object(record));
                }

                Ok(Value::Array(records))
            }
            None => Err("CSV input must be a string".to_string()),
        }
    }

    /// 改进的CSV行解析，支持引号转义
    fn parse_csv_lines(&self, csv_str: &str) -> Result<Vec<Vec<String>>, String> {
        let mut lines = Vec::new();
        let _delimiter = self.config.input_format.delimiter.as_deref().unwrap_or(",");
        
        for line in csv_str.lines() {
            if line.trim().is_empty() {
                continue;
            }
            
            let fields = self.parse_csv_line(line, _delimiter)?;
            lines.push(fields);
        }
        
        Ok(lines)
    }
    
    /// 解析单行CSV，支持引号转义
    fn parse_csv_line(&self, line: &str, _delimiter: &str) -> Result<Vec<String>, String> {
        let mut fields = Vec::new();
        let mut current_field = String::new();
        let mut in_quotes = false;
        let mut quote_char = '\0';
        let mut chars = line.chars().peekable();
        
        while let Some(ch) = chars.next() {
            if !in_quotes {
                match ch {
                    '"' | '\'' => {
                        in_quotes = true;
                        quote_char = ch;
                    }
                    ',' => {
                        fields.push(current_field.clone());
                        current_field.clear();
                    }
                    _ => {
                        current_field.push(ch);
                    }
                }
            } else {
                // 在引号内的处理
                if ch == quote_char {
                    // 检查是否是转义引号
                    if let Some(&next_ch) = chars.peek() {
                        if next_ch == quote_char {
                            // 转义引号，添加一个引号到字段中
                            current_field.push(quote_char);
                            chars.next(); // 跳过转义的引号
                        } else {
                            // 引号结束
                            in_quotes = false;
                            quote_char = '\0';
                        }
                    } else {
                        // 引号结束（行尾）
                        in_quotes = false;
                        quote_char = '\0';
                    }
                } else {
                    current_field.push(ch);
                }
            }
        }
        
        // 添加最后一个字段
        fields.push(current_field);
        
        Ok(fields)
    }

    /// 应用过滤器
    fn apply_filters(&self, data: &Value) -> Result<Value, String> {
        if self.config.filters.is_none() {
            return Ok(data.clone());
        }

        let filters = self.config.filters.as_ref().unwrap();
        let mut filtered_data = Vec::new();

        if let Value::Array(records) = data {
            for record in records {
                let mut passes_all_filters = true;
                
                for filter in filters {
                    if !self.apply_filter(record, filter)? {
                        passes_all_filters = false;
                        break;
                    }
                }
                
                if passes_all_filters {
                    filtered_data.push(record.clone());
                }
            }
        } else {
            // 单个记录
            let mut passes_all_filters = true;
            for filter in filters {
                if !self.apply_filter(data, filter)? {
                    passes_all_filters = false;
                    break;
                }
            }
            
            if passes_all_filters {
                filtered_data.push(data.clone());
            }
        }

        Ok(Value::Array(filtered_data))
    }

    /// 应用单个过滤器
    fn apply_filter(&self, record: &Value, filter: &FilterConfig) -> Result<bool, String> {
        let field_value = record.get(&filter.field)
            .unwrap_or(&Value::Null);

        match filter.filter_type.as_str() {
            "equals" => {
                Ok(field_value == &filter.condition)
            }
            "not_equals" => {
                Ok(field_value != &filter.condition)
            }
            "greater_than" => {
                if let (Some(num_val), Some(cond_val)) = (field_value.as_f64(), filter.condition.as_f64()) {
                    Ok(num_val > cond_val)
                } else {
                    Err("Invalid numeric comparison".to_string())
                }
            }
            "less_than" => {
                if let (Some(num_val), Some(cond_val)) = (field_value.as_f64(), filter.condition.as_f64()) {
                    Ok(num_val < cond_val)
                } else {
                    Err("Invalid numeric comparison".to_string())
                }
            }
            "contains" => {
                if let (Some(str_val), Some(cond_str)) = (field_value.as_str(), filter.condition.as_str()) {
                    Ok(str_val.contains(cond_str))
                } else {
                    Err("Invalid string comparison".to_string())
                }
            }
            "regex" => {
                if let (Some(str_val), Some(pattern)) = (field_value.as_str(), filter.condition.as_str()) {
                    let regex = regex::Regex::new(pattern)
                        .map_err(|e| format!("Invalid regex pattern: {}", e))?;
                    Ok(regex.is_match(str_val))
                } else {
                    Err("Invalid regex comparison".to_string())
                }
            }
            _ => Err(format!("Unsupported filter type: {}", filter.filter_type)),
        }
    }

    /// 应用字段映射
    fn apply_field_mappings(&self, data: &Value) -> Result<Value, String> {
        // 如果没有字段映射，直接返回原始数据
        if self.config.field_mappings.is_empty() {
            return Ok(data.clone());
        }

        let mut mapped_data = Vec::new();

        if let Value::Array(records) = data {
            for record in records {
                let mut mapped_record = serde_json::Map::new();
                
                for (_key, mapping) in &self.config.field_mappings {
                    let target_field = &mapping.target_field;
                    let source_value = record.get(&mapping.source_field);
                    
                    let value = if let Some(val) = source_value {
                        self.apply_type_conversion(val, &mapping.type_conversion)?
                    } else if let Some(default_val) = &mapping.default_value {
                        default_val.clone()
                    } else if mapping.required.unwrap_or(false) {
                        return Err(format!("Required field '{}' is missing", mapping.source_field));
                    } else {
                        Value::Null
                    };
                    
                    // 应用验证规则
                    let validated_value = if let Some(validation) = &mapping.validation {
                        match self.validate_field(&value, validation) {
                            Ok(()) => value,
                            Err(e) => {
                                // 验证失败时，对于非必需字段保留原始值，必需字段返回错误
                                if mapping.required.unwrap_or(false) {
                                    return Err(format!("Validation failed for field '{}': {}", mapping.source_field, e));
                                } else {
                                    value
                                }
                            }
                        }
                    } else {
                        value
                    };
                    
                    mapped_record.insert(target_field.clone(), validated_value);
                }
                
                mapped_data.push(Value::Object(mapped_record));
            }
        } else {
            let mut mapped_record = serde_json::Map::new();
            
            for (_key, mapping) in &self.config.field_mappings {
                let target_field = &mapping.target_field;
                let source_value = data.get(&mapping.source_field);
                
                let value = if let Some(val) = source_value {
                    self.apply_type_conversion(val, &mapping.type_conversion)?
                } else if let Some(default_val) = &mapping.default_value {
                    default_val.clone()
                } else if mapping.required.unwrap_or(false) {
                    return Err(format!("Required field '{}' is missing", mapping.source_field));
                } else {
                    Value::Null
                };
                
                if let Some(validation) = &mapping.validation {
                    self.validate_field(&value, validation)?;
                }
                
                mapped_record.insert(target_field.clone(), value);
            }
            
            mapped_data.push(Value::Object(mapped_record));
        }

        Ok(Value::Array(mapped_data))
    }

    /// 应用类型转换
    fn apply_type_conversion(&self, value: &Value, conversion: &Option<String>) -> Result<Value, String> {
        match conversion {
            None => Ok(value.clone()),
            Some(conv_type) => match conv_type.as_str() {
                "string" => {
                    match value {
                        Value::String(s) => Ok(Value::String(s.clone())),
                        _ => Ok(Value::String(value.to_string())),
                    }
                },
                "integer" => {
                    match value.as_i64() {
                        Some(i) => Ok(Value::Number(i.into())),
                        None => {
                            value.as_str().and_then(|s| s.parse::<i64>().ok())
                                .map(|i| Value::Number(i.into()))
                                .ok_or_else(|| format!("Cannot convert {} to integer", value))
                        }
                    }
                }
                "float" => {
                    match value.as_f64() {
                        Some(f) => Ok(Value::Number(serde_json::Number::from_f64(f).unwrap())),
                        None => {
                            value.as_str().and_then(|s| s.parse::<f64>().ok())
                                .and_then(|f| serde_json::Number::from_f64(f))
                                .map(Value::Number)
                                .ok_or_else(|| format!("Cannot convert {} to float", value))
                        }
                    }
                }
                "boolean" => {
                    match value.as_bool() {
                        Some(b) => Ok(Value::Bool(b)),
                        None => {
                            match value.as_str() {
                                Some("true" | "1" | "yes" | "on") => Ok(Value::Bool(true)),
                                Some("false" | "0" | "no" | "off") => Ok(Value::Bool(false)),
                                _ => Err(format!("Cannot convert {} to boolean", value)),
                            }
                        }
                    }
                }
                _ => Err(format!("Unsupported type conversion: {}", conv_type)),
            },
        }
    }

    /// 验证字段
    fn validate_field(&self, value: &Value, validation: &ValidationRule) -> Result<(), String> {
        match validation.rule_type.as_str() {
            "range" => {
                if let Some(num_val) = value.as_f64() {
                    let min = validation.params.get("min").and_then(|v| v.as_f64()).unwrap_or(f64::MIN);
                    let max = validation.params.get("max").and_then(|v| v.as_f64()).unwrap_or(f64::MAX);
                    if num_val < min || num_val > max {
                        return Err(format!("Value {} is outside range [{}, {}]", num_val, min, max));
                    }
                } else {
                    return Err("Range validation requires numeric value".to_string());
                }
            }
            "length" => {
                if let Some(str_val) = value.as_str() {
                    let min = validation.params.get("min").and_then(|v| v.as_u64()).unwrap_or(0);
                    let max = validation.params.get("max").and_then(|v| v.as_u64()).unwrap_or(u64::MAX);
                    let len = str_val.len() as u64;
                    if len < min || len > max {
                        return Err(format!("String length {} is outside range [{}, {}]", len, min, max));
                    }
                } else {
                    return Err("Length validation requires string value".to_string());
                }
            }
            "pattern" => {
                if let (Some(str_val), Some(pattern)) = (value.as_str(), validation.params.get("pattern").and_then(|v| v.as_str())) {
                    let regex = regex::Regex::new(pattern)
                        .map_err(|e| format!("Invalid regex pattern: {}", e))?;
                    if !regex.is_match(str_val) {
                        return Err(format!("Value '{}' does not match pattern '{}'", str_val, pattern));
                    }
                } else {
                    return Err("Pattern validation requires string value and pattern".to_string());
                }
            }
            _ => return Err(format!("Unsupported validation rule: {}", validation.rule_type)),
        }
        Ok(())
    }

    /// 应用聚合
    fn apply_aggregations(&self, data: &Value) -> Result<Value, String> {
        if self.config.aggregations.is_none() {
            return Ok(data.clone());
        }

        let aggregations = self.config.aggregations.as_ref().unwrap();
        let mut aggregated_data = Vec::new();

        // 简化的聚合实现
        if let Value::Array(records) = data {
            // 按分组字段分组
            let mut groups: HashMap<String, Vec<&Value>> = HashMap::new();
            
            for record in records {
                let mut group_key = String::new();
                for group_field in aggregations.iter().flat_map(|agg| agg.group_by.iter()) {
                    if let Some(value) = record.get(group_field) {
                        group_key.push_str(&value.to_string());
                        group_key.push('|');
                    }
                }
                
                groups.entry(group_key).or_default().push(record);
            }

            // 对每个组应用聚合
            for (group_key, group_records) in groups {
                let mut aggregated_record = serde_json::Map::new();
                
                // 添加分组字段值
                if !group_key.is_empty() {
                    let group_fields: Vec<&str> = group_key.split('|').collect();
                    for aggregation in aggregations {
                        for (i, group_field) in aggregation.group_by.iter().enumerate() {
                            if i < group_fields.len() && !group_fields[i].is_empty() {
                                if let Ok(parsed) = serde_json::from_str::<Value>(group_fields[i]) {
                                    aggregated_record.insert(group_field.clone(), parsed);
                                }
                            }
                        }
                    }
                }

                // 应用聚合函数
                for aggregation in aggregations {
                    let aggregated_value = self.apply_aggregation_function(&group_records, aggregation)?;
                    aggregated_record.insert(aggregation.target_field.clone(), aggregated_value);
                }
                
                aggregated_data.push(Value::Object(aggregated_record));
            }
        }

        Ok(Value::Array(aggregated_data))
    }

    /// 应用聚合函数
    fn apply_aggregation_function(&self, records: &[&Value], aggregation: &AggregationConfig) -> Result<Value, String> {
        let mut values = Vec::new();
        
        for record in records {
            if let Some(value) = record.get(&aggregation.aggregate_field) {
                if let Some(num_val) = value.as_f64() {
                    values.push(num_val);
                }
            }
        }

        if values.is_empty() {
            return Ok(Value::Null);
        }

        let result = match aggregation.aggregation_type.as_str() {
            "sum" => values.iter().sum(),
            "avg" => values.iter().sum::<f64>() / values.len() as f64,
            "min" => values.iter().fold(f64::MAX, |a, &b| a.min(b)),
            "max" => values.iter().fold(f64::MIN, |a, &b| a.max(b)),
            "count" => values.len() as f64,
            _ => return Err(format!("Unsupported aggregation type: {}", aggregation.aggregation_type)),
        };

        Ok(Value::Number(serde_json::Number::from_f64(result).unwrap()))
    }

    /// 应用 Python 脚本
    fn apply_python_script(&self, data: &Value, script_path: &str) -> Result<Value, String> {
        // 使用系统临时目录
        let temp_dir = std::env::temp_dir();
        let temp_input = temp_dir.join("nuts_adapter_input.json");
        let temp_output = temp_dir.join("nuts_adapter_output.json");
        
        fs::write(&temp_input, serde_json::to_string_pretty(data).unwrap())
            .map_err(|e| format!("Failed to write temp input: {}", e))?;

        // 执行 Python 脚本
        let output = Command::new("python3")
            .arg(script_path)
            .arg(&temp_input)
            .arg(&temp_output)
            .output()
            .map_err(|e| format!("Failed to execute Python script: {}", e))?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Python script failed: {}", error_msg));
        }

        // 读取输出
        let output_data = fs::read_to_string(&temp_output)
            .map_err(|e| format!("Failed to read temp output: {}", e))?;

        // 清理临时文件
        let _ = fs::remove_file(&temp_input);
        let _ = fs::remove_file(&temp_output);

        serde_json::from_str(&output_data)
            .map_err(|e| format!("Failed to parse Python script output: {}", e))
    }

    /// 计算记录数
    fn count_records(&self, data: &Value) -> usize {
        match data {
            Value::Array(records) => records.len(),
            Value::Object(_) => 1,
            _ => 0,
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Adapt { config, input, output, input_format, output_format, verbose } => {
            if verbose {
                println!("Applying adapter configuration...");
                println!("  Config: {}", config);
                println!("  Input: {}", input);
                println!("  Output: {}", output);
            }

            // 读取配置文件
            let config_content = fs::read_to_string(&config)?;
            let adapter_config: AdapterConfig = serde_json::from_str(&config_content)?;

            // 读取输入数据
            let input_data = match input_format.as_str() {
                "json" => {
                    let content = fs::read_to_string(&input)?;
                    serde_json::from_str(&content)?
                }
                "yaml" => {
                    let content = fs::read_to_string(&input)?;
                    serde_yaml::from_str(&content)?
                }
                "csv" => {
                    let content = fs::read_to_string(&input)?;
                    Value::String(content)
                }
                _ => return Err(format!("Unsupported input format: {}", input_format).into()),
            };

            // 创建 adapter 并应用转换
            let adapter = PythonAdapter::new(adapter_config);
            let result = adapter.apply_parse(input_data)?;

            if verbose {
                println!("Processing complete:");
                println!("  Processed records: {}", result.processed_records);
                println!("  Successful records: {}", result.successful_records);
                println!("  Failed records: {}", result.failed_records);
                if !result.errors.is_empty() {
                    println!("  Errors:");
                    for error in &result.errors {
                        println!("    - {}", error);
                    }
                }
            }

            // 写入输出
            let output_content = match output_format.as_str() {
                "json" => serde_json::to_string_pretty(&result.output_data)?,
                "yaml" => serde_yaml::to_string(&result.output_data)?,
                "csv" => {
                    // 简化的 CSV 输出
                    if let Value::Array(records) = &result.output_data {
                        let mut csv_content = String::new();
                        if !records.is_empty() {
                            if let Value::Object(first_record) = &records[0] {
                                let headers: Vec<String> = first_record.keys().cloned().collect();
                                csv_content.push_str(&headers.join(","));
                                csv_content.push('\n');
                                
                                for record in records {
                                    if let Value::Object(obj) = record {
                                        let values: Vec<String> = headers.iter()
                                            .map(|h| obj.get(h).map(|v| v.to_string()).unwrap_or_else(|| "".to_string()))
                                            .collect();
                                        csv_content.push_str(&values.join(","));
                                        csv_content.push('\n');
                                    }
                                }
                            }
                        }
                        csv_content
                    } else {
                        serde_json::to_string(&result.output_data)?
                    }
                }
                _ => return Err(format!("Unsupported output format: {}", output_format).into()),
            };

            fs::write(&output, output_content)?;
            println!("Adapter processing completed successfully!");
        }
        Commands::Validate { config, verbose } => {
            if verbose {
                println!("Validating adapter configuration...");
            }

            let config_content = fs::read_to_string(&config)?;
            let adapter_config: AdapterConfig = serde_json::from_str(&config_content)?;

            // 验证配置
            if adapter_config.name.is_empty() {
                return Err("Adapter name cannot be empty".into());
            }

            if adapter_config.adapter_type.is_empty() {
                return Err("Adapter type cannot be empty".into());
            }

            // 验证字段映射
            for (target_field, mapping) in &adapter_config.field_mappings {
                if target_field.is_empty() {
                    return Err("Target field cannot be empty".into());
                }
                if mapping.source_field.is_empty() {
                    return Err("Source field cannot be empty".into());
                }
            }

            println!("Adapter configuration is valid!");
        }
        Commands::ListTemplates { filter } => {
            println!("Available adapter templates:");
            
            let templates = vec![
                ("bpftrace", "BPFTrace output adapter"),
                ("nri", "NRI (Node Resource Interface) adapter"),
                ("prometheus", "Prometheus metrics adapter"),
                ("csv", "CSV data adapter"),
                ("json", "JSON data adapter"),
                ("custom", "Custom Python script adapter"),
            ];

            for (template_type, description) in templates {
                if let Some(filter_str) = &filter {
                    if !template_type.contains(filter_str) && !description.contains(filter_str) {
                        continue;
                    }
                }
                println!("  {} - {}", template_type, description);
            }
        }
        Commands::GenerateTemplate { adapter_type, output, params } => {
            let template = match adapter_type.as_str() {
                "bpftrace" => generate_bpftrace_template(&params),
                "nri" => generate_nri_template(&params),
                "prometheus" => generate_prometheus_template(&params),
                "csv" => generate_csv_template(&params),
                "json" => generate_json_template(&params),
                "custom" => generate_custom_template(&params),
                _ => return Err(format!("Unknown adapter type: {}", adapter_type).into()),
            };

            fs::write(&output, template)?;
            println!("Adapter template generated: {}", output);
        }
    }

    Ok(())
}

fn generate_bpftrace_template(_params: &[String]) -> String {
    r#"{
  "name": "bpftrace_adapter",
  "adapter_type": "bpftrace",
  "description": "BPFTrace output adapter for converting eBPF data to standard format",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "timestamp": {
      "source_field": "ts_ms",
      "target_field": "timestamp_ms",
      "type_conversion": "integer",
      "required": true
    },
    "pid": {
      "source_field": "pid",
      "target_field": "process_id",
      "type_conversion": "integer",
      "required": false
    },
    "comm": {
      "source_field": "comm",
      "target_field": "process_name",
      "type_conversion": "string",
      "required": false
    },
    "event_type": {
      "source_field": "type",
      "target_field": "event_type",
      "type_conversion": "string",
      "required": true
    }
  },
  "filters": [
    {
      "filter_type": "not_equals",
      "field": "type",
      "condition": "start"
    },
    {
      "filter_type": "not_equals",
      "field": "type",
      "condition": "end"
    }
  ]
}"#.to_string()
}

fn generate_nri_template(_params: &[String]) -> String {
    r#"{
  "name": "nri_adapter",
  "adapter_type": "nri",
  "description": "NRI (Node Resource Interface) adapter for container data",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "pod_uid": {
      "source_field": "pod_uid",
      "target_field": "pod_uid",
      "type_conversion": "string",
      "required": true
    },
    "pod_name": {
      "source_field": "pod_name",
      "target_field": "pod_name",
      "type_conversion": "string",
      "required": false
    },
    "namespace": {
      "source_field": "namespace",
      "target_field": "namespace",
      "type_conversion": "string",
      "required": false
    },
    "container_id": {
      "source_field": "container_id",
      "target_field": "container_id",
      "type_conversion": "string",
      "required": false
    }
  }
}"#.to_string()
}

fn generate_prometheus_template(_params: &[String]) -> String {
    r#"{
  "name": "prometheus_adapter",
  "adapter_type": "prometheus",
  "description": "Prometheus metrics adapter",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "metric_name": {
      "source_field": "__name__",
      "target_field": "metric_name",
      "type_conversion": "string",
      "required": true
    },
    "metric_value": {
      "source_field": "value",
      "target_field": "metric_value",
      "type_conversion": "float",
      "required": true
    },
    "timestamp": {
      "source_field": "timestamp",
      "target_field": "timestamp_ms",
      "type_conversion": "integer",
      "required": false
    }
  },
  "aggregations": [
    {
      "aggregation_type": "avg",
      "group_by": ["metric_name"],
      "aggregate_field": "metric_value",
      "target_field": "avg_value"
    }
  ]
}"#.to_string()
}

fn generate_csv_template(_params: &[String]) -> String {
    r#"{
  "name": "csv_adapter",
  "adapter_type": "csv",
  "description": "CSV data adapter",
  "input_format": {
    "format": "csv",
    "delimiter": ",",
    "header": true
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "id": {
      "source_field": "id",
      "target_field": "id",
      "type_conversion": "string",
      "required": true
    },
    "name": {
      "source_field": "name",
      "target_field": "name",
      "type_conversion": "string",
      "required": false
    },
    "value": {
      "source_field": "value",
      "target_field": "value",
      "type_conversion": "float",
      "required": false
    }
  }
}"#.to_string()
}

fn generate_json_template(_params: &[String]) -> String {
    r#"{
  "name": "json_adapter",
  "adapter_type": "json",
  "description": "JSON data adapter",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "id": {
      "source_field": "id",
      "target_field": "id",
      "type_conversion": "string",
      "required": true
    },
    "data": {
      "source_field": "data",
      "target_field": "payload",
      "type_conversion": "string",
      "required": false
    }
  }
}"#.to_string()
}

fn generate_custom_template(_params: &[String]) -> String {
    r#"{
  "name": "custom_adapter",
  "adapter_type": "custom",
  "description": "Custom Python script adapter",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "input_field": {
      "source_field": "input_field",
      "target_field": "output_field",
      "type_conversion": "string",
      "required": false
    }
  },
  "python_script": "/path/to/custom_transform.py"
}"#.to_string()
}
