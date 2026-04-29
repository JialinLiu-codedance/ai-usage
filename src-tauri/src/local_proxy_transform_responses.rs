use crate::local_proxy_error::LocalProxyTransformError;
use serde_json::{json, Value};

pub fn anthropic_to_responses(
    body: Value,
    is_codex_oauth: bool,
) -> Result<Value, LocalProxyTransformError> {
    let mut result = json!({});

    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    if let Some(system) = body.get("system") {
        let instructions = if let Some(text) = system.as_str() {
            text.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|msg| msg.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            String::new()
        };
        if !instructions.is_empty() {
            result["instructions"] = json!(instructions);
        }
    }

    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        result["input"] = json!(convert_messages_to_input(msgs)?);
    }

    if let Some(v) = body.get("max_tokens") {
        result["max_output_tokens"] = v.clone();
    }
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let response_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": t.get("description"),
                    "parameters": t.get("input_schema").cloned().unwrap_or(json!({}))
                })
            })
            .collect();
        if !response_tools.is_empty() {
            result["tools"] = json!(response_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = map_tool_choice_to_responses(v);
    }

    if is_codex_oauth {
        result["store"] = json!(false);
        result["include"] = json!(["reasoning.encrypted_content"]);
        if let Some(obj) = result.as_object_mut() {
            obj.remove("max_output_tokens");
            obj.remove("temperature");
            obj.remove("top_p");
            obj.entry("instructions".to_string()).or_insert(json!(""));
            obj.entry("tools".to_string()).or_insert(json!([]));
            obj.entry("parallel_tool_calls".to_string())
                .or_insert(json!(false));
            obj.insert("stream".to_string(), json!(true));
        }
    }

    Ok(result)
}

fn map_tool_choice_to_responses(tool_choice: &Value) -> Value {
    match tool_choice {
        Value::String(_) => tool_choice.clone(),
        Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
            Some("any") => json!("required"),
            Some("auto") => json!("auto"),
            Some("none") => json!("none"),
            Some("tool") => {
                let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
                json!({
                    "type": "function",
                    "name": name
                })
            }
            _ => tool_choice.clone(),
        },
        _ => tool_choice.clone(),
    }
}

pub(crate) fn map_responses_stop_reason(
    status: Option<&str>,
    has_tool_use: bool,
    incomplete_reason: Option<&str>,
) -> Option<&'static str> {
    status.map(|s| match s {
        "completed" if has_tool_use => "tool_use",
        "incomplete"
            if matches!(
                incomplete_reason,
                Some("max_output_tokens") | Some("max_tokens")
            ) || incomplete_reason.is_none() =>
        {
            "max_tokens"
        }
        "incomplete" => "end_turn",
        _ => "end_turn",
    })
}

fn convert_messages_to_input(msgs: &[Value]) -> Result<Vec<Value>, LocalProxyTransformError> {
    let mut input = Vec::new();

    for msg in msgs {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content");

        if let Some(content) = content.and_then(|value| value.as_array()) {
            let mut text_parts = Vec::new();
            for block in content {
                match block.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(json!({
                                "type": if role == "assistant" { "output_text" } else { "input_text" },
                                "text": text
                            }));
                        }
                    }
                    "tool_use" => {
                        input.push(json!({
                            "type": "function_call",
                            "call_id": block.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            "name": block.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                            "arguments": serde_json::to_string(&block.get("input").cloned().unwrap_or(json!({}))).unwrap_or_default()
                        }));
                    }
                    "tool_result" => {
                        let output = match block.get("content") {
                            Some(Value::String(text)) => text.clone(),
                            Some(value) => serde_json::to_string(value).unwrap_or_default(),
                            None => String::new(),
                        };
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": block.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or(""),
                            "output": output
                        }));
                    }
                    _ => {}
                }
            }

            if !text_parts.is_empty() {
                input.push(json!({
                    "role": role,
                    "content": text_parts
                }));
            }
        } else if let Some(text) = content.and_then(|value| value.as_str()) {
            input.push(json!({
                "role": role,
                "content": [{ "type": if role == "assistant" { "output_text" } else { "input_text" }, "text": text }]
            }));
        }
    }

    Ok(input)
}

pub fn responses_to_anthropic(body: Value) -> Result<Value, LocalProxyTransformError> {
    let output = body
        .get("output")
        .and_then(|value| value.as_array())
        .ok_or_else(|| LocalProxyTransformError::TransformError("No output in response".into()))?;

    let mut content = Vec::new();
    let mut has_tool_use = false;

    for item in output {
        match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "message" => {
                if let Some(msg_content) = item.get("content").and_then(|v| v.as_array()) {
                    for block in msg_content {
                        if block.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    content.push(json!({"type": "text", "text": text}));
                                }
                            }
                        } else if block.get("type").and_then(|v| v.as_str()) == Some("refusal") {
                            if let Some(text) = block.get("refusal").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    content.push(json!({"type": "text", "text": text}));
                                }
                            }
                        }
                    }
                }
            }
            "function_call" => {
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args_str = item
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                content.push(json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": input
                }));
                has_tool_use = true;
            }
            _ => {}
        }
    }

    let stop_reason = map_responses_stop_reason(
        body.get("status").and_then(|value| value.as_str()),
        has_tool_use,
        body.pointer("/incomplete_details/reason")
            .and_then(|reason| reason.as_str()),
    );

    let usage_json = build_anthropic_usage_from_responses(body.get("usage"));

    Ok(json!({
        "id": body.get("id").and_then(|v| v.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("model").and_then(|v| v.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": usage_json
    }))
}

pub(crate) fn build_anthropic_usage_from_responses(usage: Option<&Value>) -> Value {
    let u = match usage {
        Some(v) if !v.is_null() => v,
        _ => {
            return json!({
                "input_tokens": 0,
                "output_tokens": 0
            })
        }
    };

    let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output = u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    json!({
        "input_tokens": input,
        "output_tokens": output,
        "cache_read_input_tokens": u.pointer("/input_tokens_details/cached_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        "cache_creation_input_tokens": u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
    })
}
