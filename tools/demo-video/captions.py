"""Burn feature captions (pill lower-thirds, Thmanyah Sans) into the tour recording.

    python3 captions.py   -> rusty-inbox-features.mp4
"""
import json
import os
import subprocess

from PIL import Image, ImageDraw, ImageFont, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
D = os.environ.get("DEMO_DIR", os.path.join(ROOT, "target", "demo"))
HERE = os.path.dirname(os.path.abspath(__file__))
FONT = os.environ.get("CAPTION_FONT", os.path.expanduser("~/.fonts/thmanyah/thmanyahsans-Bold.otf"))
marks = json.load(open(f"{D}/marks.json"))
os.makedirs(f"{D}/cap", exist_ok=True)

ICON = Image.open(os.path.join(ROOT, "app/packaging/icons/rusty-inbox-128.png")).convert("RGBA").resize((46, 46))


def pill(text, path, n, total):
    f = ImageFont.truetype(FONT, 34)
    small = ImageFont.truetype(FONT, 20)
    tw = f.getbbox(text)[2]
    counter = f"{n:02d}/{total:02d}"
    cw = small.getbbox(counter)[2]
    w, h = 46 + 18 + tw + 26 + cw + 56, 78
    pad = 30
    im = Image.new("RGBA", (w + 2 * pad, h + 2 * pad), (0, 0, 0, 0))
    sh = Image.new("RGBA", im.size, (0, 0, 0, 0))
    ImageDraw.Draw(sh).rounded_rectangle((pad, pad + 6, pad + w, pad + h + 6), 39, fill=(0, 0, 0, 120))
    im = Image.alpha_composite(im, sh.filter(ImageFilter.GaussianBlur(12)))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle((pad, pad, pad + w, pad + h), 39, fill=(18, 18, 22, 236), outline=(255, 255, 255, 40), width=2)
    im.alpha_composite(ICON, (pad + 18, pad + 16))
    d.text((pad + 18 + 46 + 18, pad + h / 2), text, font=f, fill=(255, 255, 255, 255), anchor="lm")
    d.text((pad + w - 28, pad + h / 2 + 1), counter, font=small, fill=(232, 116, 59, 255), anchor="rm")
    im.save(path)
    return im.size


inputs, filters = [], []
caps = [(t, label) for t, label in marks if label != "END"]
end = marks[-1][0]
last = "[0:v]"
for i, (t, label) in enumerate(caps):
    nxt = caps[i + 1][0] if i + 1 < len(caps) else end
    a, b = t + 0.25, max(t + 1.6, nxt - 0.2)
    path = f"{D}/cap/{i:02d}.png"
    pill(label, path, i + 1, len(caps))
    inputs += ["-loop", "1", "-t", f"{end + 1:.2f}", "-i", path]
    k = i + 1
    filters.append(
        f"[{k}:v]format=rgba,fade=t=in:st={a:.2f}:d=0.3:alpha=1,fade=t=out:st={b - 0.3:.2f}:d=0.3:alpha=1[c{k}]")
    out = f"[v{k}]"
    filters.append(f"{last}[c{k}]overlay=x=W-w-24:y=H-h-14:enable='between(t,{a:.2f},{b:.2f})'{out}")
    last = out

cmd = ["ffmpeg", "-v", "error", "-y", "-i", f"{D}/raw.mkv", *inputs, "-filter_complex", ";".join(filters),
       "-map", last, "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-pix_fmt", "yuv420p", "-r", "60",
       "-movflags", "+faststart", f"{D}/rusty-inbox-features.mp4"]
subprocess.run(cmd, check=True)
print("ok")
