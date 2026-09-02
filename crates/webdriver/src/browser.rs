//! Browser process management and DevTools endpoint discovery.

use crate::cdp::{CdpSession, WebSocketClient};
use crate::json;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub struct BrowserInstance {
    child: Child,
    temp_dir: PathBuf,
    session: Option<CdpSession>,
}

impl BrowserInstance {
    pub fn launch(
        custom_browser: Option<&str>,
        headless: bool,
    ) -> Result<(Self, CdpSession), String> {
        let browser_path = find_browser(custom_browser)?;

        let temp_dir = env::temp_dir().join(format!(
            "local-tools-webdriver-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("failed to create temp profile dir: {e}"))?;

        let mut cmd = Command::new(&browser_path);

        if headless {
            cmd.arg("--headless=new");
        }

        cmd.arg("--remote-debugging-port=0")
            .arg("--disable-gpu")
            .arg("--hide-scrollbars")
            .arg("--mute-audio")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg(format!("--user-data-dir={}", temp_dir.display()))
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "failed to launch browser at {}: {e}",
                browser_path.display()
            )
        })?;

        let stderr = child
            .stderr
            .take()
            .ok_or("failed to capture browser stderr")?;
        let (tx, rx) = mpsc::channel();

        // Read stderr in background to capture DevTools listening port
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if let Some(pos) = line.find("DevTools listening on ws://") {
                    let rest = &line[pos + "DevTools listening on ws://".len()..];
                    let _ = tx.send(rest.to_string());
                    break;
                }
            }
        });

        let ws_endpoint = rx
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "timed out waiting for browser DevTools endpoint".to_string())?;

        // Format is e.g. "127.0.0.1:54321/devtools/browser/uuid"
        let parts: Vec<&str> = ws_endpoint.splitn(2, '/').collect();
        let addr = parts[0];
        let port: u16 = addr
            .split(':')
            .nth(1)
            .and_then(|p| p.parse().ok())
            .ok_or_else(|| format!("invalid DevTools address '{addr}'"))?;

        // Create fresh page target via HTTP PUT /json/new or /json/list
        let page_ws_path = create_new_tab(port)?;

        let ws_client = WebSocketClient::connect("127.0.0.1", port, &page_ws_path)?;
        let mut session = CdpSession::new(ws_client);
        session.enable_domains()?;

        let instance = Self {
            child,
            temp_dir,
            session: None,
        };

        Ok((instance, session))
    }
}

impl Drop for BrowserInstance {
    fn drop(&mut self) {
        if let Some(mut session) = self.session.take() {
            let _ = session.close_browser();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn http_request(port: u16, method: &str, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("failed to connect to 127.0.0.1:{port}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;

    let req = format!(
        "{method} {path} HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Content-Length: 0\r\n\
         Connection: close\r\n\
         \r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    let mut content_length: Option<usize> = None;

    // Read header
    while buf.len() < 8192 {
        if stream.read_exact(&mut byte).is_err() {
            break;
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            let header_str = String::from_utf8_lossy(&buf);
            for line in header_str.lines() {
                if let Some(val) = line.to_lowercase().strip_prefix("content-length:") {
                    if let Ok(len) = val.trim().parse::<usize>() {
                        content_length = Some(len);
                    }
                }
            }
            break;
        }
    }

    // Read body
    let mut body = Vec::new();
    if let Some(len) = content_length {
        body.resize(len, 0);
        stream
            .read_exact(&mut body)
            .map_err(|e| format!("failed to read HTTP body of {len} bytes: {e}"))?;
    } else {
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
        body.extend_from_slice(&rest);
    }

    String::from_utf8(body).map_err(|e| format!("invalid UTF-8 in response body: {e}"))
}

fn create_new_tab(port: u16) -> Result<String, String> {
    let start = Instant::now();
    let timeout = Duration::from_secs(5);

    while start.elapsed() < timeout {
        if let Ok(body) = http_request(port, "PUT", "/json/new") {
            if let Ok(val) = json::parse(&body) {
                if let Some(ws_url) = val.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                    if let Some(pos) = ws_url.find("/devtools/page/") {
                        return Ok(ws_url[pos..].to_string());
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    // Fallback: query /json/list
    if let Ok(body) = http_request(port, "GET", "/json/list") {
        if let Ok(val) = json::parse(&body) {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    if item.get("type").and_then(|t| t.as_str()) == Some("page") {
                        if let Some(ws_url) =
                            item.get("webSocketDebuggerUrl").and_then(|v| v.as_str())
                        {
                            if let Some(pos) = ws_url.find("/devtools/page/") {
                                return Ok(ws_url[pos..].to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Err("failed to obtain page target WebSocket URL from browser".to_string())
}

pub fn find_browser(custom: Option<&str>) -> Result<PathBuf, String> {
    if let Some(c) = custom {
        let p = PathBuf::from(c);
        if p.exists() {
            return Ok(p);
        }
        return Err(format!("specified browser binary does not exist: {c}"));
    }

    if let Ok(env_bin) = env::var("CHROME_BIN").or_else(|_| env::var("BROWSER_BIN")) {
        let p = PathBuf::from(env_bin);
        if p.exists() {
            return Ok(p);
        }
    }

    let candidates = [
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/usr/bin/chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/brave-browser",
    ];

    for path in candidates {
        let p = Path::new(path);
        if p.exists() {
            return Ok(p.to_path_buf());
        }
    }

    // Check PATH
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            for bin_name in &["chromium", "google-chrome", "brave", "msedge"] {
                let full = dir.join(bin_name);
                if full.is_file() {
                    return Ok(full);
                }
            }
        }
    }

    Err(
        "no Chromium-based browser found. Install Chromium, Google Chrome, or Brave, or set CHROME_BIN."
            .to_string(),
    )
}
