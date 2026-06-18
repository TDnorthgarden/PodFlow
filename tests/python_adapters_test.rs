//! Python adapters 工具测试
//!
//! 测试 podflow-adapters CLI 工具的功能：
//! 1. adapt 命令功能
//! 2. _apply_parse 核心逻辑
//! 3. 配置验证功能
//! 4. 模板生成功能

use std::fs;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

/// 测试 Python adapters 工具的基本功能
#[tokio::test]
async fn test_python_adapters_basic_functionality() {
    println!("🔍 测试 Python adapters 工具基本功能...");
    
    // 测试帮助信息
    let help_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "--help"])
        .output()
        .expect("Failed to run podflow-adapters --help");
    
    assert!(help_output.status.success());
    let help_text = String::from_utf8_lossy(&help_output.stdout);
    assert!(help_text.contains("PodFlow Adapters 工具"));
    assert!(help_text.contains("adapt"));
    assert!(help_text.contains("validate"));
    assert!(help_text.contains("list-templates"));
    assert!(help_text.contains("generate-template"));
    
    println!("✅ 帮助信息测试通过");
}

/// 测试配置验证功能
#[tokio::test]
async fn test_adapter_config_validation() {
    println!("🔍 测试配置验证功能...");
    
    // 创建测试配置文件
    let test_config = r#"
{
  "name": "test_adapter",
  "adapter_type": "test",
  "description": "Test adapter for validation",
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
    "name": {
      "source_field": "name",
      "target_field": "name",
      "type_conversion": "string",
      "required": false
    }
  }
}"#;
    
    let config_path = "/tmp/test_adapter_config.json";
    fs::write(config_path, test_config).expect("Failed to write test config");
    
    // 验证配置
    let validate_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "validate", "--config", config_path])
        .output()
        .expect("Failed to validate adapter config");
    
    assert!(validate_output.status.success());
    let validate_text = String::from_utf8_lossy(&validate_output.stdout);
    assert!(validate_text.contains("Adapter configuration is valid"));
    
    // 清理
    let _ = fs::remove_file(config_path);
    
    println!("✅ 配置验证测试通过");
}

/// 测试模板列表功能
#[tokio::test]
async fn test_template_list_functionality() {
    println!("🔍 测试模板列表功能...");
    
    // 列出所有模板
    let list_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "list-templates"])
        .output()
        .expect("Failed to list templates");
    
    assert!(list_output.status.success());
    let list_text = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_text.contains("bpftrace"));
    assert!(list_text.contains("nri"));
    assert!(list_text.contains("prometheus"));
    assert!(list_text.contains("csv"));
    assert!(list_text.contains("json"));
    assert!(list_text.contains("custom"));
    
    // 测试过滤功能
    let filter_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "list-templates", "--filter", "bpftrace"])
        .output()
        .expect("Failed to list filtered templates");
    
    assert!(filter_output.status.success());
    let filter_text = String::from_utf8_lossy(&filter_output.stdout);
    assert!(filter_text.contains("bpftrace"));
    
    println!("✅ 模板列表测试通过");
}

/// 测试模板生成功能
#[tokio::test]
async fn test_template_generation() {
    println!("🔍 测试模板生成功能...");
    
    let template_path = "/tmp/test_bpftrace_template.json";
    
    // 生成 BPFTrace 模板
    let generate_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "generate-template", 
                "--adapter-type", "bpftrace", "--output", template_path])
        .output()
        .expect("Failed to generate template");
    
    assert!(generate_output.status.success());
    
    // 验证生成的模板
    let template_content = fs::read_to_string(template_path)
        .expect("Failed to read generated template");
    
    assert!(template_content.contains("bpftrace_adapter"));
    assert!(template_content.contains("field_mappings"));
    assert!(template_content.contains("filters"));
    
    // 清理
    let _ = fs::remove_file(template_path);
    
    println!("✅ 模板生成测试通过");
}

/// 测试 adapt 命令功能
#[tokio::test]
async fn test_adapt_command_functionality() {
    println!("🔍 测试 adapt 命令功能...");
    
    // 创建测试配置
    let test_config = r#"
{
  "name": "json_test_adapter",
  "adapter_type": "json",
  "description": "Test JSON adapter",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "user_id": {
      "source_field": "id",
      "target_field": "user_id",
      "type_conversion": "integer",
      "required": true
    },
    "user_name": {
      "source_field": "name",
      "target_field": "user_name",
      "type_conversion": "string",
      "required": false
    },
    "user_age": {
      "source_field": "age",
      "target_field": "user_age",
      "type_conversion": "integer",
      "required": false
    }
  },
  "filters": [
    {
      "filter_type": "greater_than",
      "field": "age",
      "condition": 18
    }
  ]
}"#;
    
    // 创建测试输入数据
    let test_input = r#"
[
  {
    "id": "1",
    "name": "Alice",
    "age": 25
  },
  {
    "id": "2",
    "name": "Bob",
    "age": 17
  },
  {
    "id": "3",
    "name": "Charlie",
    "age": 30
  }
]"#;
    
    let config_path = "/tmp/test_adapt_config.json";
    let input_path = "/tmp/test_adapt_input.json";
    let output_path = "/tmp/test_adapt_output.json";
    
    // 写入测试文件
    fs::write(config_path, test_config).expect("Failed to write config");
    fs::write(input_path, test_input).expect("Failed to write input");
    
    // 执行 adapt 命令
    let adapt_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "adapt",
                "--config", config_path,
                "--input", input_path,
                "--output", output_path,
                "--verbose"])
        .output()
        .expect("Failed to run adapt command");
    
    assert!(adapt_output.status.success());
    
    // 验证输出
    let output_content = fs::read_to_string(output_path)
        .expect("Failed to read output");
    
    // 解析输出 JSON
    let output_json: serde_json::Value = serde_json::from_str(&output_content)
        .expect("Failed to parse output JSON");
    
    if let serde_json::Value::Array(records) = output_json {
        // 应该只有两条记录（Bob 被过滤器过滤掉了）
        assert_eq!(records.len(), 2);
        
        // 验证字段映射
        for record in &records {
            assert!(record.get("user_id").is_some());
            assert!(record.get("user_name").is_some());
            assert!(record.get("user_age").is_some());
        }
    } else {
        panic!("Expected array in output");
    }
    
    // 清理
    let _ = fs::remove_file(config_path);
    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_path);
    
    println!("✅ adapt 命令测试通过");
}

/// 测试 CSV 输入格式
#[tokio::test]
async fn test_csv_input_format() {
    println!("🔍 测试 CSV 输入格式...");
    
    // 创建 CSV 配置
    let csv_config = r#"
{
  "name": "csv_test_adapter",
  "adapter_type": "csv",
  "description": "Test CSV adapter",
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
}"#;
    
    // 创建 CSV 输入数据
    let csv_input = "id,name,value\n1,alice,10.5\n2,bob,20.3\n3,charlie,15.7";
    
    let config_path = "/tmp/test_csv_config.json";
    let input_path = "/tmp/test_csv_input.csv";
    let output_path = "/tmp/test_csv_output.json";
    
    // 写入测试文件
    fs::write(config_path, csv_config).expect("Failed to write config");
    fs::write(input_path, csv_input).expect("Failed to write input");
    
    // 执行 adapt 命令
    let adapt_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "adapt",
                "--config", config_path,
                "--input", input_path,
                "--output", output_path,
                "--input-format", "csv"])
        .output()
        .expect("Failed to run adapt command");
    
    assert!(adapt_output.status.success());
    
    // 验证输出
    let output_content = fs::read_to_string(output_path)
        .expect("Failed to read output");
    
    // 解析输出 JSON
    let output_json: serde_json::Value = serde_json::from_str(&output_content)
        .expect("Failed to parse output JSON");
    
    if let serde_json::Value::Array(records) = output_json {
        assert_eq!(records.len(), 3);
        
        // 验证第一条记录
        if let serde_json::Value::Object(first_record) = &records[0] {
            assert_eq!(first_record.get("id").unwrap().as_str().unwrap(), "1");
            assert_eq!(first_record.get("name").unwrap().as_str().unwrap(), "alice");
            assert!((first_record.get("value").unwrap().as_f64().unwrap() - 10.5).abs() < 0.001);
        } else {
            panic!("Expected object in first record");
        }
    } else {
        panic!("Expected array in output");
    }
    
    // 清理
    let _ = fs::remove_file(config_path);
    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_path);
    
    println!("✅ CSV 输入格式测试通过");
}

/// 测试聚合功能
#[tokio::test]
async fn test_aggregation_functionality() {
    println!("🔍 测试聚合功能...");
    
    // 创建带聚合的配置
    let aggregation_config = r#"
{
  "name": "aggregation_test_adapter",
  "adapter_type": "json",
  "description": "Test aggregation adapter",
  "input_format": {
    "format": "json"
  },
  "output_format": {
    "format": "json"
  },
  "field_mappings": {
    "category": {
      "source_field": "category",
      "target_field": "category",
      "type_conversion": "string",
      "required": true
    },
    "value": {
      "source_field": "value",
      "target_field": "value",
      "type_conversion": "float",
      "required": true
    }
  },
  "aggregations": [
    {
      "aggregation_type": "avg",
      "group_by": ["category"],
      "aggregate_field": "value",
      "target_field": "avg_value"
    },
    {
      "aggregation_type": "count",
      "group_by": ["category"],
      "aggregate_field": "value",
      "target_field": "count"
    }
  ]
}"#;
    
    // 创建测试输入数据
    let test_input = r#"
[
  {
    "category": "A",
    "value": 10.0
  },
  {
    "category": "A",
    "value": 20.0
  },
  {
    "category": "B",
    "value": 15.0
  },
  {
    "category": "B",
    "value": 25.0
  }
]"#;
    
    let config_path = "/tmp/test_aggregation_config.json";
    let input_path = "/tmp/test_aggregation_input.json";
    let output_path = "/tmp/test_aggregation_output.json";
    
    // 写入测试文件
    fs::write(config_path, aggregation_config).expect("Failed to write config");
    fs::write(input_path, test_input).expect("Failed to write input");
    
    // 执行 adapt 命令
    let adapt_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "adapt",
                "--config", config_path,
                "--input", input_path,
                "--output", output_path])
        .output()
        .expect("Failed to run adapt command");
    
    assert!(adapt_output.status.success());
    
    // 验证输出
    let output_content = fs::read_to_string(output_path)
        .expect("Failed to read output");
    
    // 解析输出 JSON
    let output_json: serde_json::Value = serde_json::from_str(&output_content)
        .expect("Failed to parse output JSON");
    
    if let serde_json::Value::Array(records) = output_json {
        assert_eq!(records.len(), 2); // 两个分组：A 和 B
        
        // 验证聚合结果
        for record in &records {
            assert!(record.get("category").is_some());
            assert!(record.get("avg_value").is_some());
            assert!(record.get("count").is_some());
            
            let category = record.get("category").unwrap().as_str().unwrap();
            let avg_value = record.get("avg_value").unwrap().as_f64().unwrap();
            let count = record.get("count").unwrap().as_f64().unwrap();
            
            if category == "A" {
                assert!((avg_value - 15.0).abs() < 0.001); // (10 + 20) / 2
                assert_eq!(count as u64, 2);
            } else if category == "B" {
                assert!((avg_value - 20.0).abs() < 0.001); // (15 + 25) / 2
                assert_eq!(count as u64, 2);
            }
        }
    } else {
        panic!("Expected array in output");
    }
    
    // 清理
    let _ = fs::remove_file(config_path);
    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_path);
    
    println!("✅ 聚合功能测试通过");
}

/// 测试 _apply_parse 核心逻辑
#[tokio::test]
async fn test_apply_parse_core_logic() {
    println!("🔍 测试 _apply_parse 核心逻辑...");
    
    // 这个测试验证 PythonAdapter::apply_parse 方法的核心逻辑
    // 由于这是内部方法，我们通过 adapt 命令来间接测试
    
    let complex_config = r#"
{
  "name": "complex_test_adapter",
  "adapter_type": "json",
  "description": "Complex test adapter with all features",
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
      "required": true,
      "validation": {
        "rule_type": "pattern",
        "params": {
          "pattern": "^user_\\d+$"
        }
      }
    },
    "score": {
      "source_field": "score",
      "target_field": "normalized_score",
      "type_conversion": "float",
      "required": false,
      "validation": {
        "rule_type": "range",
        "params": {
          "min": 0,
          "max": 100
        }
      }
    }
  },
  "filters": [
    {
      "filter_type": "regex",
      "field": "id",
      "condition": "^user_[12]$"
    }
  ],
  "aggregations": [
    {
      "aggregation_type": "avg",
      "group_by": ["id"],
      "aggregate_field": "normalized_score",
      "target_field": "avg_score"
    }
  ]
}"#;
    
    let test_input = r#"
[
  {
    "id": "user_1",
    "score": 85.5
  },
  {
    "id": "user_2",
    "score": 92.3
  },
  {
    "id": "user_3",
    "score": 78.9
  },
  {
    "id": "invalid_id",
    "score": 105.0
  }
]"#;
    
    let config_path = "/tmp/test_complex_config.json";
    let input_path = "/tmp/test_complex_input.json";
    let output_path = "/tmp/test_complex_output.json";
    
    // 写入测试文件
    fs::write(config_path, complex_config).expect("Failed to write config");
    fs::write(input_path, test_input).expect("Failed to write input");
    
    // 执行 adapt 命令
    let adapt_output = Command::new("cargo")
        .args(&["run", "--bin", "podflow-adapters", "--", "adapt",
                "--config", config_path,
                "--input", input_path,
                "--output", output_path,
                "--verbose"])
        .output()
        .expect("Failed to run adapt command");
    
    assert!(adapt_output.status.success());
    
    // 验证输出
    let output_content = fs::read_to_string(output_path)
        .expect("Failed to read output");
    
    println!("实际输出内容: {}", output_content);
    
    // 解析输出 JSON
    let output_json: serde_json::Value = serde_json::from_str(&output_content)
        .expect("Failed to parse output JSON");
    
    if let serde_json::Value::Array(records) = output_json {
        println!("记录数量: {}", records.len());
        // 应该只有两条记录（user_3 被过滤器过滤掉，invalid_id 被验证规则过滤掉）
        assert_eq!(records.len(), 2);
        
        // 验证字段映射和验证
        for (i, record) in records.iter().enumerate() {
            println!("记录 {}: {:?}", i, record);
            assert!(record.get("id").is_some());
            assert!(record.get("avg_score").is_some());
            
            let id = record.get("id").unwrap().as_str().unwrap();
            let avg_score_value = record.get("avg_score").unwrap();
            println!("ID: {}, avg_score值: {:?}", id, avg_score_value);
            
            let avg_score = match avg_score_value {
                serde_json::Value::Number(n) => n.as_f64().unwrap(),
                serde_json::Value::String(s) => s.parse::<f64>().unwrap(),
                _ => panic!("avg_score 不是数字或字符串: {:?}", avg_score_value),
            };
            
            if id == "user_1" {
                assert!((avg_score - 85.5).abs() < 0.001);
            } else if id == "user_2" {
                assert!((avg_score - 92.3).abs() < 0.001);
            }
        }
    } else {
        panic!("Expected array in output");
    }
    
    // 清理
    let _ = fs::remove_file(config_path);
    let _ = fs::remove_file(input_path);
    let _ = fs::remove_file(output_path);
    
    println!("✅ _apply_parse 核心逻辑测试通过");
}
