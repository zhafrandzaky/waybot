import json, time, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from mn import MN, WS_SHIM

OUT = "/tmp/probe_live.json"
CODE = sys.argv[1]
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
      try { const txt = await (r.clone()).text(); rec.status = r.status; rec.res = txt.slice(0, 60000); } catch(e) { rec.err = 'body:'+e; }
      window.__fetchlog.push(rec); return r;
    } catch(e) { rec.err = String(e); window.__fetchlog.push(rec); throw e; }
  };

  const OX = window.XMLHttpRequest;
  window.XMLHttpRequest = function() {
    const x = new OX(); const open = x.open, send = x.send; let rec = null;
    x.open = function(m,u){ rec={url:String(u),method:m,t:Date.now()}; return open.apply(x,arguments); };
    x.send = function(b){ if(rec){ try{rec.body=b?String(b).slice(0,30000):'';}catch(e){} x.addEventListener('loadend',()=>{try{rec.status=x.status;rec.res=String(x.responseText||'').slice(0,60000);}catch(e){} (window.__fetchlog=window.__fetchlog||[]).push(rec);}); } return send.apply(x,arguments); };
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
        const qc = document.querySelector('.quiz-container[data-quesid]');
        const t = document.body ? document.body.innerText.slice(0, 8000) : '';
        const opts = [...document.querySelectorAll('button.option[data-cy^=option-], button[data-cy^=option-]')].map(e => ({cy:e.getAttribute('data-cy'), aria:e.getAttribute('aria-label'), sel:e.getAttribute('aria-selected'), cls:String(e.className).slice(0,160), html:e.outerHTML.slice(0,350)}));
        const inputs = [...document.querySelectorAll('input:not([aria-hidden]),textarea')].map(e => ({tag:e.tagName, cy:e.getAttribute('data-cy'), ph:e.placeholder, cls:String(e.className).slice(0,140), html:e.outerHTML.slice(0,300)}));
        const next = [...document.querySelectorAll('div,button')].filter(e=>e.children.length===0 && /Next question/i.test(e.innerText||'')).map(e=>e.outerHTML.slice(0,250));
        const submit = document.querySelector('button[data-cy=submit-button]');
        return {url: location.href, t, qn: document.querySelector('[data-testid=current-question-number]')?.innerText, qid: qc?.getAttribute('data-quesid'), cls: qc?.className, opts, inputs, next, submit: submit?{dis:submit.disabled, aria:submit.getAttribute('aria-label')}:null, fetch:(window.__fetchlog||[]).slice(-120), focus:document.hasFocus(), vis:document.visibilityState};
        """)["value"]
    except Exception as e:
        info = {"error": str(e)}
    info["tag"] = tag; info["ts"] = time.time()
    with open(OUT, "w") as f: json.dump(info, f)

for _ in range(40):
    time.sleep(0.5)
    try:
        if m.js("return !!document.querySelector('input[data-cy=gamecode-field]')")["value"]: break
    except Exception: pass
m.fill("input[data-cy=gamecode-field]", CODE)
m.click("button[data-cy=joinGame-button]")
for _ in range(60):
    time.sleep(0.5)
    try:
        if m.js("return !!document.querySelector('input[data-cy=enter-name-field]')")["value"]: break
    except Exception: pass
m.js(WS_SHIM); m.js(FETCH_SHIM)
m.fill("input[data-cy=enter-name-field]", NAME)
time.sleep(0.5)
m.click("button[data-cy=start-game-button]")
time.sleep(2)
snap("after_click")
try:
    txt = m.js("return document.body.innerText")["value"]
    if "already exists" in txt:
        m.js("const b=[...document.querySelectorAll('button')].find(x=>/^Yes$/i.test((x.innerText||'').trim())); if(b)b.click(); return !!b")
        time.sleep(2)
except Exception: pass
snap("joined")

handled = set()
end = time.time() + 1500
n = 0
while time.time() < end:
    n += 1
    time.sleep(1)
    snap(f"poll{n}")
    try:
        st = json.load(open(OUT))
        qid = st.get("qid")
        if not qid or qid in handled:
            continue
        handled.add(qid)
        # capture payload before answering
        n0 = m.js("return (window.__fetchlog||[]).length")["value"]
        # answer: type / select first option
        if st.get("inputs"):
            inp = st["inputs"][0]
            if inp.get("tag") in ("INPUT", "TEXTAREA"):
                sel = f"{inp['tag'].lower()}[data-cy='{inp['cy']}']" if inp.get("cy") else f"{inp['tag'].lower()}.{str(inp.get('cls','')).split()[0]}"
                try:
                    m.fill(sel, "Central Processing Unit")
                except Exception: pass
        if st.get("opts"):
            try:
                m.click(f"button[data-cy='{st['opts'][0]['cy']}']")
                if len(st["opts"]) > 1 and "is-msq" in st["opts"][0].get("cls", ""):
                    time.sleep(0.3)
                    m.click(f"button[data-cy='{st['opts'][1]['cy']}']")
            except Exception: pass
        time.sleep(0.6)
        if m.js("return !!document.querySelector('button[data-cy=submit-button]') && !document.querySelector('button[data-cy=submit-button]').disabled")["value"]:
            try: m.click("button[data-cy=submit-button]")
            except Exception: pass
        time.sleep(2)
        snap(f"answered_{qid}")
        log = m.js("return (window.__fetchlog||[]).slice(arguments[0])", [n0])["value"]
        with open("/tmp/probe_payloads.jsonl", "a") as f:
            f.write(json.dumps({"qid": qid, "log": log}) + "\n")
        # advance
        m.js("const d=[...document.querySelectorAll('div,button')].find(e=>e.children.length===0&&/Next question/i.test(e.innerText||'')); if(d)d.click(); return !!d")
        time.sleep(1.5)
    except Exception as e:
        with open("/tmp/probe_err.log", "a") as f: f.write(f"{time.time()} {e}\n")
