from mn import MN
import json,time
m=MN(); m.new_session()
out=[]
end=time.time()+180
while time.time()<end:
    try:
        d=m.js("return {q:document.querySelector('[data-testid=current-question-number]')?.innerText, next:[...document.querySelectorAll('div')].filter(e=>e.children.length===0&&/Next question/.test(e.innerText)).length, t:document.body.innerText.slice(-300), nws:(window.__wgn.ws||[]).length, qids:[...(window.__wgn.ws||[])].filter(x=>x.data.includes('quizId')).slice(-3).map(x=>x.data.slice(0,4000))}")["value"]
    except Exception as e:
        d={'err':str(e)}
    d['ts']=time.time(); out.append(d)
    open('/tmp/watch.json','w').write(json.dumps(out))
    time.sleep(1)
