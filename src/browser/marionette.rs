//! Minimal Marionette client (raw TCP, no geckodriver).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
pub enum MnError {
    #[error("koneksi Marionette: {0}")]
    Io(String),
    #[error("Marionette: {0}")]
    Remote(String),
    #[error("protokol Marionette rusak")]
    Protocol,
}

pub struct Marionette {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: u64,
    timeout: Duration,
}

impl Marionette {
    pub fn connect(port: u16, timeout: Duration) -> Result<Self, MnError> {
        let stream =
            TcpStream::connect(("127.0.0.1", port)).map_err(|e| MnError::Io(e.to_string()))?;
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| MnError::Io(e.to_string()))?;
        let writer = stream.try_clone().map_err(|e| MnError::Io(e.to_string()))?;
        let mut client = Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 0,
            timeout,
        };
        let hello = client.recv()?;
        let app = hello
            .get("applicationType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if app != "gecko" {
            return Err(MnError::Protocol);
        }
        Ok(client)
    }

    fn recv(&mut self) -> Result<Value, MnError> {
        let mut header = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = self
                .reader
                .read(&mut byte)
                .map_err(|e| MnError::Io(e.to_string()))?;
            if n == 0 {
                return Err(MnError::Io("koneksi ditutup".into()));
            }
            if byte[0] == b':' {
                break;
            }
            header.push(byte[0]);
            if header.len() > 10 {
                return Err(MnError::Protocol);
            }
        }
        let len: usize = std::str::from_utf8(&header)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(MnError::Protocol)?;
        let mut body = vec![0u8; len];
        self.reader
            .read_exact(&mut body)
            .map_err(|e| MnError::Io(e.to_string()))?;
        serde_json::from_slice(&body).map_err(|_| MnError::Protocol)
    }

    pub fn command(&mut self, name: &str, params: Value) -> Result<Value, MnError> {
        self.next_id += 1;
        let id = self.next_id;
        let payload =
            serde_json::to_vec(&json!([0, id, name, params])).map_err(|_| MnError::Protocol)?;
        let mut frame = format!("{}:", payload.len()).into_bytes();
        frame.extend_from_slice(&payload);
        self.writer
            .write_all(&frame)
            .map_err(|e| MnError::Io(e.to_string()))?;
        self.writer.flush().ok();
        self.writer
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| MnError::Io(e.to_string()))?;
        let response = self.recv()?;
        let error = response.get(2).cloned().unwrap_or(Value::Null);
        if !error.is_null() {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string());
            return Err(MnError::Remote(message));
        }
        Ok(response.get(3).cloned().unwrap_or(Value::Null))
    }

    pub fn new_session(&mut self) -> Result<Value, MnError> {
        self.command(
            "WebDriver:NewSession",
            json!({"capabilities": {"alwaysMatch": {"browserName": "firefox"}}}),
        )
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub fn set_context_chrome(&mut self) -> Result<(), MnError> {
        self.command("Marionette:SetContext", json!({"value": "chrome"}))?;
        Ok(())
    }

    pub fn set_context_content(&mut self) -> Result<(), MnError> {
        self.command("Marionette:SetContext", json!({"value": "content"}))?;
        Ok(())
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), MnError> {
        self.command(
            "WebDriver:Navigate",
            json!({"url": url, "pageLoadStrategy": "eager"}),
        )?;
        Ok(())
    }

    pub fn script(&mut self, script: &str, args: Value) -> Result<Value, MnError> {
        let out = self.command(
            "WebDriver:ExecuteScript",
            json!({"script": script, "args": args, "scriptTimeout": 30_000}),
        )?;
        Ok(out.get("value").cloned().unwrap_or(Value::Null))
    }

    pub fn async_script(&mut self, script: &str, args: Value) -> Result<Value, MnError> {
        let out = self.command(
            "WebDriver:ExecuteAsyncScript",
            json!({"script": script, "args": args, "scriptTimeout": 45_000}),
        )?;
        Ok(out.get("value").cloned().unwrap_or(Value::Null))
    }

    pub fn find(&mut self, css: &str) -> Result<String, MnError> {
        let value = self.command(
            "WebDriver:FindElement",
            json!({"using": "css selector", "value": css}),
        )?;
        value
            .get("value")
            .and_then(|v| v.get("element-6066-11e4-a52e-4f735466cecf"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or(MnError::Protocol)
    }

    pub fn click(&mut self, element_id: &str) -> Result<(), MnError> {
        self.command("WebDriver:ElementClick", json!({"id": element_id}))?;
        Ok(())
    }

    pub fn clear(&mut self, element_id: &str) -> Result<(), MnError> {
        self.command("WebDriver:ElementClear", json!({"id": element_id}))?;
        Ok(())
    }

    pub fn send_keys(&mut self, element_id: &str, text: &str) -> Result<(), MnError> {
        self.command(
            "WebDriver:ElementSendKeys",
            json!({"id": element_id, "text": text}),
        )?;
        Ok(())
    }

    pub fn new_tab(&mut self) -> Result<String, MnError> {
        let value = self.command("WebDriver:NewWindow", json!({"type": "tab"}))?;
        value
            .get("handle")
            .and_then(|h| h.as_str())
            .map(str::to_string)
            .ok_or(MnError::Protocol)
    }

    pub fn switch_window(&mut self, handle: &str) -> Result<(), MnError> {
        self.command("WebDriver:SwitchToWindow", json!({"handle": handle}))?;
        Ok(())
    }

    pub fn install_addon(&mut self, path: &str) -> Result<Value, MnError> {
        self.command("Addon:Install", json!({"path": path, "temporary": true}))
    }

    pub fn screenshot(&mut self) -> Result<Vec<u8>, MnError> {
        use base64::Engine;
        let value = self.command("WebDriver:TakeScreenshot", json!({}))?;
        let b64 = value
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|_| MnError::Protocol)
    }

    #[allow(dead_code)]
    pub fn quit(&mut self) -> Result<(), MnError> {
        self.command("WebDriver:DeleteSession", json!({}))?;
        Ok(())
    }

    /// Read the Marionette handshake without opening a session (readiness probe).
    #[allow(dead_code)]
    pub fn probe(port: u16) -> bool {
        TcpStream::connect(("127.0.0.1", port)).is_ok()
    }

    #[allow(dead_code)]
    pub fn read_line(stream: &mut TcpStream) -> Option<String> {
        let mut buf = Vec::new();
        let mut reader = BufReader::new(stream);
        reader.read_until(b':', &mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    }
}
