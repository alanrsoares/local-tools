//! DSL grammar and step representation for browser automation.

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Goto(String),
    Viewport {
        width: u32,
        height: u32,
    },
    WaitFor {
        selector: String,
        timeout_ms: u64,
    },
    Wait {
        duration_ms: u64,
    },
    Click {
        selector: String,
    },
    Type {
        selector: String,
        text: String,
        clear: bool,
    },
    Eval {
        expr: String,
    },
    Screenshot {
        path: String,
        full_page: bool,
        selector: Option<String>,
    },
    Pdf {
        path: String,
    },
    Html {
        path: Option<String>,
    },
}

/// Parse a duration string like "60s", "500ms", "2m", "10".
pub fn parse_duration_ms(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration string".to_string());
    }

    if let Some(ms_str) = s.strip_suffix("ms") {
        ms_str
            .parse::<u64>()
            .map_err(|e| format!("invalid millisecond duration '{s}': {e}"))
    } else if let Some(s_str) = s.strip_suffix('s') {
        let secs = s_str
            .parse::<f64>()
            .map_err(|e| format!("invalid second duration '{s}': {e}"))?;
        Ok((secs * 1000.0).round() as u64)
    } else if let Some(m_str) = s.strip_suffix('m') {
        let mins = m_str
            .parse::<f64>()
            .map_err(|e| format!("invalid minute duration '{s}': {e}"))?;
        Ok((mins * 60_000.0).round() as u64)
    } else {
        // Default bare numbers to seconds
        let secs = s
            .parse::<f64>()
            .map_err(|e| format!("invalid duration '{s}': {e}"))?;
        Ok((secs * 1000.0).round() as u64)
    }
}

/// Parse a multi-line script string into DSL steps.
pub fn parse_script(script: &str, default_timeout_ms: u64) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();

    for line in script.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let tokens = tokenize_line(trimmed)?;
        if tokens.is_empty() {
            continue;
        }

        let parsed = parse_tokens(&tokens, default_timeout_ms)?;
        steps.extend(parsed);
    }

    Ok(steps)
}

/// Split a line into tokens, respecting single and double quotes.
pub fn tokenize_line(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = None;
    let mut escape = false;

    for ch in line.chars() {
        if escape {
            current.push(ch);
            escape = false;
            continue;
        }

        if ch == '\\' {
            escape = true;
            continue;
        }

        if let Some(q) = in_quote {
            if ch == q {
                in_quote = None;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }

    if in_quote.is_some() {
        return Err("unclosed quote in line".to_string());
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

/// Parse a slice of string tokens into DSL steps.
pub fn parse_tokens(tokens: &[String], default_timeout_ms: u64) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    let mut idx = 0;

    while idx < tokens.len() {
        let tok = &tokens[idx];

        // First positional or bare URL
        if idx == 0 && is_url(tok) {
            steps.push(Step::Goto(tok.clone()));
            idx += 1;
            continue;
        }

        match tok.as_str() {
            "goto" | "open" | "navigate" => {
                idx += 1;
                if idx >= tokens.len() {
                    return Err(format!("'{tok}' requires a URL argument"));
                }
                steps.push(Step::Goto(tokens[idx].clone()));
                idx += 1;
            }
            "viewport" | "resize" => {
                idx += 1;
                if idx >= tokens.len() {
                    return Err(format!("'{tok}' requires width and height arguments"));
                }
                let w_str = &tokens[idx];
                idx += 1;
                let (w, h) = if w_str.contains('x') || w_str.contains('*') {
                    let parts: Vec<&str> = w_str.split(['x', '*']).collect();
                    if parts.len() != 2 {
                        return Err(format!("invalid viewport dimensions '{w_str}'"));
                    }
                    let w = parts[0]
                        .parse::<u32>()
                        .map_err(|e| format!("invalid width: {e}"))?;
                    let h = parts[1]
                        .parse::<u32>()
                        .map_err(|e| format!("invalid height: {e}"))?;
                    (w, h)
                } else {
                    if idx >= tokens.len() {
                        return Err(format!("'{tok}' requires height after width"));
                    }
                    let w = w_str
                        .parse::<u32>()
                        .map_err(|e| format!("invalid width: {e}"))?;
                    let h = tokens[idx]
                        .parse::<u32>()
                        .map_err(|e| format!("invalid height: {e}"))?;
                    idx += 1;
                    (w, h)
                };
                steps.push(Step::Viewport {
                    width: w,
                    height: h,
                });
            }
            "wait-for" | "waitfor" => {
                idx += 1;
                if idx >= tokens.len() {
                    return Err(format!("'{tok}' requires a CSS selector argument"));
                }
                let selector = tokens[idx].clone();
                idx += 1;

                let mut timeout_ms = default_timeout_ms;
                if idx < tokens.len() {
                    if tokens[idx] == "--timeout" || tokens[idx] == "-t" {
                        idx += 1;
                        if idx < tokens.len() {
                            timeout_ms = parse_duration_ms(&tokens[idx])?;
                            idx += 1;
                        }
                    } else if let Ok(d) = parse_duration_ms(&tokens[idx]) {
                        timeout_ms = d;
                        idx += 1;
                    }
                }
                steps.push(Step::WaitFor {
                    selector,
                    timeout_ms,
                });
            }
            "wait" | "sleep" => {
                idx += 1;
                if idx >= tokens.len() {
                    return Err(format!(
                        "'{tok}' requires a duration argument (e.g. 500ms, 2s)"
                    ));
                }
                let duration_ms = parse_duration_ms(&tokens[idx])?;
                steps.push(Step::Wait { duration_ms });
                idx += 1;
            }
            "click" => {
                idx += 1;
                if idx >= tokens.len() {
                    return Err("click requires a CSS selector argument".to_string());
                }
                steps.push(Step::Click {
                    selector: tokens[idx].clone(),
                });
                idx += 1;
            }
            "type" | "fill" | "input" => {
                let clear = tok == "fill";
                idx += 1;
                if idx + 1 >= tokens.len() {
                    return Err(format!("'{tok}' requires <selector> and <text> arguments"));
                }
                let selector = tokens[idx].clone();
                let text = tokens[idx + 1].clone();
                steps.push(Step::Type {
                    selector,
                    text,
                    clear,
                });
                idx += 2;
            }
            "eval" | "exec" | "js" => {
                idx += 1;
                if idx >= tokens.len() {
                    return Err("eval requires a JavaScript expression argument".to_string());
                }
                steps.push(Step::Eval {
                    expr: tokens[idx].clone(),
                });
                idx += 1;
            }
            "screenshot" | "--screenshot" => {
                idx += 1;
                let mut path = "screenshot.png".to_string();
                let mut full_page = false;
                let mut selector = None;

                while idx < tokens.len() {
                    if tokens[idx] == "--full-page" || tokens[idx] == "-f" {
                        full_page = true;
                        idx += 1;
                    } else if tokens[idx] == "--selector" || tokens[idx] == "-s" {
                        idx += 1;
                        if idx < tokens.len() {
                            selector = Some(tokens[idx].clone());
                            idx += 1;
                        }
                    } else if !tokens[idx].starts_with('-')
                        && (tokens[idx].ends_with(".png")
                            || tokens[idx].ends_with(".jpg")
                            || tokens[idx].ends_with(".jpeg")
                            || !is_verb(&tokens[idx]))
                    {
                        path = tokens[idx].clone();
                        idx += 1;
                    } else {
                        break;
                    }
                }
                steps.push(Step::Screenshot {
                    path,
                    full_page,
                    selector,
                });
            }
            "pdf" | "--pdf" => {
                idx += 1;
                let path = if idx < tokens.len()
                    && !tokens[idx].starts_with('-')
                    && !is_verb(&tokens[idx])
                {
                    let p = tokens[idx].clone();
                    idx += 1;
                    p
                } else {
                    "page.pdf".to_string()
                };
                steps.push(Step::Pdf { path });
            }
            "html" | "--html" | "dump" => {
                idx += 1;
                let path = if idx < tokens.len()
                    && !tokens[idx].starts_with('-')
                    && !is_verb(&tokens[idx])
                {
                    let p = tokens[idx].clone();
                    idx += 1;
                    Some(p)
                } else {
                    None
                };
                steps.push(Step::Html { path });
            }
            other if is_url(other) => {
                steps.push(Step::Goto(other.to_string()));
                idx += 1;
            }
            unknown => {
                return Err(format!("unknown command or verb '{unknown}'"));
            }
        }
    }

    Ok(steps)
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("file://")
        || s.starts_with("data:")
        || s.starts_with("about:")
}

fn is_verb(s: &str) -> bool {
    matches!(
        s,
        "goto"
            | "open"
            | "navigate"
            | "viewport"
            | "resize"
            | "wait-for"
            | "waitfor"
            | "wait"
            | "sleep"
            | "click"
            | "type"
            | "fill"
            | "input"
            | "eval"
            | "exec"
            | "js"
            | "screenshot"
            | "--screenshot"
            | "pdf"
            | "--pdf"
            | "html"
            | "--html"
            | "dump"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_example() {
        let args = vec![
            "http://localhost:3000".to_string(),
            "wait-for".to_string(),
            ".my-css-selector".to_string(),
            "--timeout".to_string(),
            "60s".to_string(),
            "--screenshot".to_string(),
        ];

        let steps = parse_tokens(&args, 30_000).expect("parse failed");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0], Step::Goto("http://localhost:3000".to_string()));
        assert_eq!(
            steps[1],
            Step::WaitFor {
                selector: ".my-css-selector".to_string(),
                timeout_ms: 60_000
            }
        );
        assert_eq!(
            steps[2],
            Step::Screenshot {
                path: "screenshot.png".to_string(),
                full_page: false,
                selector: None
            }
        );
    }

    #[test]
    fn parse_script_multiline() {
        let script = r#"
        goto https://example.com
        viewport 1280 800
        click #login-btn
        type #email "test@user.com"
        wait 500ms
        screenshot out.png --full-page
        "#;

        let steps = parse_script(script, 30_000).expect("script parse failed");
        assert_eq!(steps.len(), 6);
        assert_eq!(steps[0], Step::Goto("https://example.com".to_string()));
        assert_eq!(
            steps[1],
            Step::Viewport {
                width: 1280,
                height: 800
            }
        );
        assert_eq!(
            steps[2],
            Step::Click {
                selector: "#login-btn".to_string()
            }
        );
        assert_eq!(
            steps[3],
            Step::Type {
                selector: "#email".to_string(),
                text: "test@user.com".to_string(),
                clear: false
            }
        );
        assert_eq!(steps[4], Step::Wait { duration_ms: 500 });
        assert_eq!(
            steps[5],
            Step::Screenshot {
                path: "out.png".to_string(),
                full_page: true,
                selector: None,
            }
        );
    }

    #[test]
    fn parse_durations() {
        assert_eq!(parse_duration_ms("500ms").unwrap(), 500);
        assert_eq!(parse_duration_ms("2s").unwrap(), 2000);
        assert_eq!(parse_duration_ms("1.5s").unwrap(), 1500);
        assert_eq!(parse_duration_ms("2m").unwrap(), 120_000);
        assert_eq!(parse_duration_ms("10").unwrap(), 10_000);
        assert!(parse_duration_ms("").is_err());
        assert!(parse_duration_ms("abc").is_err());
    }

    #[test]
    fn parse_all_verbs() {
        let args = vec![
            "https://test.local".to_string(),
            "viewport".to_string(),
            "1920x1080".to_string(),
            "eval".to_string(),
            "1 + 1".to_string(),
            "fill".to_string(),
            "#user".to_string(),
            "admin".to_string(),
            "pdf".to_string(),
            "doc.pdf".to_string(),
            "html".to_string(),
            "dump.html".to_string(),
        ];

        let steps = parse_tokens(&args, 30_000).expect("parse failed");
        assert_eq!(steps.len(), 6);
        assert_eq!(steps[0], Step::Goto("https://test.local".to_string()));
        assert_eq!(
            steps[1],
            Step::Viewport {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(
            steps[2],
            Step::Eval {
                expr: "1 + 1".to_string()
            }
        );
        assert_eq!(
            steps[3],
            Step::Type {
                selector: "#user".to_string(),
                text: "admin".to_string(),
                clear: true
            }
        );
        assert_eq!(
            steps[4],
            Step::Pdf {
                path: "doc.pdf".to_string()
            }
        );
        assert_eq!(
            steps[5],
            Step::Html {
                path: Some("dump.html".to_string())
            }
        );
    }

    #[test]
    fn parse_errors() {
        assert!(parse_tokens(&["click".to_string()], 30_000).is_err());
        assert!(parse_tokens(&["wait-for".to_string()], 30_000).is_err());
        assert!(parse_tokens(&["viewport".to_string()], 30_000).is_err());
        assert!(parse_tokens(&["invalid_verb".to_string()], 30_000).is_err());
    }
}
