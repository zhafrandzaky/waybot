import json, socket, time, sys

import os
PORT = int(os.environ.get("MN_PORT", "2828"))

class MN:
    def __init__(self):
        self.s = socket.create_connection(("127.0.0.1", PORT), 10)
        self.s.settimeout(120)
        self.hello = self._recv()
        self.id = 0
        self.session = None

    def _recv(self):
        h = b""
        while not h.endswith(b":"):
            c = self.s.recv(1)
            if not c:
                raise EOFError
            h += c
        n = int(h[:-1]); d = b""
        while len(d) < n:
            d += self.s.recv(n - len(d))
        return json.loads(d)

    def cmd(self, name, params=None):
        self.id += 1
        payload = json.dumps([0, self.id, name, params or {}]).encode()
        self.s.sendall(str(len(payload)).encode() + b":" + payload)
        r = self._recv()
        if r[2]:
            raise RuntimeError(f"{name}: {r[2]}")
        return r[3]

    def new_session(self):
        r = self.cmd("WebDriver:NewSession", {"capabilities": {"alwaysMatch": {"browserName": "firefox"}}})
        self.session = r["sessionId"]
        return r

    def nav(self, url):
        return self.cmd("WebDriver:Navigate", {"url": url})

    def js(self, script, args=None):
        return self.cmd("WebDriver:ExecuteScript", {"script": script, "args": args or []})

    def find(self, css):
        return self.cmd("WebDriver:FindElement", {"using": "css selector", "value": css})["value"]["element-6066-11e4-a52e-4f735466cecf"]

    def click(self, css):
        el = self.find(css)
        return self.cmd("WebDriver:ElementClick", {"id": el})

    def fill(self, css, text):
        el = self.find(css)
        self.cmd("WebDriver:ElementClear", {"id": el})
        self.cmd("WebDriver:ElementSendKeys", {"id": el, "text": text})

    def close(self):
        try: self.s.close()
        except Exception: pass


WS_SHIM = r"""
(() => {
  if (window.__wslog_installed) return;
  window.__wslog_installed = true;
  window.__wslog = [];
  const OW = window.WebSocket;
  function W(url, protocols) {
    const ws = protocols === undefined ? new OW(url) : new OW(url, protocols);
    const send = ws.send.bind(ws);
    ws.send = function(data) {
      try { window.__wslog.push({d: 'out', url: String(url), t: Date.now(), data: String(data).slice(0, 20000)}); } catch(e){}
      return send(data);
    };
    ws.addEventListener('message', ev => {
      try { window.__wslog.push({d: 'in', url: String(url), t: Date.now(), data: String(ev.data).slice(0, 60000)}); } catch(e){}
    });
    return ws;
  }
  W.prototype = OW.prototype;
  W.CONNECTING = 0; W.OPEN = 1; W.CLOSING = 2; W.CLOSED = 3;
  window.WebSocket = W;
})();
"""
