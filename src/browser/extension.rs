//! Temporary preload extension: hides `navigator.webdriver` and records network traffic.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const PAGE_JS: &str = r#"
(() => {
  try {
    Object.defineProperty(navigator, 'webdriver', { get: () => false, configurable: true });
  } catch (e) {}
  if (!window.__waybotNet) {
    window.__waybotNet = [];
    const push = (rec) => {
      try {
        window.__waybotNet.push(rec);
        if (window.__waybotNet.length > 400) window.__waybotNet.shift();
      } catch (e) {}
    };
    try {
      const of = window.fetch;
      window.fetch = async function (input, init) {
        const url = typeof input === 'string' ? input : (input && input.url) || '';
        const rec = { d: 'fetch', url: String(url), method: (init && init.method) || 'GET', t: Date.now() };
        try { rec.body = init && init.body ? String(init.body).slice(0, 30000) : ''; } catch (e) {}
        try {
          const r = await of(input, init);
          try { rec.status = r.status; rec.res = (await r.clone().text()).slice(0, 60000); } catch (e) {}
          push(rec);
          return r;
        } catch (e) { rec.err = String(e); push(rec); throw e; }
      };
    } catch (e) {}
    try {
      const OX = window.XMLHttpRequest;
      window.XMLHttpRequest = function () {
        const x = new OX();
        const open = x.open, send = x.send;
        let rec = null;
        x.open = function (m, u) { rec = { d: 'xhr', url: String(u), method: m, t: Date.now() }; return open.apply(x, arguments); };
        x.send = function (b) {
          if (rec) {
            try { rec.body = b ? String(b).slice(0, 30000) : ''; } catch (e) {}
            x.addEventListener('loadend', () => { try { rec.status = x.status; rec.res = String(x.responseText || '').slice(0, 60000); } catch (e) {} push(rec); });
          }
          return send.apply(x, arguments);
        };
        return x;
      };
    } catch (e) {}
    try {
      const OW = window.WebSocket;
      window.WebSocket = function (url, protocols) {
        const ws = protocols === undefined ? new OW(url) : new OW(url, protocols);
        const send = ws.send.bind(ws);
        ws.send = function (data) { push({ d: 'ws-out', url: String(url), t: Date.now(), body: String(data).slice(0, 20000) }); return send(data); };
        ws.addEventListener('message', async (ev) => {
          let data = ev.data;
          try {
            if (data instanceof Blob) data = await data.text();
            else if (data instanceof ArrayBuffer) data = new TextDecoder().decode(data);
          } catch (e) {}
          push({ d: 'ws-in', url: String(url), t: Date.now(), res: String(data).slice(0, 60000) });
        });
        return ws;
      };
      window.WebSocket.prototype = OW.prototype;
      window.WebSocket.CONNECTING = 0; window.WebSocket.OPEN = 1; window.WebSocket.CLOSING = 2; window.WebSocket.CLOSED = 3;
    } catch (e) {}
  }
  window.__waybot = { v: 1 };
})();
"#;

const MANIFEST: &str = r#"{
  "manifest_version": 3,
  "name": "waybot preload",
  "version": "1.0",
  "browser_specific_settings": { "gecko": { "id": "preload@waybot.local" } },
  "content_scripts": [
    {
      "matches": ["https://wayground.com/*", "https://*.wayground.com/*"],
      "js": ["page.js"],
      "run_at": "document_start",
      "world": "MAIN",
      "all_frames": true
    }
  ]
}
"#;

pub fn build(dir: &Path) -> io::Result<PathBuf> {
    let root = dir.join("preload");
    fs::create_dir_all(&root)?;
    fs::write(root.join("manifest.json"), MANIFEST)?;
    fs::write(root.join("page.js"), PAGE_JS)?;

    let xpi = dir.join("preload.xpi");
    let file = File::create(&xpi)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for name in ["manifest.json", "page.js"] {
        zip.start_file(name, options)?;
        zip.write_all(fs::read(root.join(name))?.as_slice())?;
    }
    zip.finish()?;
    Ok(xpi)
}

/// Fallback shim executed in the page once content context is available.
pub const RUNTIME_SHIM: &str = PAGE_JS;
