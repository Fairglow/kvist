use serde_json::Value;

use crate::{Error, Result, ToolDefinition};

/// Quotes and escapes a string literal into a GBNF terminal string matching `"value"`.
fn gbnf_quote_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!(r#""\"{}\"""#, escaped)
}

/// Compiles a bounded JSON Schema into a GBNF (GGML BNF) grammar string.
///
/// Supports objects with properties, required fields, arrays, strings,
/// integers, numbers, booleans, null, and string enumerations.
pub fn compile_json_schema(schema: &Value) -> Result<String> {
    let mut compiler = GbnfCompiler::new();
    let root_expr = compiler.compile_type("root", schema)?;
    compiler.add_rule("root", root_expr);
    Ok(compiler.finish())
}

/// Compiles a slice of tool definitions into a GBNF grammar string enforcing
/// structured tool selection: `{"name": "<tool_name>", "arguments": <parameters>}`.
pub fn compile_tools_schema(tools: &[ToolDefinition]) -> Result<String> {
    if tools.is_empty() {
        return Err(Error::InvalidModelRequest {
            reason: "cannot compile GBNF grammar for empty tool definitions".to_owned(),
        });
    }

    let mut compiler = GbnfCompiler::new();
    let mut tool_rules = Vec::new();

    for (index, tool) in tools.iter().enumerate() {
        let tool_hint = format!("tool_{index}_{}", tool.name.replace('-', "_"));
        let args_rule = compiler.compile_type(&format!("{tool_hint}_args"), &tool.parameters)?;
        let name_terminal = gbnf_quote_string(&tool.name);

        let call_rule = format!("{tool_hint}_call");
        let call_expr = format!(
            r#""{{" ws "\"name\"" ws ":" ws {name_terminal} ws "," ws "\"arguments\"" ws ":" ws {args_rule} "}}" ws"#
        );
        compiler.add_rule(&call_rule, call_expr);
        tool_rules.push(call_rule);
    }

    let single_call = tool_rules.join(" | ");
    let root_expr = format!("({single_call})");
    compiler.add_rule("root", root_expr);
    Ok(compiler.finish())
}

struct GbnfCompiler {
    rules: Vec<(String, String)>,
    rule_counter: usize,
}

impl GbnfCompiler {
    fn new() -> Self {
        Self {
            rules: Vec::new(),
            rule_counter: 0,
        }
    }

    fn add_rule(&mut self, name: &str, expression: String) {
        if !self.rules.iter().any(|(n, _)| n == name) {
            self.rules.push((name.to_owned(), expression));
        }
    }

    fn next_rule_name(&mut self, prefix: &str) -> String {
        self.rule_counter += 1;
        format!("{prefix}_{}", self.rule_counter)
    }

    fn compile_type(&mut self, hint: &str, schema: &Value) -> Result<String> {
        let schema_obj = match schema {
            Value::Object(map) => map,
            _ => {
                return Err(Error::InvalidModelRequest {
                    reason: "schema must be a JSON object".to_owned(),
                });
            }
        };

        // Check enum first
        if let Some(Value::Array(enum_vals)) = schema_obj.get("enum") {
            if enum_vals.is_empty() {
                return Err(Error::InvalidModelRequest {
                    reason: "enum in schema must not be empty".to_owned(),
                });
            }
            let mut options = Vec::new();
            for val in enum_vals {
                match val {
                    Value::String(s) => {
                        let terminal = gbnf_quote_string(s);
                        options.push(format!("{terminal} ws"));
                    }
                    Value::Number(n) => {
                        options.push(format!("\"{n}\" ws"));
                    }
                    Value::Bool(b) => {
                        options.push(format!("\"{}\" ws", if *b { "true" } else { "false" }));
                    }
                    Value::Null => {
                        options.push("\"null\" ws".to_owned());
                    }
                    _ => {
                        return Err(Error::InvalidModelRequest {
                            reason: "enum values must be primitive literals".to_owned(),
                        });
                    }
                }
            }
            let rule_name = self.next_rule_name(hint);
            let expr = format!("({})", options.join(" | "));
            self.add_rule(&rule_name, expr);
            return Ok(rule_name);
        }

        let type_name = match schema_obj.get("type") {
            Some(Value::String(t)) => t.as_str(),
            None => {
                if schema_obj.contains_key("properties") {
                    "object"
                } else {
                    "value"
                }
            }
            Some(_) => {
                return Err(Error::InvalidModelRequest {
                    reason: "schema type must be a string".to_owned(),
                });
            }
        };

        match type_name {
            "string" => Ok("string".to_owned()),
            "number" => Ok("number".to_owned()),
            "integer" => Ok("integer".to_owned()),
            "boolean" => Ok("boolean".to_owned()),
            "null" => Ok("null".to_owned()),
            "array" => self.compile_array(hint, schema_obj),
            "object" => self.compile_object(hint, schema_obj),
            "value" => Ok("value".to_owned()),
            other => Err(Error::InvalidModelRequest {
                reason: format!("unsupported schema type: {other}"),
            }),
        }
    }

    fn compile_array(
        &mut self,
        hint: &str,
        schema: &serde_json::Map<String, Value>,
    ) -> Result<String> {
        let item_expr = if let Some(items) = schema.get("items") {
            let item_hint = format!("{hint}_item");
            self.compile_type(&item_hint, items)?
        } else {
            "value".to_owned()
        };

        let rule_name = self.next_rule_name(hint);
        let expr = format!(r#""[" ws ({item_expr} ("," ws {item_expr})*)? "]" ws"#);
        self.add_rule(&rule_name, expr);
        Ok(rule_name)
    }

    fn compile_object(
        &mut self,
        hint: &str,
        schema: &serde_json::Map<String, Value>,
    ) -> Result<String> {
        let properties = match schema.get("properties") {
            Some(Value::Object(props)) => props,
            Some(_) => {
                return Err(Error::InvalidModelRequest {
                    reason: "object properties must be an object".to_owned(),
                });
            }
            None => {
                return Ok("object".to_owned());
            }
        };

        if properties.is_empty() {
            let rule_name = self.next_rule_name(hint);
            self.add_rule(&rule_name, r#""{" ws "}" ws"#.to_owned());
            return Ok(rule_name);
        }

        let required_set: std::collections::HashSet<&str> = match schema.get("required") {
            Some(Value::Array(reqs)) => reqs.iter().filter_map(|val| val.as_str()).collect(),
            _ => std::collections::HashSet::new(),
        };

        let mut sorted_props: Vec<(&String, &Value)> = properties.iter().collect();
        sorted_props.sort_by_key(|(k, _)| *k);

        let all_required = !sorted_props.is_empty()
            && sorted_props
                .iter()
                .all(|(k, _)| required_set.contains(k.as_str()));

        if all_required {
            let mut parts = Vec::new();
            for (k, v) in &sorted_props {
                let prop_hint = format!("{hint}_{k}");
                let val_rule = self.compile_type(&prop_hint, v)?;
                let prop_rule = self.next_rule_name(&prop_hint);
                let key_terminal = gbnf_quote_string(k);
                self.add_rule(&prop_rule, format!("{key_terminal} ws \":\" ws {val_rule}"));
                parts.push(prop_rule);
            }
            let rule_name = self.next_rule_name(hint);
            let seq = parts.join(r#" "," ws "#);
            let expr = format!(r#""{{" ws {seq} "}}" ws"#);
            self.add_rule(&rule_name, expr);
            return Ok(rule_name);
        }

        let mut prop_rules = Vec::new();
        for (k, v) in &sorted_props {
            let prop_hint = format!("{hint}_{k}");
            let val_rule = self.compile_type(&prop_hint, v)?;
            let prop_rule = self.next_rule_name(&prop_hint);
            let key_terminal = gbnf_quote_string(k);
            self.add_rule(&prop_rule, format!("{key_terminal} ws \":\" ws {val_rule}"));
            prop_rules.push(prop_rule);
        }

        let kv_rule = self.next_rule_name(&format!("{hint}_kv"));
        self.add_rule(&kv_rule, prop_rules.join(" | "));

        let rule_name = self.next_rule_name(hint);
        let expr = format!(r#""{{" ws ({kv_rule} ("," ws {kv_rule})*)? "}}" ws"#);
        self.add_rule(&rule_name, expr);
        Ok(rule_name)
    }

    fn finish(self) -> String {
        let mut out = String::new();
        for (name, expr) in &self.rules {
            out.push_str(&format!("{name} ::= {expr}\n"));
        }
        out.push_str("ws ::= [ \\t\\n\\r]*\n");
        out.push_str(r#"string ::= "\"" ([^"\\] | "\\" (["\\/bfnrt] | "u" [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F] [0-9a-fA-F]))* "\"" ws"#);
        out.push('\n');
        out.push_str(
            r#"number ::= ("-"? ([0-9] | [1-9] [0-9]*)) ("." [0-9]+)? ([eE] [-+]? [0-9]+)? ws"#,
        );
        out.push('\n');
        out.push_str(r#"integer ::= ("-"? ([0-9] | [1-9] [0-9]*)) ws"#);
        out.push('\n');
        out.push_str(r#"boolean ::= ("true" | "false") ws"#);
        out.push('\n');
        out.push_str(r#"null ::= "null" ws"#);
        out.push('\n');
        out.push_str(
            r#"object ::= "{" ws (string ":" ws value ("," ws string ":" ws value)*)? "}" ws"#,
        );
        out.push('\n');
        out.push_str(r#"array ::= "[" ws (value ("," ws value)*)? "]" ws"#);
        out.push('\n');
        out.push_str("value ::= object | array | string | number | boolean | null\n");
        out
    }
}
