"""Generate index.html (HyperFrames + GSAP) for the Rusty Inbox 0.2.0 video.

    python3 build.py        # writes $MEDIA/video/index.html from timeline.py (VO timing) + clips

Real footage: assets/clips/sNN.mp4 (cut by edit.py from the recorded app). Motion graphics: everything else.
"""
import os

from timeline import S, TOTAL, W

MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
OUT = f"{MEDIA}/video/index.html"
CMD = "flatpak install --user https://karem505.github.io/rusty-inbox/rusty-inbox.flatpakref"

CSS = r"""
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Regular.otf);font-weight:400}
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Medium.otf);font-weight:500}
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Bold.otf);font-weight:700}
@font-face{font-family:"Thmanyah Sans";src:url(assets/fonts/thmanyahsans-Black.otf);font-weight:900}
@font-face{font-family:"Plex Mono";src:url(assets/fonts/IBMPlexMono-Regular.ttf);font-weight:400}
@font-face{font-family:"Plex Mono";src:url(assets/fonts/IBMPlexMono-Medium.ttf);font-weight:500}
html,body{margin:0;width:1920px;height:1080px;overflow:hidden;background:#0E0D0C}
#root{position:relative;width:1920px;height:1080px;overflow:hidden;background:#0E0D0C;font-family:"Thmanyah Sans",sans-serif;color:#F6EEE6}
.scene{position:absolute;inset:0;background:#0E0D0C;overflow:hidden}
.glow{position:absolute;border-radius:50%;filter:blur(150px);pointer-events:none}
.grid{position:absolute;inset:0;opacity:.06;background-image:linear-gradient(rgba(246,238,230,.6) 1px,transparent 1px),linear-gradient(90deg,rgba(246,238,230,.6) 1px,transparent 1px);background-size:96px 96px}
.copy{position:absolute;top:150px;width:780px;display:flex;flex-direction:column;gap:22px;box-sizing:border-box}
.copy.L{left:96px}.copy.R{right:96px}
.kicker{font-size:26px;font-weight:700;letter-spacing:4px;text-transform:uppercase;color:#E8743B}
.title{font-size:88px;font-weight:900;line-height:1.04;margin:0;letter-spacing:-1px}
.body{font-size:30px;line-height:1.45;color:#C9BEB3;margin:0;max-width:700px}
.chips{display:flex;flex-wrap:wrap;gap:14px;margin-top:14px;max-width:760px}
.chip{padding:12px 24px;border-radius:30px;font-size:27px;font-weight:700;border:1.5px solid rgba(232,116,59,.55);background:#191715;color:#F6EEE6;white-space:nowrap}
.chip.hot{background:#E8743B;border-color:#E8743B;color:#14100D}
.chip.ok{border-color:rgba(246,238,230,.22);color:#C9BEB3}
.vw{position:absolute;top:140px;width:908px;height:800px;border-radius:20px;overflow:hidden;background:#191715;
  box-shadow:0 70px 130px -30px rgba(0,0,0,.9),0 0 0 1px rgba(246,238,230,.13)}
.vw.R{right:90px}.vw.L{left:90px}
.vw video{position:absolute;inset:0;width:100%;height:100%;object-fit:cover}
.foot{position:absolute;left:96px;bottom:44px;font-size:21px;color:#8F857B;letter-spacing:.3px}
.foot.R{left:auto;right:96px}
.center{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center}
.icon{width:210px;height:210px;filter:drop-shadow(0 30px 60px rgba(183,65,14,.55))}
.brand{font-size:140px;font-weight:900;line-height:1.1;margin:18px 0 0;letter-spacing:-2px}
.tag{font-size:40px;color:#C9BEB3;margin:10px 0 0}
.cmd{margin-top:38px;padding:22px 34px;border-radius:16px;background:#191715;border:1.5px solid rgba(246,238,230,.16);font-family:"Plex Mono",monospace;font-size:29px;font-weight:500;color:#F6EEE6;white-space:nowrap}
.cmd b{color:#E8743B;font-weight:500}
.links{display:flex;gap:22px;margin-top:30px;align-items:center}
.link{font-size:34px;font-weight:700}
.sponsor{padding:14px 30px;border-radius:34px;font-size:29px;font-weight:700;background:#E8743B;color:#14100D}
.cover-l{position:absolute;left:96px;top:0;bottom:0;width:900px;display:flex;flex-direction:column;justify-content:center}
.brandrow{display:flex;align-items:center;gap:26px}
.brandrow img{width:112px;height:112px;filter:drop-shadow(0 18px 36px rgba(183,65,14,.5))}
.brandrow .n{font-size:76px;font-weight:900;letter-spacing:-1px}
.ver{padding:8px 22px;border-radius:40px;background:#E8743B;color:#14100D;font-size:44px;font-weight:900}
.hook{font-size:128px;font-weight:900;line-height:1.03;margin:40px 0 0;letter-spacing:-3px}
.hook span{display:block}
.hook .a{color:#F6EEE6}.hook .b{color:#F6EEE6}.hook .c{color:#E8743B}
"""

html, js = [], []
z = 10
TL = 1  # scene track toggles 1/2, video track 3/4


def glow(x, y, size, color, op):
    return (f'<div class="glow" data-layout-ignore style="left:{x}px;top:{y}px;width:{size}px;height:{size}px;'
            f'background:{color};opacity:{op}"></div>')


def chip_js(cid, t, hot=False):
    flash = (f'tl.to("#{cid}",{{backgroundColor:"#E8743B",color:"#14100D",duration:.12,ease:"power1.out"}},{t + .25:.2f});'
             f'tl.to("#{cid}",{{backgroundColor:"#191715",color:"#F6EEE6",duration:.5,ease:"power1.inOut"}},{t + .75:.2f});') if not hot else ""
    return (f'tl.to("#{cid}",{{opacity:1,y:0,scale:1,duration:.45,ease:"expo.out"}},{t:.2f});' + flash)


def feature_scene(sc, side, kicker, title, body, chips, foot=True, glow_xy=None):
    """chips: list of (text, time, cls)."""
    global z, TL
    z += 10
    st, en = S[sc]["start"], S[sc]["end"] + 0.5
    ts, tv = (1, 3) if TL == 1 else (2, 4)
    TL = 2 if TL == 1 else 1
    sid = f"s{sc}"
    clip_d = round(S[sc]["end"] - st + 0.6, 2)
    gx = 1000 if side == "L" else -250          # glow behind the window
    chips_html = "".join(f'<span class="chip {c}" id="{sid}-c{i}">{t}</span>' for i, (t, _, c) in enumerate(chips))
    html.append(f'''<div id="{sid}" class="scene" data-start="{st:.2f}" data-duration="{en - st:.2f}" data-track-index="{ts}" style="z-index:{z}">
  {glow(gx, 120, 900, "#B7410E", .32)}<div class="grid" data-layout-ignore></div>
  <div class="copy {side}">
    <div class="kicker" id="{sid}-k">{kicker}</div>
    <h2 class="title" id="{sid}-t">{title}</h2>
    <p class="body" id="{sid}-b">{body}</p>
    <div class="chips">{chips_html}</div>
  </div>
  {'<div class="foot' + (' R' if side == 'R' else '') + '">Real recording on a fictional demo mailbox</div>' if foot else ''}
</div>
<div class="vw {'L' if side == 'R' else 'R'}" id="{sid}-w" style="z-index:{z + 5}">
  <video id="{sid}-v" src="assets/clips/{sid}.mp4" data-start="{st:.2f}" data-duration="{clip_d}" data-track-index="{tv}" muted playsinline></video>
</div>''')
    rot = 5 if side == "L" else -5
    dx = 120 if side == "L" else -120
    j = [f'gsap.set("#{sid}-w",{{opacity:0,transformPerspective:2400,rotationY:{rot},rotationX:2}});',
         f'gsap.set(".chip[id^={sid}-c]",{{opacity:0,y:16,scale:.92}});',
         f'tl.from("#{sid}",{{opacity:0,duration:.4,ease:"power2.out"}},{st:.2f});',
         f'tl.to("#{sid}-w",{{opacity:1,duration:.45,ease:"power1.out"}},{st + .1:.2f});',
         f'tl.from("#{sid}-w",{{x:{dx},scale:.92,duration:1.0,ease:"expo.out"}},{st + .1:.2f});',
         f'tl.to("#{sid}-w",{{rotationY:{rot / 3:.2f},duration:{max(en - st - 1.2, 1):.2f},ease:"sine.inOut"}},{st + 1.0:.2f});',
         f'tl.from("#{sid}-k",{{y:20,opacity:0,duration:.45,ease:"power2.out"}},{st + .15:.2f});',
         f'tl.from("#{sid}-t",{{y:56,opacity:0,duration:.75,ease:"power4.out"}},{st + .25:.2f});',
         f'tl.from("#{sid}-b",{{y:24,opacity:0,duration:.6,ease:"sine.out"}},{st + .5:.2f});']
    for i, (_, t, c) in enumerate(chips):
        j.append(chip_js(f"{sid}-c{i}", max(t, st + .8), hot=(c == "hot")))
    js.extend(j)


# ---------------------------------------------------------------- scene 1: cover + hook
z += 10
s1 = S[1]
html.append(f'''<div id="s1" class="scene" data-start="0" data-duration="{s1['end'] + .5:.2f}" data-track-index="1" style="z-index:{z}">
  {glow(900, 60, 1000, "#B7410E", .38)}<div class="grid" data-layout-ignore></div>
  <div class="cover-l">
    <div class="brandrow"><img id="s1-icon" src="assets/img/icon.png" alt=""><span class="n">Rusty Inbox</span><span class="ver" id="s1-ver">0.2.0</span></div>
    <h1 class="hook"><span class="a" id="s1-a">Your AI.</span><span class="b" id="s1-b">Your model.</span><span class="c" id="s1-c">Your mail.</span></h1>
  </div>
</div>
<div class="vw R" id="s1-w" style="z-index:{z + 5};top:120px;width:908px;height:800px;right:70px">
  <video id="s1-v" src="assets/clips/s1.mp4" data-start="0" data-duration="{s1['end'] + .6:.2f}" data-track-index="3" muted playsinline></video>
</div>''')
a, b, c = (W(1, "your", 0, 1), W(1, "model", 0, 1), W(1, "mail", 0, 1))
js.append(f'''gsap.set("#s1-w",{{transformPerspective:2400,rotationY:-7,rotationX:2,x:0}});
tl.to("#s1-w",{{rotationY:-2,duration:{s1['end'] - 0.5:.2f},ease:"sine.inOut"}},.5);
tl.to("#s1-ver",{{scale:1.12,duration:.35,ease:"power2.out"}},.55);
tl.to("#s1-ver",{{scale:1,duration:.5,ease:"power2.inOut"}},.9);
gsap.set("#s1-a,#s1-b",{{opacity:1}});
tl.to("#s1-a",{{color:"#E8743B",duration:.25,ease:"power1.out"}},{a:.2f});
tl.to("#s1-a",{{color:"#F6EEE6",duration:.5,ease:"power1.inOut"}},{a + .7:.2f});
tl.to("#s1-b",{{color:"#E8743B",duration:.25,ease:"power1.out"}},{b:.2f});
tl.to("#s1-b",{{color:"#F6EEE6",duration:.5,ease:"power1.inOut"}},{b + .7:.2f});
tl.fromTo("#s1-c",{{scale:1}},{{scale:1.05,transformOrigin:"left center",duration:.3,ease:"power2.out"}},{c:.2f});
tl.to("#s1-c",{{scale:1,duration:.5,ease:"power2.inOut"}},{c + .4:.2f});''')

# ---------------------------------------------------------------- scene 2: choose your AI
st2 = S[2]["start"]
feature_scene(
    2, "L", "01 · Choose your AI", "Bring your own model.",
    "Sign in with ChatGPT, or use your own key. The model list is fetched live from the provider.",
    [("ChatGPT subscription", W(2, "chat", 0, 0), "hot"),
     ("OpenAI", W(2, "openai", 0, 1), "ok"), ("Anthropic", W(2, "anthropic", 0, 1), "ok"),
     ("OpenRouter", W(2, "router", 0, 1), "ok"), ("Ollama", W(2, "ol", 0, 1), "ok"),
     ("Any compatible server", W(2, "compatible", 0, 1), "ok"),
     ("Live model list + search", W(2, "list", 0, 2), "hot"), ("Thinking level", W(2, "think", 0, 2), "hot")])

# ---------------------------------------------------------------- scene 3: write
feature_scene(
    3, "R", "02 · Write", "Write with a safety net.",
    "Proofread, rewrite, or write from one line. Nothing changes until you click Replace.",
    [("Proofread", W(3, "proofread", 0, 0), "hot"), ("Rewrite", W(3, "rewrite", 0, 1), "hot"),
     ("Write for me", W(3, "one", 0, 2), "hot"), ("You click Replace", W(3, "replace", 0, 3), "hot"),
     ("Signature and quote untouched", W(3, "signature", 0, 3), "ok")])

# ---------------------------------------------------------------- scene 4: read
feature_scene(
    4, "L", "03 · Read", "Understand it in one click.",
    "Summaries with action items and due dates, suggested replies, translation, and attachments.",
    [("Summary + action items", W(4, "summar", 0, 0), "hot"), ("3 suggested replies", W(4, "suggested", 0, 1), "hot"),
     ("Translate in place", W(4, "translate", 0, 2), "hot"), ("Attachment summary", W(4, "attachment", 0, 2), "hot")])

# ---------------------------------------------------------------- scene 5: stay safe
feature_scene(
    5, "R", "04 · Stay safe", "Spot phishing first.",
    "One click gives a risk level and the reasons, right above the message.",
    [("Risk level", W(5, "risk", 0, 0), "hot"), ("The reasons", W(5, "reasons", 0, 0), "hot")])

# ---------------------------------------------------------------- scene 6: stay on top
feature_scene(
    6, "L", "05 · Stay on top", "Never lose a thread.",
    "Mail that needs an answer, mail nobody answered, plain-words search and a daily briefing.",
    [("Needs reply", W(6, "needs", 0, 0), "hot"), ("Follow up", W(6, "follow", 0, 1), "hot"),
     ("Ask in plain words", W(6, "plain", 0, 2), "hot"), ("Daily briefing", W(6, "briefing", 0, 3), "hot")])

# ---------------------------------------------------------------- scene 7: privacy
feature_scene(
    7, "R", "06 · Private by design", "Off until you turn it on.",
    "Only the mail you act on goes to the provider you chose. With Ollama, nothing leaves your machine.",
    [("Off by default", W(7, "off", 0, 0), "hot"), ("Only what you click", W(7, "only", 0, 1), "hot"),
     ("Exclude accounts", W(7, "exclude", 0, 1), "hot"), ("Ollama: all local", W(7, "stays", 0, 1), "hot")])

# ---------------------------------------------------------------- scene 8: end card
z += 10
s8 = S[8]
t8 = s8["start"]
html.append(f'''<div id="s8" class="scene" data-start="{t8:.2f}" data-duration="{s8['end'] - t8:.2f}" data-track-index="1" style="z-index:{z}">
  {glow(560, 100, 800, "#B7410E", .5)}<div class="grid" data-layout-ignore></div>
  <div class="center">
    <img id="s8-icon" class="icon" src="assets/img/icon.png" alt="">
    <h1 id="s8-brand" class="brand">Rusty Inbox 0.2.0</h1>
    <p id="s8-tag" class="tag">Free and open source · Linux · Windows · macOS</p>
    <div id="s8-cmd" class="cmd"><b>$</b> {CMD}</div>
    <div class="links" id="s8-links"><span class="link">github.com/karem505/rusty-inbox</span><span class="sponsor" id="s8-sp">Sponsor on GitHub</span></div>
  </div>
</div>''')
t_l = W(8, "linux", 0, 0)
t_f = W(8, "flatpak", 0, 1)
t_g = W(8, "github", 0, 1)
js.append(f'''gsap.set("#s8-cmd,#s8-links",{{opacity:0,y:24}});
tl.from("#s8",{{opacity:0,duration:.35,ease:"power2.out"}},{t8:.2f});
tl.from("#s8-icon",{{scale:.5,opacity:0,duration:.9,ease:"expo.out"}},{t8 + .15:.2f});
tl.from("#s8-brand",{{y:50,opacity:0,duration:.8,ease:"power4.out"}},{t8 + .35:.2f});
tl.from("#s8-tag",{{y:24,opacity:0,duration:.6,ease:"sine.out"}},{max(t_l - .4, t8 + .7):.2f});
tl.to("#s8-cmd",{{opacity:1,y:0,duration:.6,ease:"expo.out"}},{S[8]["sents"][1]["t0"] - .1:.2f});
tl.to("#s8-links",{{opacity:1,y:0,duration:.6,ease:"expo.out"}},{t_g - .2:.2f});
tl.to("#s8-sp",{{scale:1.06,duration:.4,ease:"power2.out"}},{t_g + 1.0:.2f});
tl.to("#s8-sp",{{scale:1,duration:.5,ease:"power2.inOut"}},{t_g + 1.4:.2f});
tl.to("#s8 .center",{{opacity:0,duration:.5,ease:"power2.in"}},{TOTAL - .5:.2f});''')

DOC = f'''<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8" />
<meta name="viewport" content="width=1920, height=1080" />
<title>Rusty Inbox 0.2.0: the AI release</title>
<script src="assets/gsap.min.js"></script>
<style>{CSS}</style>
</head>
<body>
<div id="root" data-composition-id="main" data-start="0" data-duration="{TOTAL:.2f}" data-width="1920" data-height="1080">
{chr(10).join(html)}
<audio id="mix" src="assets/audio/mix.wav" data-start="0" data-duration="{TOTAL:.2f}" data-track-index="5" data-volume="1"></audio>
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
open(OUT, "w").write(DOC)
print("ok", len(DOC), "total", round(TOTAL, 2))
