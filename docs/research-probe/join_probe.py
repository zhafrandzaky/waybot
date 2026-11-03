import json, time, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from mn import MN, WS_SHIM

OUT = "/tmp/probe_state.json"
CODE = sys.argv[1] if len(sys.argv) > 1 else "930417"
NAME = sys.argv[2] if len(sys.argv) > 2 else "WayBotProbe"

FETCH_SHIM = r"""
(() => {
  if (window.__fetchlog_installed) return;
  window.__fetchlog_installed = true;
  window.__fetchlog = [];
  const of = window.fetch;
  window.fetch = async function(input, init) {
    const url = typeof input === 'string' ? input : (input && input.url) || '';
    let body = '';
    try { body = init && init.body ? String(init.body).slice(0, 30000) : ''; } catch(e) {}
    const rec = {url, method: (init && init.method) || 'GET', body, t: Date.now()};
    try {
      const r = await of(input, init);
      try {
        const clone = r.clone();
        const txt = await clone.text();
        rec.status = r.status; rec.res = txt.slice(0, 60000);
      } catch(e) { rec.err = 'body:'+e; }
      window.__fetchlog.push(rec);
      return r;
    } catch(e) {
      rec.err = String(e); window.__fetchlog.push(rec); throw e;
    }
  };
  const OX = window.XMLHttpRequest;
  window.XMLHttpRequest = function() {
    const x = new OX();
    const open = x.open, send = x.send;
    let rec = null;
    x.open = function(m, u) { rec = {url: String(u), method: m, t: Date.now()}; try { open.apply(x, arguments); } catch(e) { throw e; } };
    x.send = function(b) {
      if (rec) {
        try { rec.body = b ? String(b).slice(0, 30000) : ''; } catch(e) {}
        x.addEventListener('loadend', () => {
          try { rec.status = x.status; rec.res = String(x.responseText || '').slice(0, 60000); } catch(e) {}
          window.__fetchlog.push(rec);
        });
      }
      return send.apply(x, arguments);
    };
    return x;
  };
})();
"""

m = MN()
m.new_session()
m.nav("https://wayground.com/join")

def snap(tag):
    try:
        info = m.js("""
        const t = document.body ? document.body.innerText.slice(0, 6000) : '';
        const els = [...document.querySelectorAll('input,button,textarea,[data-testid],[role=button]')].map(e => ({tag:e.tagName, cy:e.getAttribute('data-cy'), tid:e.getAttribute('data-testid'), txt:(e.innerText||'').slice(0,80), ph:e.placeholder||'', aria:e.getAttribute('aria-label')||'', cls:(e.className||'').toString().slice(0,120), html:e.outerHTML.slice(0,300)})).slice(0,160);
        const q = document.querySelector('[data-testid=current-question-number]');
        return {url: location.href, t, els, q: q ? q.innerText : '', ws:(window.__wslog||[]).slice(-200), fetch:(window.__fetchlog||[]).slice(-200), focus: document.hasFocus(), vis: document.visibilityState};
        """)["value"]
    except Exception as e:
        info = {"error": str(e)}
    info["tag"] = tag
    info["ts"] = time.time()
    with open(OUT, "w") as f:
        json.dump(info, f)

for _ in range(40):
    time.sleep(0.5)
    try:
        if m.js("return document.querySelector('input[data-cy=gamecode-field]') !== null")["value"]:
            break
    except Exception:
        pass
m.fill("input[data-cy=gamecode-field]", CODE)
m.click("button[data-cy=joinGame-button]")
for _ in range(60):
    time.sleep(0.5)
    if m.js("return document.querySelector('input[data-cy=enter-name-field]') !== null")["value"]:
        break
m.js(WS_SHIM)
m.js(FETCH_SHIM)
m.fill("input[data-cy=enter-name-field]", NAME)
time.sleep(0.5)
m.click("button[data-cy=start-game-button]")
time.sleep(2)
snap("after_start_click")

# resume dialog handling
try:
    txt = m.js("return document.body.innerText")["value"]
    if "incomplete game already exists" in txt or "Resume game" in txt:
        m.js("""
        const btn=[...document.querySelectorAll('button')].find(b=>/^Yes$/i.test((b.innerText||'').trim()));
        if(btn) btn.click(); return !!btn;
        """)
        time.sleep(2)
        snap("after_resume_click")
except Exception:
    pass

snap("joined")

end = time.time() + 1200
n = 0
while time.time() < end:
    time.sleep(1)
    n += 1
    snap(f"poll{n}")
