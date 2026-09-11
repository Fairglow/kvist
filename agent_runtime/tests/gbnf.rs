use agent_runtime::compile_json_schema;
use serde_json::json;

#[test]
fn compile_simple_string_schema() {
    let schema = json!({"type": "string"});
    let grammar = compile_json_schema(&schema).expect("compile string schema");
    assert!(grammar.contains("root ::= string"));
    assert!(grammar.contains("string ::= "));
}

#[test]
fn compile_enum_schema() {
    let schema = json!({
        "type": "string",
        "enum": ["start", "stop", "pause"]
    });
    let grammar = compile_json_schema(&schema).expect("compile enum schema");
    assert!(grammar.contains("start"));
    assert!(grammar.contains("stop"));
    assert!(grammar.contains("pause"));
}

#[test]
fn compile_strict_object_schema_with_required_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "count": {"type": "integer"},
            "active": {"type": "boolean"}
        },
        "required": ["name", "count", "active"]
    });
    let grammar = compile_json_schema(&schema).expect("compile object schema");
    assert!(grammar.contains("root ::="));
    assert!(grammar.contains("\"\\\"name\\\"\" ws \":\""));
    assert!(grammar.contains("\"\\\"count\\\"\" ws \":\""));
    assert!(grammar.contains("\"\\\"active\\\"\" ws \":\""));
    assert!(grammar.contains("boolean"));
    assert!(grammar.contains("integer"));
    assert!(grammar.contains("string"));
    assert!(grammar.contains("ws ::= "));
}

#[test]
fn compile_tools_schema_generates_valid_tool_dispatch_grammar() {
    use agent_runtime::{ToolDefinition, compile_tools_schema};

    let tools = vec![
        ToolDefinition {
            name: "read_file".to_owned(),
            description: "Read a file from disk".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "write_file".to_owned(),
            description: "Write a file to disk".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        },
    ];

    let grammar = compile_tools_schema(&tools).expect("compile tools schema");
    assert!(grammar.contains("root ::="));
    assert!(grammar.contains("\"\\\"name\\\"\" ws \":\" ws \"\\\"read_file\\\"\""));
    assert!(grammar.contains("\"\\\"name\\\"\" ws \":\" ws \"\\\"write_file\\\"\""));
    assert!(grammar.contains("\"\\\"arguments\\\"\""));
    assert!(grammar.contains("\"\\\"path\\\"\""));
    assert!(grammar.contains("\"\\\"content\\\"\""));
}

#[test]
fn compile_tools_schema_rejects_empty_tools() {
    use agent_runtime::compile_tools_schema;
    assert!(compile_tools_schema(&[]).is_err());
}

#[test]
fn compile_nested_array_and_object_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"}
                    },
                    "required": ["id"]
                }
            }
        },
        "required": ["items"]
    });
    let grammar = compile_json_schema(&schema).expect("compile nested schema");
    assert!(grammar.contains("root ::="));
    assert!(grammar.contains(r#""[" ws"#));
    assert!(grammar.contains(r#""]" ws"#));
    assert!(grammar.contains("integer"));
}

#[test]
fn compile_rejects_non_object_schema() {
    let schema = json!("not an object");
    assert!(compile_json_schema(&schema).is_err());
}

#[test]
fn compile_object_with_optional_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "title": {"type": "string"},
            "optional_tag": {"type": "string"}
        },
        "required": ["title"]
    });
    let grammar = compile_json_schema(&schema).expect("compile optional props schema");
    assert!(grammar.contains("root ::="));
    assert!(grammar.contains("\"\\\"title\\\"\""));
    assert!(grammar.contains("\"\\\"optional_tag\\\"\""));
}

#[test]
fn compile_primitive_types_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "ratio": {"type": "number"},
            "flag": {"type": "boolean"},
            "nothing": {"type": "null"}
        },
        "required": ["ratio", "flag", "nothing"]
    });
    let grammar = compile_json_schema(&schema).expect("compile primitives schema");
    assert!(grammar.contains("number"));
    assert!(grammar.contains("boolean"));
    assert!(grammar.contains("null"));
}
