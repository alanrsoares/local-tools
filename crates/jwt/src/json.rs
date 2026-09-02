//! Pure JSON formatting and claim extraction without external dependencies.

/// Prettify a JSON string with 2-space indentation.
pub fn prettify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    let mut indent: usize = 0;
    let mut in_string = false;
    let mut escape = false;

    for ch in raw.chars() {
        if escape {
            out.push(ch);
            escape = false;
            continue;
        }

        if ch == '\\' && in_string {
            out.push(ch);
            escape = true;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            out.push(ch);
            continue;
        }

        if in_string {
            out.push(ch);
            continue;
        }

        match ch {
            '{' | '[' => {
                out.push(ch);
                indent += 2;
                out.push('\n');
                out.push_str(&" ".repeat(indent));
            }
            '}' | ']' => {
                indent = indent.saturating_sub(2);
                out.push('\n');
                out.push_str(&" ".repeat(indent));
                out.push(ch);
            }
            ',' => {
                out.push(ch);
                out.push('\n');
                out.push_str(&" ".repeat(indent));
            }
            ':' => {
                out.push(':');
                out.push(' ');
            }
            ' ' | '\t' | '\r' | '\n' => {
                // Skip raw whitespace outside strings
            }
            _ => out.push(ch),
        }
    }

    out
}

/// Extract a top-level key's value from a JSON object.
pub fn extract_field(json: &str, key: &str) -> Option<String> {
    let search_key = format!("\"{key}\"");
    let key_pos = json.find(&search_key)?;
    let after_key = &json[key_pos + search_key.len()..];
    let colon_pos = after_key.find(':')?;
    let val_part = after_key[colon_pos + 1..].trim();

    if let Some(inner) = val_part.strip_prefix('"') {
        // String value
        let end_quote = inner.find('"')?;
        Some(inner[..end_quote].to_string())
    } else {
        // Number, boolean, null, or token until comma or closing brace
        let mut end = 0;
        for (i, c) in val_part.char_indices() {
            if c == ',' || c == '}' || c == ']' || c.is_whitespace() {
                end = i;
                break;
            }
            end = i + c.len_utf8();
        }
        let res = val_part[..end].trim();
        if res.is_empty() {
            None
        } else {
            Some(res.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettify_json_string() {
        let raw = r#"{"sub":"1234","name":"Alan","admin":true}"#;
        let formatted = prettify(raw);
        assert!(formatted.contains("  \"sub\": \"1234\","));
        assert!(formatted.contains("  \"admin\": true"));
    }

    #[test]
    fn extract_field_values() {
        let json = r#"{"sub": "alan", "exp": 1725283900, "verified": true}"#;
        assert_eq!(extract_field(json, "sub"), Some("alan".to_string()));
        assert_eq!(extract_field(json, "exp"), Some("1725283900".to_string()));
        assert_eq!(extract_field(json, "verified"), Some("true".to_string()));
        assert_eq!(extract_field(json, "missing"), None);
    }
}
