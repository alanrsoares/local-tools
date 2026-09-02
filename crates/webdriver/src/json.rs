//! Zero-dependency JSON parser and serializer for CDP messages.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Number(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&HashMap<String, JsonValue>> {
        match self {
            JsonValue::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn to_json_string(&self) -> String {
        match self {
            JsonValue::Null => "null".to_string(),
            JsonValue::Bool(b) => b.to_string(),
            JsonValue::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    format!("{:.0}", n)
                } else {
                    format!("{}", n)
                }
            }
            JsonValue::String(s) => format!("\"{}\"", escape_str(s)),
            JsonValue::Array(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.to_json_string()).collect();
                format!("[{}]", inner.join(","))
            }
            JsonValue::Object(map) => {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("\"{}\":{}", escape_str(k), v.to_json_string()))
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
        }
    }
}

pub fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

pub struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
        }
    }

    pub fn parse(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        let val = self.parse_value()?;
        self.skip_whitespace();
        Ok(val)
    }

    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.chars.peek() {
            Some('"') => self.parse_string().map(JsonValue::String),
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('t') | Some('f') => self.parse_bool(),
            Some('n') => self.parse_null(),
            Some(&c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(&c) => Err(format!("unexpected character in json: '{c}'")),
            None => Err("unexpected end of json input".to_string()),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.chars.next() != Some('"') {
            return Err("expected opening quote".to_string());
        }

        let mut s = String::new();
        let mut escape = false;

        while let Some(c) = self.chars.next() {
            if escape {
                match c {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    '/' => s.push('/'),
                    'b' => s.push('\x08'),
                    'f' => s.push('\x0C'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    'u' => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            if let Some(h) = self.chars.next() {
                                hex.push(h);
                            } else {
                                return Err("truncated unicode escape".to_string());
                            }
                        }
                        if let Ok(code) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(code) {
                                s.push(ch);
                            }
                        }
                    }
                    _ => s.push(c),
                }
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                return Ok(s);
            } else {
                s.push(c);
            }
        }

        Err("unterminated string literal".to_string())
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        if self.chars.next() != Some('{') {
            return Err("expected '{'".to_string());
        }

        let mut map = HashMap::new();
        self.skip_whitespace();

        if let Some(&'}') = self.chars.peek() {
            self.chars.next();
            return Ok(JsonValue::Object(map));
        }

        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();

            if self.chars.next() != Some(':') {
                return Err("expected ':' after object key".to_string());
            }

            let val = self.parse_value()?;
            map.insert(key, val);

            self.skip_whitespace();
            match self.chars.next() {
                Some(',') => continue,
                Some('}') => break,
                Some(c) => return Err(format!("expected ',' or '}}' in object, got '{c}'")),
                None => return Err("unterminated object".to_string()),
            }
        }

        Ok(JsonValue::Object(map))
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        if self.chars.next() != Some('[') {
            return Err("expected '['".to_string());
        }

        let mut arr = Vec::new();
        self.skip_whitespace();

        if let Some(&']') = self.chars.peek() {
            self.chars.next();
            return Ok(JsonValue::Array(arr));
        }

        loop {
            let val = self.parse_value()?;
            arr.push(val);

            self.skip_whitespace();
            match self.chars.next() {
                Some(',') => continue,
                Some(']') => break,
                Some(c) => return Err(format!("expected ',' or ']' in array, got '{c}'")),
                None => return Err("unterminated array".to_string()),
            }
        }

        Ok(JsonValue::Array(arr))
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        let mut buf = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_alphabetic() {
                buf.push(self.chars.next().unwrap());
            } else {
                break;
            }
        }
        match buf.as_str() {
            "true" => Ok(JsonValue::Bool(true)),
            "false" => Ok(JsonValue::Bool(false)),
            _ => Err(format!("invalid boolean literal '{buf}'")),
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        let mut buf = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_alphabetic() {
                buf.push(self.chars.next().unwrap());
            } else {
                break;
            }
        }
        if buf == "null" {
            Ok(JsonValue::Null)
        } else {
            Err(format!("invalid null literal '{buf}'"))
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let mut buf = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                buf.push(self.chars.next().unwrap());
            } else {
                break;
            }
        }
        buf.parse::<f64>()
            .map(JsonValue::Number)
            .map_err(|e| format!("invalid number '{buf}': {e}"))
    }
}

pub fn parse(input: &str) -> Result<JsonValue, String> {
    Parser::new(input).parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_complex_json() {
        let raw = r#"{
            "id": 1,
            "method": "Page.navigate",
            "params": {
                "url": "https://example.com",
                "timeout": 30000,
                "flags": ["fast", "silent"]
            },
            "enabled": true,
            "session": null
        }"#;

        let val = parse(raw).expect("parse failed");
        assert_eq!(val.get("id").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            val.get("method").and_then(|v| v.as_str()),
            Some("Page.navigate")
        );

        let params = val.get("params").unwrap();
        assert_eq!(
            params.get("url").and_then(|v| v.as_str()),
            Some("https://example.com")
        );
        assert_eq!(val.get("enabled").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(val.get("session"), Some(&JsonValue::Null));
    }
}
