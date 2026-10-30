use serde_json::Value;

/// 一次或多次对话请求加总后的输入、输出 token。两项都是 0 表示接口没有带回用量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
}

impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
    }
}

pub fn omit(value: &u64) -> bool {
    *value == 0
}

pub fn label(input: u64, output: u64) -> Option<String> {
    if input == 0 && output == 0 {
        None
    } else {
        Some(format!("输入 {input} · 输出 {output}"))
    }
}

pub fn from_chat(kind: &str, body: &str) -> Usage {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return Usage::default();
    };
    let (node, inputs, outputs): (_, &[&str], &[&str]) = match kind {
        "anthropic" => (value.get("usage"), &["input_tokens"], &["output_tokens"]),
        "gemini" => (value.get("usageMetadata"), &["promptTokenCount"], &["candidatesTokenCount"]),
        "openai" | "ollama" | "azure" => (
            value.get("usage"),
            &["prompt_tokens", "input_tokens"],
            &["completion_tokens", "output_tokens"],
        ),
        _ => return Usage::default(),
    };
    let Some(node) = node.filter(|item| item.is_object()) else {
        return Usage::default();
    };
    Usage {
        input: count(node, inputs),
        output: count(node, outputs),
    }
}

fn count(node: &Value, keys: &[&str]) -> u64 {
    for key in keys {
        let Some(value) = node.get(*key) else {
            continue;
        };
        if let Some(number) = value.as_u64() {
            return number;
        }
        if let Some(number) = value.as_i64().and_then(|item| u64::try_from(item).ok()) {
            return number;
        }
        return 0;
    }
    0
}
