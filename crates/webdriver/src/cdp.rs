//! Zero-dependency Chrome DevTools Protocol (CDP) WebSocket client.

use crate::base64;
use crate::json::{self, JsonValue};
use crate::locator;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

pub struct WebSocketClient {
    stream: TcpStream,
}

impl WebSocketClient {
    pub fn connect(host: &str, port: u16, path: &str) -> Result<Self, String> {
        let stream = TcpStream::connect((host, port))
            .map_err(|e| format!("failed to connect to CDP at {host}:{port}: {e}"))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(60)))
            .map_err(|e| format!("failed to set read timeout: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(60)))
            .map_err(|e| format!("failed to set write timeout: {e}"))?;

        let mut client = Self { stream };
        client.handshake(host, port, path)?;
        Ok(client)
    }

    fn handshake(&mut self, host: &str, port: u16, path: &str) -> Result<(), String> {
        let request = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n"
        );

        self.stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("failed to send websocket handshake: {e}"))?;
        self.stream
            .flush()
            .map_err(|e| format!("failed to flush handshake: {e}"))?;

        // Read headers until \r\n\r\n
        let mut header_buf = Vec::new();
        let mut byte = [0u8; 1];

        while header_buf.len() < 8192 {
            self.stream
                .read_exact(&mut byte)
                .map_err(|e| format!("failed to read handshake response: {e}"))?;
            header_buf.push(byte[0]);

            if header_buf.ends_with(b"\r\n\r\n") {
                break;
            }
        }

        let resp_str = String::from_utf8_lossy(&header_buf);
        if !resp_str.starts_with("HTTP/1.1 101") && !resp_str.starts_with("HTTP/1.0 101") {
            return Err(format!("invalid websocket handshake response: {resp_str}"));
        }

        Ok(())
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), String> {
        let payload = text.as_bytes();
        let mut frame = Vec::with_capacity(payload.len() + 14);

        // Byte 0: FIN (0x80) | Text Opcode (0x01)
        frame.push(0x81);

        // Mask bit is required from client to server (0x80)
        let len = payload.len();
        if len <= 125 {
            frame.push(0x80 | (len as u8));
        } else if len <= 65535 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }

        // 4-byte mask key
        let mask = [0x12, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask);

        // Masked payload
        for (i, &b) in payload.iter().enumerate() {
            frame.push(b ^ mask[i % 4]);
        }

        self.stream
            .write_all(&frame)
            .map_err(|e| format!("failed to send websocket frame: {e}"))?;
        self.stream
            .flush()
            .map_err(|e| format!("failed to flush websocket frame: {e}"))?;

        Ok(())
    }

    pub fn read_text(&mut self) -> Result<String, String> {
        let mut full_payload = Vec::new();

        loop {
            let mut header = [0u8; 2];
            self.stream
                .read_exact(&mut header)
                .map_err(|e| format!("failed to read websocket frame header: {e}"))?;

            let fin = (header[0] & 0x80) != 0;
            let opcode = header[0] & 0x0f;
            let masked = (header[1] & 0x80) != 0;
            let len_code = header[1] & 0x7f;

            let len: usize = if len_code <= 125 {
                len_code as usize
            } else if len_code == 126 {
                let mut b = [0u8; 2];
                self.stream
                    .read_exact(&mut b)
                    .map_err(|e| format!("failed to read 16-bit length: {e}"))?;
                u16::from_be_bytes(b) as usize
            } else {
                let mut b = [0u8; 8];
                self.stream
                    .read_exact(&mut b)
                    .map_err(|e| format!("failed to read 64-bit length: {e}"))?;
                u64::from_be_bytes(b) as usize
            };

            let mask_key = if masked {
                let mut m = [0u8; 4];
                self.stream
                    .read_exact(&mut m)
                    .map_err(|e| format!("failed to read mask key: {e}"))?;
                Some(m)
            } else {
                None
            };

            let mut payload = vec![0u8; len];
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("failed to read frame payload of {len} bytes: {e}"))?;

            if let Some(mask) = mask_key {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
            }

            match opcode {
                // Continuation frame
                0x0 => {
                    full_payload.extend_from_slice(&payload);
                }
                // Text frame
                0x1 => {
                    full_payload.extend_from_slice(&payload);
                }
                // Close frame
                0x8 => {
                    return Err("websocket connection closed by peer".to_string());
                }
                // Ping frame -> send Pong
                0x9 => {
                    let mut pong = Vec::with_capacity(payload.len() + 6);
                    pong.push(0x8A); // FIN | Pong
                    pong.push(0x80 | (payload.len() as u8).min(125));
                    let pmask = [0x00, 0x00, 0x00, 0x00];
                    pong.extend_from_slice(&pmask);
                    pong.extend_from_slice(&payload);
                    let _ = self.stream.write_all(&pong);
                    let _ = self.stream.flush();
                    continue;
                }
                // Pong frame -> ignore
                0xA => continue,
                other => {
                    return Err(format!("unsupported websocket opcode 0x{other:02X}"));
                }
            }

            if fin {
                break;
            }
        }

        String::from_utf8(full_payload).map_err(|e| format!("invalid UTF-8 in message: {e}"))
    }
}

/// A console message or uncaught exception captured from the page.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
}

impl ConsoleEntry {
    pub fn is_error(&self) -> bool {
        matches!(self.level.as_str(), "error" | "warning" | "exception")
    }
}

pub struct CdpSession {
    ws: WebSocketClient,
    next_id: u64,
    console: Vec<ConsoleEntry>,
}

impl CdpSession {
    pub fn new(ws: WebSocketClient) -> Self {
        Self {
            ws,
            next_id: 1,
            console: Vec::new(),
        }
    }

    /// Console entries captured so far, oldest first.
    pub fn console_entries(&self) -> &[ConsoleEntry] {
        &self.console
    }

    pub fn clear_console(&mut self) {
        self.console.clear();
    }

    /// Pump the socket so events emitted since the last command land in the
    /// buffer. CDP delivers events before the reply to a later command, so a
    /// trivial round-trip is enough to drain them.
    pub fn drain_events(&mut self) -> Result<(), String> {
        self.evaluate("0")?;
        Ok(())
    }

    fn record_event(&mut self, val: &JsonValue) {
        let Some(method) = val.get("method").and_then(|m| m.as_str()) else {
            return;
        };
        let params = val.get("params");

        let entry = match method {
            "Runtime.consoleAPICalled" => {
                let level = params
                    .and_then(|p| p.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("log")
                    .to_string();
                let text = params
                    .and_then(|p| p.get("args"))
                    .and_then(|a| a.as_array())
                    .map(|args| {
                        args.iter()
                            .map(describe_remote_object)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                Some(ConsoleEntry { level, text })
            }
            "Runtime.exceptionThrown" => {
                let details = params.and_then(|p| p.get("exceptionDetails"));
                let text = details
                    .and_then(|d| d.get("exception"))
                    .map(describe_remote_object)
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        details
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| "uncaught exception".to_string());
                Some(ConsoleEntry {
                    level: "exception".to_string(),
                    text,
                })
            }
            "Log.entryAdded" => {
                let entry = params.and_then(|p| p.get("entry"));
                let level = entry
                    .and_then(|e| e.get("level"))
                    .and_then(|l| l.as_str())
                    .unwrap_or("info")
                    .to_string();
                let text = entry
                    .and_then(|e| e.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(ConsoleEntry { level, text })
            }
            _ => None,
        };

        if let Some(entry) = entry {
            if !entry.text.is_empty() {
                self.console.push(entry);
            }
        }
    }

    pub fn call(&mut self, method: &str, params: JsonValue) -> Result<JsonValue, String> {
        let req_id = self.next_id;
        self.next_id += 1;

        let mut map = HashMap::new();
        map.insert("id".to_string(), JsonValue::Number(req_id as f64));
        map.insert("method".to_string(), JsonValue::String(method.to_string()));
        map.insert("params".to_string(), params);

        let req_json = JsonValue::Object(map).to_json_string();
        self.ws.send_text(&req_json)?;

        // Wait for response matching req_id
        let start = Instant::now();
        let timeout = Duration::from_secs(60);

        loop {
            if start.elapsed() > timeout {
                return Err(format!("timeout waiting for CDP response to '{method}'"));
            }

            let msg = self.ws.read_text()?;
            let val = json::parse(&msg)?;
            self.record_event(&val);

            if let Some(id_val) = val.get("id") {
                if id_val.as_i64() == Some(req_id as i64) {
                    if let Some(err) = val.get("error") {
                        let msg = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown CDP error");
                        return Err(format!("CDP {method} error: {msg}"));
                    }
                    if let Some(result) = val.get("result") {
                        return Ok(result.clone());
                    }
                    return Ok(JsonValue::Null);
                }
            }
        }
    }

    pub fn enable_domains(&mut self) -> Result<(), String> {
        self.call("Page.enable", JsonValue::Object(HashMap::new()))?;
        self.call("Runtime.enable", JsonValue::Object(HashMap::new()))?;
        self.call("DOM.enable", JsonValue::Object(HashMap::new()))?;
        // Log domain surfaces network/security failures the page never logs itself.
        let _ = self.call("Log.enable", JsonValue::Object(HashMap::new()));
        Ok(())
    }

    pub fn navigate(&mut self, url: &str, timeout_ms: u64) -> Result<(), String> {
        let mut params = HashMap::new();
        params.insert("url".to_string(), JsonValue::String(url.to_string()));
        self.call("Page.navigate", JsonValue::Object(params))?;

        self.await_ready(timeout_ms);
        Ok(())
    }

    /// Poll `document.readyState` until the document is usable. Returns whether
    /// it got there; callers proceed regardless, matching prior behaviour.
    fn await_ready(&mut self, timeout_ms: u64) -> bool {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        while start.elapsed() < timeout {
            if let Ok(val) = self.evaluate("document.readyState") {
                if matches!(val.as_str(), Some("interactive") | Some("complete")) {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        false
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) -> Result<(), String> {
        let mut params = HashMap::new();
        params.insert("width".to_string(), JsonValue::Number(width as f64));
        params.insert("height".to_string(), JsonValue::Number(height as f64));
        params.insert("deviceScaleFactor".to_string(), JsonValue::Number(1.0));
        params.insert("mobile".to_string(), JsonValue::Bool(false));

        self.call(
            "Emulation.setDeviceMetricsOverride",
            JsonValue::Object(params),
        )?;
        Ok(())
    }

    pub fn evaluate(&mut self, expression: &str) -> Result<JsonValue, String> {
        let mut params = HashMap::new();
        params.insert(
            "expression".to_string(),
            JsonValue::String(expression.to_string()),
        );
        params.insert("returnByValue".to_string(), JsonValue::Bool(true));
        params.insert("awaitPromise".to_string(), JsonValue::Bool(true));

        let resp = self.call("Runtime.evaluate", JsonValue::Object(params))?;

        if let Some(ex) = resp.get("exceptionDetails") {
            let desc = ex
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("JS evaluation threw an exception");
            return Err(format!("JS error: {desc}"));
        }

        if let Some(res) = resp.get("result") {
            if let Some(val) = res.get("value") {
                return Ok(val.clone());
            }
        }

        Ok(JsonValue::Null)
    }

    /// Wait until `spec` resolves to an element that is visible *and* painted.
    /// Two animation frames after the layout box appears rule out an `ok` that
    /// really means "matched unpainted SSR markup".
    pub fn wait_for(&mut self, spec: &str, timeout_ms: u64) -> Result<(), String> {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let check_expr = locator::with_prelude(&format!(
            "(() => {{ const el = window.__wd.find(\"{}\"); return !!el && window.__wd.visible(el); }})()",
            json::escape_str(spec)
        ));

        while start.elapsed() < timeout {
            if let Ok(JsonValue::Bool(true)) = self.evaluate(&check_expr) {
                self.evaluate(
                    "new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))",
                )?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "timed out waiting for '{spec}' after {timeout_ms}ms"
        ))
    }

    /// Wait until the current URL contains `substring`.
    pub fn wait_for_url(&mut self, substring: &str, timeout_ms: u64) -> Result<String, String> {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        while start.elapsed() < timeout {
            if let Ok(url) = self.current_url() {
                if url.contains(substring) {
                    return Ok(url);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "timed out waiting for URL containing '{substring}' after {timeout_ms}ms"
        ))
    }

    /// Wait until the DOM stops mutating for `quiet_ms`. Generic "SPA settled"
    /// signal, preferable to guessing with a fixed sleep.
    pub fn wait_for_quiet(&mut self, quiet_ms: u64, timeout_ms: u64) -> Result<(), String> {
        let script = format!(
            r#"new Promise((resolve) => {{
                let last = Date.now();
                const deadline = Date.now() + {timeout_ms};
                const obs = new MutationObserver(() => {{ last = Date.now(); }});
                obs.observe(document.documentElement, {{
                    subtree: true, childList: true, attributes: true, characterData: true
                }});
                const iv = setInterval(() => {{
                    const settled = Date.now() - last >= {quiet_ms};
                    if (settled || Date.now() > deadline) {{
                        clearInterval(iv);
                        obs.disconnect();
                        resolve(settled);
                    }}
                }}, 50);
            }})"#
        );

        match self.evaluate(&script)? {
            JsonValue::Bool(true) => Ok(()),
            _ => Err(format!(
                "DOM never stayed quiet for {quiet_ms}ms within {timeout_ms}ms"
            )),
        }
    }

    pub fn reload(&mut self, timeout_ms: u64) -> Result<String, String> {
        self.call("Page.reload", JsonValue::Object(HashMap::new()))?;
        self.await_ready(timeout_ms);
        self.current_url()
    }

    pub fn current_url(&mut self) -> Result<String, String> {
        Ok(self
            .evaluate("location.href")?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    pub fn title(&mut self) -> Result<String, String> {
        Ok(self
            .evaluate("document.title")?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// Move the real mouse over the element so CSS `:hover` actually applies —
    /// JS-dispatched mouse events do not trigger it.
    pub fn hover(&mut self, spec: &str) -> Result<(), String> {
        let center = self.evaluate(&locator::with_prelude(&format!(
            r#"(() => {{
                const el = window.__wd.require("{}");
                el.scrollIntoView({{ block: "center", inline: "center" }});
                const r = el.getBoundingClientRect();
                return {{ x: r.left + r.width / 2, y: r.top + r.height / 2 }};
            }})()"#,
            json::escape_str(spec)
        )))?;

        let x = center.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = center.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let mut params = HashMap::new();
        params.insert(
            "type".to_string(),
            JsonValue::String("mouseMoved".to_string()),
        );
        params.insert("x".to_string(), JsonValue::Number(x));
        params.insert("y".to_string(), JsonValue::Number(y));
        self.call("Input.dispatchMouseEvent", JsonValue::Object(params))?;
        Ok(())
    }

    /// Send a key chord such as `Enter`, `Escape` or `Meta+O` to the focused element.
    pub fn press(&mut self, chord: &str) -> Result<(), String> {
        let mut modifiers = 0f64;
        let mut key_part = chord;

        for part in chord.split('+') {
            match part.to_ascii_lowercase().as_str() {
                "alt" | "option" => modifiers += 1.0,
                "ctrl" | "control" => modifiers += 2.0,
                "meta" | "cmd" | "command" => modifiers += 4.0,
                "shift" => modifiers += 8.0,
                _ => key_part = part,
            }
        }

        let (key, code, vk, text) = key_descriptor(key_part)?;

        for event in ["keyDown", "keyUp"] {
            let mut params = HashMap::new();
            params.insert("type".to_string(), JsonValue::String(event.to_string()));
            params.insert("key".to_string(), JsonValue::String(key.clone()));
            params.insert("code".to_string(), JsonValue::String(code.clone()));
            params.insert("windowsVirtualKeyCode".to_string(), JsonValue::Number(vk));
            params.insert("nativeVirtualKeyCode".to_string(), JsonValue::Number(vk));
            params.insert("modifiers".to_string(), JsonValue::Number(modifiers));
            if event == "keyDown" {
                if let Some(ref t) = text {
                    params.insert("text".to_string(), JsonValue::String(t.clone()));
                }
            }
            self.call("Input.dispatchKeyEvent", JsonValue::Object(params))?;
        }

        Ok(())
    }

    pub fn click(&mut self, spec: &str) -> Result<(), String> {
        let script = locator::with_prelude(&format!(
            r#"(() => {{
                const el = window.__wd.require("{}");
                el.scrollIntoView({{ block: "center", inline: "center" }});
                el.click();
                return true;
            }})()"#,
            json::escape_str(spec)
        ));

        self.evaluate(&script)?;
        Ok(())
    }

    /// Set a form field's value. Writes through the *native* value setter so a
    /// React controlled input actually fires `onChange` — assigning `el.value`
    /// directly is swallowed by React's value tracker.
    pub fn type_text(&mut self, spec: &str, text: &str, clear: bool) -> Result<(), String> {
        let script = locator::with_prelude(&format!(
            r#"(() => {{
                const el = window.__wd.require("{}");
                el.focus();
                const next = ({} ? "" : (el.value || "")) + "{}";
                const proto = el instanceof HTMLTextAreaElement
                    ? HTMLTextAreaElement.prototype
                    : HTMLInputElement.prototype;
                const setter = Object.getOwnPropertyDescriptor(proto, "value");
                if (setter && setter.set) {{ setter.set.call(el, next); }} else {{ el.value = next; }}
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return true;
            }})()"#,
            json::escape_str(spec),
            if clear { "true" } else { "false" },
            json::escape_str(text)
        ));

        self.evaluate(&script)?;
        Ok(())
    }

    pub fn capture_screenshot(
        &mut self,
        full_page: bool,
        selector: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let mut params = HashMap::new();
        params.insert("format".to_string(), JsonValue::String("png".to_string()));

        if let Some(sel) = selector {
            let clip_script = locator::with_prelude(&format!(
                r#"(() => {{
                    const el = window.__wd.require("{}");
                    el.scrollIntoView({{ block: "center", inline: "center" }});
                    const r = el.getBoundingClientRect();
                    return {{ x: r.left, y: r.top, width: r.width, height: r.height }};
                }})()"#,
                json::escape_str(sel)
            ));

            let clip_val = self.evaluate(&clip_script)?;
            let x = clip_val.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y = clip_val.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let w = clip_val
                .get("width")
                .and_then(|v| v.as_f64())
                .unwrap_or(100.0);
            let h = clip_val
                .get("height")
                .and_then(|v| v.as_f64())
                .unwrap_or(100.0);

            let mut clip = HashMap::new();
            clip.insert("x".to_string(), JsonValue::Number(x));
            clip.insert("y".to_string(), JsonValue::Number(y));
            clip.insert("width".to_string(), JsonValue::Number(w));
            clip.insert("height".to_string(), JsonValue::Number(h));
            clip.insert("scale".to_string(), JsonValue::Number(1.0));

            params.insert("clip".to_string(), JsonValue::Object(clip));
        } else if full_page {
            params.insert("captureBeyondViewport".to_string(), JsonValue::Bool(true));
        }

        let resp = self.call("Page.captureScreenshot", JsonValue::Object(params))?;
        let data_b64 = resp
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| "missing 'data' in screenshot response".to_string())?;

        base64::decode(data_b64).map_err(|e| format!("failed to decode screenshot base64: {e}"))
    }

    pub fn print_to_pdf(&mut self) -> Result<Vec<u8>, String> {
        let mut params = HashMap::new();
        params.insert("printBackground".to_string(), JsonValue::Bool(true));

        let resp = self.call("Page.printToPDF", JsonValue::Object(params))?;
        let data_b64 = resp
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| "missing 'data' in PDF response".to_string())?;

        base64::decode(data_b64).map_err(|e| format!("failed to decode PDF base64: {e}"))
    }

    pub fn get_html(&mut self) -> Result<String, String> {
        let val = self.evaluate("document.documentElement.outerHTML")?;
        val.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "failed to retrieve HTML string".to_string())
    }

    pub fn close_browser(&mut self) -> Result<(), String> {
        let _ = self.call("Browser.close", JsonValue::Object(HashMap::new()));
        Ok(())
    }
}

/// Best-effort human rendering of a CDP `RemoteObject` for the console buffer.
fn describe_remote_object(obj: &JsonValue) -> String {
    if let Some(desc) = obj.get("description").and_then(|d| d.as_str()) {
        return desc.to_string();
    }
    match obj.get("value") {
        Some(JsonValue::String(s)) => s.clone(),
        Some(other) => other.to_json_string(),
        None => obj
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// Map a key name to the `(key, code, virtualKeyCode, text)` CDP needs.
fn key_descriptor(name: &str) -> Result<(String, String, f64, Option<String>), String> {
    let named = match name {
        "Enter" | "enter" | "Return" => Some(("Enter", "Enter", 13.0, Some("\r"))),
        "Tab" | "tab" => Some(("Tab", "Tab", 9.0, Some("\t"))),
        "Escape" | "escape" | "Esc" | "esc" => Some(("Escape", "Escape", 27.0, None)),
        "Backspace" | "backspace" => Some(("Backspace", "Backspace", 8.0, None)),
        "Delete" | "delete" => Some(("Delete", "Delete", 46.0, None)),
        "Space" | "space" => Some((" ", "Space", 32.0, Some(" "))),
        "ArrowUp" | "Up" => Some(("ArrowUp", "ArrowUp", 38.0, None)),
        "ArrowDown" | "Down" => Some(("ArrowDown", "ArrowDown", 40.0, None)),
        "ArrowLeft" | "Left" => Some(("ArrowLeft", "ArrowLeft", 37.0, None)),
        "ArrowRight" | "Right" => Some(("ArrowRight", "ArrowRight", 39.0, None)),
        "Home" | "home" => Some(("Home", "Home", 36.0, None)),
        "End" | "end" => Some(("End", "End", 35.0, None)),
        "PageUp" => Some(("PageUp", "PageUp", 33.0, None)),
        "PageDown" => Some(("PageDown", "PageDown", 34.0, None)),
        _ => None,
    };

    if let Some((key, code, vk, text)) = named {
        return Ok((
            key.to_string(),
            code.to_string(),
            vk,
            text.map(|t| t.to_string()),
        ));
    }

    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => {
            let upper = ch.to_ascii_uppercase();
            let code = if ch.is_ascii_alphabetic() {
                format!("Key{upper}")
            } else if ch.is_ascii_digit() {
                format!("Digit{ch}")
            } else {
                String::new()
            };
            Ok((
                ch.to_string(),
                code,
                upper as u32 as f64,
                Some(ch.to_string()),
            ))
        }
        _ => Err(format!("unknown key '{name}'")),
    }
}
