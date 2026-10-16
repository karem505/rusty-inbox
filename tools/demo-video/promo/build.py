"""Generate index.html for the Rusty Inbox launch video (HyperFrames + GSAP)."""
import json

SCENES = [
    # id, start, dur, video, media_start, side, kicker, title, body
    ("s3", 9.2, 7.6, "inbox", 0.4, "L", "01 · Smart Inbox", "People first.",
     "Mail sorts itself into People, Invitations, Notifications and Newsletters. Automated mail folds out of the way."),
    ("s4", 16.4, 8.2, "reader", 0.6, "R", "02 · Keyboard", "Fly through your inbox.",
     "Full HTML email in threads. Open, pin, done, snooze and undo without leaving the keyboard."),
    ("s5", 24.2, 5.6, "select", 0.2, "L", "03 · Bulk actions", "Select. All. Done.",
     "Pick a few conversations or the whole list, and act on all of them at once, with undo."),
    ("s6", 29.4, 5.6, "accounts", 0.5, "R", "04 · Accounts", "Every account, one place.",
     "Each list shows all your accounts together, or unfolds into one account at a time."),
    ("s7", 34.6, 5.8, "addaccount", 0.4, "L", "05 · Settings", "Add any account in seconds.",
     "Email and password; the servers are found for you. Every option lives in one clean Settings window."),
    ("s8", 40.0, 7.6, "themes", 0.5, "R", "06 · Native", "Looks like your desktop.",
     "Follows your system theme and font, live. Change the theme and Rusty Inbox changes with it."),
]
KEYS = [("J", 2.4), ("J", 2.9), ("K", 3.5), ("P", 4.3), ("E", 5.05), ("Z", 5.6), ("S", 6.6)]
THEMES = ["Tokyo Night", "Catppuccin Latte", "Everforest", "Rosé Pine", "Nord", "Gruvbox"]
TOTAL = 53.0

css = r"""
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Regular.otf);font-weight:400}
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Medium.otf);font-weight:500}
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Bold.otf);font-weight:700}
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Black.otf);font-weight:900}
html,body{margin:0;width:1920px;height:1080px;overflow:hidden;background:#0E0D0C}
#root{position:relative;width:1920px;height:1080px;overflow:hidden;background:#0E0D0C;
  font-family:"Thmanyah Sans",sans-serif;color:#F6EEE6}
.scene{position:absolute;inset:0;background:#0E0D0C;overflow:hidden}
.glow{position:absolute;border-radius:50%;filter:blur(140px);pointer-events:none}
.grid{position:absolute;inset:0;opacity:.07;
  background-image:linear-gradient(rgba(246,238,230,.6) 1px,transparent 1px),linear-gradient(90deg,rgba(246,238,230,.6) 1px,transparent 1px);
  background-size:96px 96px}
.copy{position:absolute;top:0;bottom:0;width:500px;display:flex;flex-direction:column;justify-content:center;gap:22px;box-sizing:border-box}
.copy.L{left:90px}.copy.R{right:90px}
.kicker{font-size:24px;font-weight:700;letter-spacing:3px;text-transform:uppercase;color:#E8743B}
.title{font-size:72px;font-weight:900;line-height:1.08;margin:0}
.body{font-size:27px;line-height:1.5;color:#C9BEB3;margin:0;max-width:500px}
.vw{position:absolute;top:202px;width:1200px;height:675px;border-radius:18px;overflow:hidden;
  box-shadow:0 60px 120px -30px rgba(0,0,0,.85),0 0 0 1px rgba(246,238,230,.12);background:#191715}
.vw.L{right:70px}.vw.R{left:70px}
.vw video{position:absolute;inset:0;width:100%;height:100%;object-fit:cover}
.keys{display:flex;gap:12px;margin-top:10px}
.key{width:56px;height:56px;border-radius:14px;background:#191715;border:2px solid rgba(246,238,230,.16);
  display:flex;align-items:center;justify-content:center;font-size:28px;font-weight:700;color:#F6EEE6;
  box-shadow:0 6px 0 rgba(0,0,0,.5)}
.tick{position:relative;height:64px;margin-top:6px}
.tick span{position:absolute;left:0;top:0;padding:12px 24px;border-radius:32px;font-size:26px;font-weight:700;
  background:rgba(232,116,59,.14);border:1.5px solid rgba(232,116,59,.55);color:#F6EEE6;white-space:nowrap}
/* intro / outro */
.center{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:26px}
.icon{width:220px;height:220px;filter:drop-shadow(0 30px 60px rgba(183,65,14,.55))}
.brand{font-size:150px;font-weight:900;line-height:1.1;margin:0;letter-spacing:-2px}
.tag{font-size:40px;color:#C9BEB3;margin:0}
.chips{display:flex;gap:16px;margin-top:8px}
.chip{padding:12px 26px;border-radius:30px;font-size:26px;font-weight:700;border:1.5px solid rgba(246,238,230,.18);color:#F6EEE6;background:#191715}
/* stats */
.stats{position:absolute;inset:0;display:flex;flex-direction:column;justify-content:center;padding:0 140px;gap:56px;box-sizing:border-box}
.stats h2{font-size:84px;font-weight:900;margin:0;line-height:1.1}
.cards{display:flex;gap:32px}
.card{flex:1;background:#191715;border:1px solid rgba(246,238,230,.10);border-radius:24px;padding:34px 38px}
.num{font-size:104px;font-weight:900;color:#E8743B;line-height:1;font-variant-numeric:tabular-nums}
.unit{font-size:44px;font-weight:700;margin-left:8px}
.lab{font-size:28px;color:#C9BEB3;margin-top:14px}
.bars{display:flex;flex-direction:column;gap:16px}
.bar{display:flex;align-items:center;gap:22px;font-size:26px;color:#C9BEB3}
.bar .name{width:330px}
.bar .track{flex:1;height:26px;border-radius:13px;background:rgba(246,238,230,.06);position:relative;overflow:hidden}
.bar .fill{position:absolute;left:0;top:0;bottom:0;border-radius:13px;transform-origin:left center}
.bar .val{width:150px;text-align:right;font-weight:700;color:#F6EEE6;font-variant-numeric:tabular-nums}
.foot{font-size:20px;color:#9E9389}
"""


def glow(x, y, size, color, op):
    return (f'<div class="glow" data-layout-ignore style="left:{x}px;top:{y}px;width:{size}px;height:{size}px;'
            f'background:{color};opacity:{op}"></div>')


html = []
js = []
z = 10
# ---- S1 intro
html.append(f'''<div id="s1" class="scene" data-start="0" data-duration="4.2" data-track-index="1" style="z-index:{z}">
  {glow(560, 140, 800, "#B7410E", .45)}<div class="grid" data-layout-ignore></div>
  <div class="center">
    <img id="s1-icon" class="icon" src="assets/img/icon.png" alt="">
    <h1 id="s1-brand" class="brand">Rusty Inbox</h1>
    <p id="s1-tag" class="tag">The fast, native email client for Linux.</p>
    <div class="chips"><span class="chip s1-chip">Rust engine</span><span class="chip s1-chip">Qt 6 / QML</span><span class="chip s1-chip">Wayland and X11</span></div>
  </div></div>''')
js.append('''tl.from("#s1-icon",{scale:.4,opacity:0,rotation:-12,duration:.9,ease:"expo.out"},.15);
tl.from("#s1-brand",{y:60,opacity:0,duration:.8,ease:"power4.out"},.45);
tl.from("#s1-tag",{y:30,opacity:0,duration:.6,ease:"power2.out"},.8);
tl.from(".s1-chip",{y:24,opacity:0,duration:.5,stagger:.12,ease:"back.out(1.4)"},1.1);''')

# ---- S2 stats
z += 10
html.append(f'''<div id="s2" class="scene" data-start="3.8" data-duration="5.8" data-track-index="2" style="z-index:{z}">
  {glow(1200, -200, 900, "#B7410E", .35)}
  <div class="stats">
    <h2 id="s2-h">Built in Rust. Feels instant.</h2>
    <div class="cards">
      <div class="card s2-card"><div><span class="num" id="n1">0.00</span><span class="unit">s</span></div><div class="lab">to a full inbox of 482 conversations</div></div>
      <div class="card s2-card"><div><span class="num" id="n2">0</span><span class="unit">MB</span></div><div class="lab">memory with the inbox open</div></div>
      <div class="card s2-card"><div><span class="num" id="n3">0</span><span class="unit">MB</span></div><div class="lab">one native binary</div></div>
    </div>
    <div class="bars">
      <div class="bar s2-bar"><span class="name">Rusty Inbox</span><span class="track"><span class="fill" id="b1" style="width:14.5%;background:#E8743B"></span></span><span class="val">87 MB</span></div>
      <div class="bar s2-bar"><span class="name">Electron mail client</span><span class="track"><span class="fill" id="b2" style="width:100%;background:#5B524B"></span></span><span class="val">602 MB</span></div>
      <div class="foot s2-bar">Memory measured on the same machine with the inbox open. Lower is better.</div>
    </div>
  </div></div>''')
js.append('''tl.from("#s2",{opacity:0,duration:.45,ease:"power2.out"},3.8);
tl.from("#s2-h",{x:-60,opacity:0,duration:.7,ease:"expo.out"},3.95);
tl.from(".s2-card",{y:50,opacity:0,duration:.6,stagger:.12,ease:"power3.out"},4.2);
const c1={v:0},c2={v:0},c3={v:0};
tl.to(c1,{v:.69,duration:1.1,ease:"power2.out",onUpdate:()=>{document.getElementById("n1").textContent=c1.v.toFixed(2)}},4.4);
tl.to(c2,{v:87,duration:1.1,ease:"power2.out",onUpdate:()=>{document.getElementById("n2").textContent=Math.round(c2.v)}},4.55);
tl.to(c3,{v:17,duration:1.0,ease:"power2.out",onUpdate:()=>{document.getElementById("n3").textContent=Math.round(c3.v)}},4.7);
tl.from(".s2-bar",{opacity:0,y:20,duration:.5,stagger:.1,ease:"sine.out"},5.3);
tl.from("#b1",{scaleX:0,duration:.9,ease:"expo.out"},5.6);
tl.from("#b2",{scaleX:0,duration:1.4,ease:"power2.inOut"},5.7);''')

# ---- feature scenes
for i, (sid, start, dur, video, ms, side, kicker, title, body) in enumerate(SCENES):
    z += 10
    track_scene = 1 if i % 2 == 0 else 2
    track_vid = 3 if i % 2 == 0 else 4
    copy_side = side
    win_side = side  # .vw.L sits right of a left copy column
    extra = ""
    if sid == "s4":
        extra = '<div class="keys">' + "".join(f'<div class="key" id="k{j}">{k}</div>' for j, (k, _) in enumerate(KEYS)) + "</div>"
    if sid == "s8":
        extra = '<div class="tick">' + "".join(f'<span id="t{j}">{t}</span>' for j, t in enumerate(THEMES)) + "</div>"
    gx = 1100 if side == "L" else -100
    html.append(f'''<div id="{sid}" class="scene" data-start="{start}" data-duration="{dur}" data-track-index="{track_scene}" style="z-index:{z}">
  {glow(gx, 160, 820, "#B7410E", .30)}<div class="grid" data-layout-ignore></div>
  <div class="copy {copy_side}">
    <div class="kicker" id="{sid}-k">{kicker}</div>
    <h2 class="title" id="{sid}-t">{title}</h2>
    <p class="body" id="{sid}-b">{body}</p>
    {extra}
  </div></div>
<div class="vw {win_side}" id="{sid}-w" style="z-index:{z + 5}">
  <video id="{sid}-v" src="assets/clips/{video}.mp4" data-start="{start}" data-duration="{dur}" data-media-start="{ms}" data-track-index="{track_vid}" muted playsinline></video>
</div>''')
    rot = -6 if side == "L" else 6
    dx = 140 if side == "L" else -140
    js.append(f'''gsap.set("#{sid}-w",{{opacity:0,transformPerspective:2400,rotationY:{rot},rotationX:3}});
tl.from("#{sid}",{{opacity:0,duration:.45,ease:"power2.out"}},{start});
tl.to("#{sid}-w",{{opacity:1,duration:.5,ease:"power1.out"}},{start + .05});
tl.from("#{sid}-w",{{x:{dx},scale:.9,duration:.9,ease:"expo.out"}},{start + .05});
tl.to("#{sid}-w",{{rotationY:{rot / 2},duration:{dur - .6},ease:"sine.inOut"}},{start + .6});
tl.from("#{sid}-k",{{y:20,opacity:0,duration:.45,ease:"power2.out"}},{start + .25});
tl.from("#{sid}-t",{{y:50,opacity:0,duration:.7,ease:"power4.out"}},{start + .35});
tl.from("#{sid}-b",{{y:24,opacity:0,duration:.6,ease:"sine.out"}},{start + .55});''')
    if sid == "s4":
        js.append(f'tl.from(".key",{{y:20,opacity:0,duration:.4,stagger:.05,ease:"back.out(1.6)"}},{start + .8});')
        for j, (_, t) in enumerate(KEYS):
            js.append(f'tl.to("#k{j}",{{keyframes:[{{scale:.86,backgroundColor:"#E8743B",borderColor:"#E8743B",duration:.08}},'
                      f'{{scale:1,backgroundColor:"#191715",borderColor:"rgba(246,238,230,.16)",duration:.35}}],ease:"power1.out"}},{start + t});')
    if sid == "s8":
        js.append('gsap.set(".tick span",{opacity:0,y:16});')
        for j in range(len(THEMES)):
            t = start + 1.0 + j
            js.append(f'tl.to("#t{j}",{{opacity:1,y:0,duration:.3,ease:"power2.out"}},{t});')
            if j < len(THEMES) - 1:
                js.append(f'tl.to("#t{j}",{{opacity:0,y:-16,duration:.25,ease:"power2.in"}},{t + .85});')

# ---- outro
z += 10
html.append(f'''<div id="s9" class="scene" data-start="47.2" data-duration="5.8" data-track-index="1" style="z-index:{z}">
  {glow(560, 140, 800, "#B7410E", .45)}<div class="grid" data-layout-ignore></div>
  <div class="center">
    <img id="s9-icon" class="icon" src="assets/img/icon.png" alt="">
    <h1 id="s9-brand" class="brand">Rusty Inbox</h1>
    <p id="s9-tag" class="tag">Fast. Native. Yours.</p>
    <div class="chips"><span class="chip s9-chip">Available now</span></div>
  </div></div>''')
js.append('''tl.from("#s9",{opacity:0,duration:.5,ease:"power2.out"},47.2);
tl.from("#s9-icon",{scale:.5,opacity:0,duration:.9,ease:"expo.out"},47.4);
tl.from("#s9-brand",{y:50,opacity:0,duration:.8,ease:"power4.out"},47.7);
tl.from("#s9-tag",{y:24,opacity:0,duration:.6,ease:"sine.out"},48.1);
tl.from(".s9-chip",{scale:.8,opacity:0,duration:.5,ease:"back.out(1.5)"},48.5);
tl.to("#s9 .center",{opacity:0,duration:.8,ease:"power2.in"},52.1);''')

doc = f'''<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=1920, height=1080" />
<title>Rusty Inbox: launch video</title>
<script src="assets/gsap.min.js"></script>
<style>{css}</style>
</head>
<body>
<div id="root" data-composition-id="main" data-start="0" data-duration="{TOTAL}" data-width="1920" data-height="1080">
{chr(10).join(html)}
<audio id="bgm" src="assets/audio/bgm.wav" data-start="0" data-duration="{TOTAL}" data-track-index="5" data-volume="0.9"></audio>
</div>
<script>
window.__timelines = window.__timelines || {{}};
const tl = gsap.timeline({{ paused: true }});
{chr(10).join(js)}
window.__timelines["main"] = tl;
</script>
</body>
</html>
'''
open("index.html", "w").write(doc)
print("ok", len(doc))
