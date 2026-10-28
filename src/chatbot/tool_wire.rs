//! Accept vLLM native calls and complete Qwen wire envelopes. All decoded calls
//! still pass the same allowlist, argument, grounding and authorization checks.
use serde_json::{Value, json};

pub(in crate::chatbot) fn contains_wire_markup(value: &str) -> bool {
    let folded = value.to_ascii_lowercase();
    [
        "<tool_call",
        "</tool_call",
        "<function=",
        "</function",
        "<parameter=",
        "</parameter",
        "<|tool_call",
        "<|im_start|>",
    ]
    .iter()
    .any(|tag| folded.contains(tag))
}

pub(in crate::chatbot) fn decode_calls(
    message: &Value,
) -> Result<Option<Vec<Value>>, &'static str> {
    if let Some(calls) = message.get("tool_calls").filter(|calls| !calls.is_null()) {
        let calls = calls.as_array().ok_or("model_invalid_tool")?;
        if !calls.is_empty() {
            if calls.len() > 3 {
                return Err("model_invalid_tool");
            }
            return Ok(Some(calls.clone()));
        }
    }
    let content = message["content"].as_str().unwrap_or("");
    if !contains_wire_markup(content) {
        return Ok(None);
    }
    let mut remaining = content;
    let mut calls = Vec::new();
    while let Some(start) = remaining.find("<tool_call>") {
        if contains_wire_markup(&remaining[..start]) {
            return Err("model_invalid_tool");
        }
        let inner = &remaining[start + "<tool_call>".len()..];
        let (body, rest) = inner
            .split_once("</tool_call>")
            .ok_or("model_invalid_tool")?;
        let body = body.trim();
        let (name, args) = if body.starts_with('{') {
            let value: Value = serde_json::from_str(body).map_err(|_| "model_invalid_tool")?;
            let name = value["name"]
                .as_str()
                .ok_or("model_invalid_tool")?
                .to_owned();
            let args = match &value["arguments"] {
                Value::String(value) => {
                    serde_json::from_str(value).map_err(|_| "model_invalid_tool")?
                }
                value => value.clone(),
            };
            (name, args)
        } else {
            let body = body
                .strip_prefix("<function=")
                .ok_or("model_invalid_tool")?;
            let (name, body) = body.split_once('>').ok_or("model_invalid_tool")?;
            let mut parameters = body
                .trim()
                .strip_suffix("</function>")
                .ok_or("model_invalid_tool")?
                .trim();
            let mut args = serde_json::Map::new();
            while !parameters.is_empty() {
                let rest = parameters
                    .strip_prefix("<parameter=")
                    .ok_or("model_invalid_tool")?;
                let (key, rest) = rest.split_once('>').ok_or("model_invalid_tool")?;
                let (value, rest) = rest
                    .split_once("</parameter>")
                    .ok_or("model_invalid_tool")?;
                if key.is_empty()
                    || key.len() > 64
                    || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    || args.contains_key(key)
                {
                    return Err("model_invalid_tool");
                }
                let value = value.trim();
                // Text parameters remain text; booleans/numbers keep schema types.
                let parsed = serde_json::from_str::<Value>(value)
                    .unwrap_or_else(|_| Value::String(value.to_owned()));
                args.insert(key.to_owned(), parsed);
                parameters = rest.trim();
            }
            (name.to_owned(), Value::Object(args))
        };
        if !args.is_object()
            || name.is_empty()
            || name.len() > 64
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("model_invalid_tool");
        }
        calls.push(json!({"id":format!("qwen-wire-{}",calls.len()+1),"type":"function","function":{"name":name,"arguments":args.to_string()}}));
        if calls.len() > 3 {
            return Err("model_invalid_tool");
        }
        remaining = rest;
    }
    if calls.is_empty() || contains_wire_markup(remaining) {
        return Err("model_invalid_tool");
    }
    Ok(Some(calls))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_qwen_xml_and_json_are_real_calls_not_chat_prose() {
        for content in [
            r#"<tool_call><function=roll_die><parameter=sides>6</parameter></function></tool_call>"#,
            r#"<tool_call>{"name":"roll_die","arguments":{"sides":6}}</tool_call>"#,
        ] {
            let calls = decode_calls(&json!({"content":content})).unwrap().unwrap();
            assert_eq!(calls[0]["function"]["name"], "roll_die");
            assert_eq!(calls[0]["function"]["arguments"], r#"{"sides":6}"#);
        }
    }
    #[test]
    fn fragments_duplicates_nested_tags_and_excess_calls_are_not_executed() {
        for content in [
            "Ja, no! </parameter></function></tool_call>",
            "<tool_call><function=roll_die><parameter=sides>6",
            "<tool_call><function=roll_die><parameter=sides>6</parameter><parameter=sides>2</parameter></function></tool_call>",
            "<tool_call><tool_call>{}</tool_call></tool_call>",
        ] {
            assert!(contains_wire_markup(content));
            assert!(decode_calls(&json!({"content":content})).is_err());
        }
    }
}
