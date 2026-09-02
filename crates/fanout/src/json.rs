//! Lightweight, zero-dependency JSON parser.
//!
//! Provides a minimal AST and recursive-descent parser sufficient for reading
//! `package.json` manifests (workspaces, scripts, package names).

use std::collections::HashMap;

/// A minimal JSON value representation.
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
    /// Attempt to borrow as a string slice.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Attempt to borrow as an array slice.
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(arr) => Some(arr.as_slice()),
            _ => None,
        }
    }

    /// Attempt to borrow as an object map.
    pub fn as_object(&self) -> Option<&HashMap<String, JsonValue>> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Look up a key if this value is an object.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object().and_then(|m| m.get(key))
    }
}

/// Parse a JSON string into a `JsonValue`.
pub fn parse(input: &str) -> Result<JsonValue, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut idx = 0;
    skip_whitespace(&chars, &mut idx);
    let val = parse_value(&chars, &mut idx)?;
    skip_whitespace(&chars, &mut idx);
    if idx < chars.len() {
        return Err(format!(
            "trailing data after JSON root at character index {idx}"
        ));
    }
    Ok(val)
}

fn skip_whitespace(chars: &[char], idx: &mut usize) {
    while *idx < chars.len() && chars[*idx].is_whitespace() {
        *idx += 1;
    }
}

fn parse_value(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    skip_whitespace(chars, idx);
    if *idx >= chars.len() {
        return Err("unexpected end of input while parsing value".to_string());
    }

    match chars[*idx] {
        'n' => parse_null(chars, idx),
        't' | 'f' => parse_bool(chars, idx),
        '"' => parse_string(chars, idx).map(JsonValue::String),
        '[' => parse_array(chars, idx).map(JsonValue::Array),
        '{' => parse_object(chars, idx).map(JsonValue::Object),
        '-' | '0'..='9' => parse_number(chars, idx).map(JsonValue::Number),
        c => Err(format!("unexpected character '{c}' at index {idx}")),
    }
}

fn parse_null(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if chars.get(*idx..*idx + 4) == Some(&['n', 'u', 'l', 'l']) {
        *idx += 4;
        Ok(JsonValue::Null)
    } else {
        Err(format!("invalid literal at index {idx}"))
    }
}

fn parse_bool(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if chars.get(*idx..*idx + 4) == Some(&['t', 'r', 'u', 'e']) {
        *idx += 4;
        Ok(JsonValue::Bool(true))
    } else if chars.get(*idx..*idx + 5) == Some(&['f', 'a', 'l', 's', 'e']) {
        *idx += 5;
        Ok(JsonValue::Bool(false))
    } else {
        Err(format!("invalid boolean literal at index {idx}"))
    }
}

fn parse_string(chars: &[char], idx: &mut usize) -> Result<String, String> {
    if *idx >= chars.len() || chars[*idx] != '"' {
        return Err(format!("expected '\"' at index {idx}"));
    }
    *idx += 1; // skip opening quote

    let mut out = String::new();
    while *idx < chars.len() {
        let ch = chars[*idx];
        *idx += 1;
        match ch {
            '"' => return Ok(out),
            '\\' => {
                if *idx >= chars.len() {
                    return Err("unterminated escape sequence in string".to_string());
                }
                let esc = chars[*idx];
                *idx += 1;
                match esc {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\x08'),
                    'f' => out.push('\x0C'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        if *idx + 4 > chars.len() {
                            return Err("unterminated unicode escape sequence".to_string());
                        }
                        let hex_str: String = chars[*idx..*idx + 4].iter().collect();
                        *idx += 4;
                        let code = u32::from_str_radix(&hex_str, 16).map_err(|e| {
                            format!("invalid hex in unicode escape '\\u{hex_str}': {e}")
                        })?;
                        let decoded = char::from_u32(code)
                            .ok_or_else(|| format!("invalid unicode code point: {code}"))?;
                        out.push(decoded);
                    }
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            }
            other => out.push(other),
        }
    }
    Err("unterminated string literal".to_string())
}

fn parse_number(chars: &[char], idx: &mut usize) -> Result<f64, String> {
    let start = *idx;
    if chars[*idx] == '-' {
        *idx += 1;
    }
    while *idx < chars.len() && chars[*idx].is_ascii_digit() {
        *idx += 1;
    }
    if *idx < chars.len() && chars[*idx] == '.' {
        *idx += 1;
        while *idx < chars.len() && chars[*idx].is_ascii_digit() {
            *idx += 1;
        }
    }
    if *idx < chars.len() && (chars[*idx] == 'e' || chars[*idx] == 'E') {
        *idx += 1;
        if *idx < chars.len() && (chars[*idx] == '+' || chars[*idx] == '-') {
            *idx += 1;
        }
        while *idx < chars.len() && chars[*idx].is_ascii_digit() {
            *idx += 1;
        }
    }

    let num_str: String = chars[start..*idx].iter().collect();
    num_str
        .parse::<f64>()
        .map_err(|e| format!("invalid number '{num_str}': {e}"))
}

fn parse_array(chars: &[char], idx: &mut usize) -> Result<Vec<JsonValue>, String> {
    if *idx >= chars.len() || chars[*idx] != '[' {
        return Err(format!("expected '[' at index {idx}"));
    }
    *idx += 1; // skip '['

    let mut items = Vec::new();
    skip_whitespace(chars, idx);
    if *idx < chars.len() && chars[*idx] == ']' {
        *idx += 1;
        return Ok(items);
    }

    loop {
        skip_whitespace(chars, idx);
        let val = parse_value(chars, idx)?;
        items.push(val);
        skip_whitespace(chars, idx);

        if *idx >= chars.len() {
            return Err("unterminated array".to_string());
        }

        match chars[*idx] {
            ',' => {
                *idx += 1;
                // Allow trailing comma gracefully
                skip_whitespace(chars, idx);
                if *idx < chars.len() && chars[*idx] == ']' {
                    *idx += 1;
                    return Ok(items);
                }
            }
            ']' => {
                *idx += 1;
                return Ok(items);
            }
            c => {
                return Err(format!(
                    "expected ',' or ']' in array, found '{c}' at index {idx}"
                ))
            }
        }
    }
}

fn parse_object(chars: &[char], idx: &mut usize) -> Result<HashMap<String, JsonValue>, String> {
    if *idx >= chars.len() || chars[*idx] != '{' {
        return Err(format!("expected '{{' at index {idx}"));
    }
    *idx += 1; // skip '{'

    let mut map = HashMap::new();
    skip_whitespace(chars, idx);
    if *idx < chars.len() && chars[*idx] == '}' {
        *idx += 1;
        return Ok(map);
    }

    loop {
        skip_whitespace(chars, idx);
        if *idx >= chars.len() {
            return Err("unterminated object".to_string());
        }

        let key = parse_string(chars, idx)?;
        skip_whitespace(chars, idx);

        if *idx >= chars.len() || chars[*idx] != ':' {
            return Err(format!("expected ':' after key in object at index {idx}"));
        }
        *idx += 1; // skip ':'

        let val = parse_value(chars, idx)?;
        map.insert(key, val);
        skip_whitespace(chars, idx);

        if *idx >= chars.len() {
            return Err("unterminated object".to_string());
        }

        match chars[*idx] {
            ',' => {
                *idx += 1;
                // Allow trailing comma gracefully
                skip_whitespace(chars, idx);
                if *idx < chars.len() && chars[*idx] == '}' {
                    *idx += 1;
                    return Ok(map);
                }
            }
            '}' => {
                *idx += 1;
                return Ok(map);
            }
            c => {
                return Err(format!(
                    "expected ',' or '}}' in object, found '{c}' at index {idx}"
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_literals() {
        assert_eq!(parse("null").unwrap(), JsonValue::Null);
        assert_eq!(parse("true").unwrap(), JsonValue::Bool(true));
        assert_eq!(parse("false").unwrap(), JsonValue::Bool(false));
        assert_eq!(parse("42").unwrap(), JsonValue::Number(42.0));
        assert_eq!(parse("-3.5").unwrap(), JsonValue::Number(-3.5));
        assert_eq!(
            parse("\"hello world\"").unwrap(),
            JsonValue::String("hello world".into())
        );
    }

    #[test]
    fn parses_escaped_strings() {
        let json = r#""line1\nline2\t\"quoted\"\\slash""#;
        assert_eq!(
            parse(json).unwrap(),
            JsonValue::String("line1\nline2\t\"quoted\"\\slash".into())
        );
    }

    #[test]
    fn parses_arrays_and_objects() {
        let json = r#"
        {
            "name": "renkonos",
            "private": true,
            "workspaces": {
                "packages": [
                    "apps/*",
                    "packages/*"
                ]
            },
            "scripts": {
                "lint": "biome check",
                "typecheck": "tsc"
            }
        }
        "#;
        let v = parse(json).unwrap();
        assert_eq!(v.get("name").and_then(|n| n.as_str()), Some("renkonos"));
        assert_eq!(v.get("private"), Some(&JsonValue::Bool(true)));

        let ws = v.get("workspaces").unwrap();
        let pkgs = ws.get("packages").and_then(|p| p.as_array()).unwrap();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].as_str(), Some("apps/*"));
        assert_eq!(pkgs[1].as_str(), Some("packages/*"));

        let scripts = v.get("scripts").unwrap();
        assert_eq!(
            scripts.get("lint").and_then(|s| s.as_str()),
            Some("biome check")
        );
    }
}
