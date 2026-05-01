use serde_json::{Map, Value};

pub fn render(value: &Value, ctx: &Value) -> Value {
    match value {
        Value::String(s) => render_string(s, ctx),
        Value::Array(arr) => Value::Array(arr.iter().map(|v| render(v, ctx)).collect()),
        Value::Object(obj) => {
            let mut out = Map::with_capacity(obj.len());
            for (k, v) in obj {
                out.insert(k.clone(), render(v, ctx));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn render_string(s: &str, ctx: &Value) -> Value {
    let trimmed = s.trim();
    if let Some(path) = whole_placeholder(trimmed) {
        return resolve(path, ctx);
    }

    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && &bytes[i..i + 2] == b"{{" {
            if let Some(end) = find_close(&bytes[i + 2..]) {
                let path = &s[i + 2..i + 2 + end].trim();
                let resolved = resolve(path, ctx);
                out.push_str(&value_to_string(&resolved));
                i += 2 + end + 2;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Value::String(out)
}

fn whole_placeholder(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.contains("}}") || inner.contains("{{") {
        return None;
    }
    Some(inner.trim())
}

fn find_close(b: &[u8]) -> Option<usize> {
    for i in 0..b.len().saturating_sub(1) {
        if &b[i..i + 2] == b"}}" {
            return Some(i);
        }
    }
    None
}

fn resolve(path: &str, ctx: &Value) -> Value {
    let mut cur = ctx;
    for seg in path.split('.') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        cur = match cur {
            Value::Object(map) => match map.get(seg) {
                Some(v) => v,
                None => return Value::String(String::new()),
            },
            Value::Array(arr) => match seg.parse::<usize>() {
                Ok(idx) => match arr.get(idx) {
                    Some(v) => v,
                    None => return Value::String(String::new()),
                },
                Err(_) => return Value::String(String::new()),
            },
            _ => return Value::String(String::new()),
        };
    }
    cur.clone()
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_scalar_in_string() {
        let ctx = json!({ "steps": { "fetch": { "url": "https://example.com" } } });
        let v = render(&json!({ "u": "got: {{steps.fetch.url}}" }), &ctx);
        assert_eq!(v["u"], json!("got: https://example.com"));
    }

    #[test]
    fn whole_string_keeps_raw_value() {
        let ctx = json!({ "steps": { "fetch": { "body": { "items": [1,2,3] } } } });
        let v = render(&json!({ "x": "{{steps.fetch.body}}" }), &ctx);
        assert_eq!(v["x"], json!({ "items": [1,2,3] }));
    }

    #[test]
    fn missing_path_yields_empty_string() {
        let ctx = json!({});
        let v = render(&json!({ "x": "{{steps.unknown.field}}" }), &ctx);
        assert_eq!(v["x"], json!(""));
    }

    #[test]
    fn walks_nested_arrays_and_objects() {
        let ctx = json!({ "input": { "name": "Vasya" } });
        let v = render(
            &json!({
                "msg": "Hi {{input.name}}",
                "list": ["raw", "{{input.name}}"],
                "deep": { "k": "{{input.name}}" }
            }),
            &ctx,
        );
        assert_eq!(v["msg"], json!("Hi Vasya"));
        assert_eq!(v["list"][1], json!("Vasya"));
        assert_eq!(v["deep"]["k"], json!("Vasya"));
    }
}
