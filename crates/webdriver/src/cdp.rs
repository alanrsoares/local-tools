//! Zero-dependency Chrome DevTools Protocol (CDP) WebSocket client.

use crate::base64;
use crate::json::{self, JsonValue};
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

pub struct CdpSession {
    ws: WebSocketClient,
    next_id: u64,
}

impl CdpSession {
    pub fn new(ws: WebSocketClient) -> Self {
        Self { ws, next_id: 1 }
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
        Ok(())
    }

    pub fn navigate(&mut self, url: &str, timeout_ms: u64) -> Result<(), String> {
        let mut params = HashMap::new();
        params.insert("url".to_string(), JsonValue::String(url.to_string()));
        self.call("Page.navigate", JsonValue::Object(params))?;

        // Wait for page readyState
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);

        while start.elapsed() < timeout {
            let res = self.evaluate("document.readyState");
            if let Ok(val) = res {
                if let Some(state) = val.as_str() {
                    if state == "interactive" || state == "complete" {
                        return Ok(());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Ok(()) // Proceed even if readyState check timed out
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

    pub fn wait_for(&mut self, selector: &str, timeout_ms: u64) -> Result<(), String> {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let check_expr = format!(
            "document.querySelector(\"{}\") !== null",
            json::escape_str(selector)
        );

        while start.elapsed() < timeout {
            let res = self.evaluate(&check_expr);
            if let Ok(JsonValue::Bool(true)) = res {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Err(format!(
            "timed out waiting for selector '{selector}' after {timeout_ms}ms"
        ))
    }

    pub fn click(&mut self, selector: &str) -> Result<(), String> {
        let script = format!(
            r#"(() => {{
                const el = document.querySelector("{}");
                if (!el) throw new Error("element not found: {}");
                el.scrollIntoView({{ block: "center", inline: "center" }});
                el.click();
                return true;
            }})()"#,
            json::escape_str(selector),
            json::escape_str(selector)
        );

        self.evaluate(&script)?;
        Ok(())
    }

    pub fn type_text(&mut self, selector: &str, text: &str, clear: bool) -> Result<(), String> {
        let script = format!(
            r#"(() => {{
                const el = document.querySelector("{}");
                if (!el) throw new Error("element not found: {}");
                el.focus();
                {}
                el.value += "{}";
                el.dispatchEvent(new Event("input", {{ bubbles: true }}));
                el.dispatchEvent(new Event("change", {{ bubbles: true }}));
                return true;
            }})()"#,
            json::escape_str(selector),
            json::escape_str(selector),
            if clear { "el.value = '';" } else { "" },
            json::escape_str(text)
        );

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
            let clip_script = format!(
                r#"(() => {{
                    const el = document.querySelector("{}");
                    if (!el) throw new Error("element not found: {}");
                    const r = el.getBoundingClientRect();
                    return {{ x: r.left, y: r.top, width: r.width, height: r.height }};
                }})()"#,
                json::escape_str(sel),
                json::escape_str(sel)
            );

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
