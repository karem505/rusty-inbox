"""VO for the 0.2.0 video: ElevenLabs per sentence, whisper (Groq) word timings + round-trip check.

    python3 gen_vo.py            # synth missing sentences, build audio/vo/*.wav + timing.json
Outputs in $MEDIA/audio: sNN_MM.wav (trimmed sentence), timing.json {scene: [{text, file, dur, words:[{w,s,e}]}]}
"""
import hashlib, json, os, re, subprocess, sys, urllib.request, uuid

HERE = os.path.dirname(os.path.abspath(__file__))
MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
AUD = f"{MEDIA}/audio"
os.makedirs(f"{AUD}/cache", exist_ok=True)
for l in open(os.path.expanduser("~/.hermes/.env")):
    if "=" in l and not l.startswith("#"):
        k, v = l.strip().split("=", 1)
        os.environ.setdefault(k, v.strip('"').strip("'"))
EL = os.environ["ELEVENLABS_API_KEY"]
GROQ = os.environ["GROQ_API_KEY"]
VOICE = "onwK4e9ZLuTAKqWW03F9"
MODEL = os.environ.get("EL_MODEL", "eleven_v4")

scenes, cur = {}, None
for line in open(f"{HERE}/script.md"):
    m = re.match(r"## (\d+)\.", line)
    if m:
        cur = int(m.group(1)); scenes[cur] = []
    elif line.startswith("- ") and cur:
        scenes[cur].append(line[2:].strip())
flat = [(sc, i, t) for sc, ts in scenes.items() for i, t in enumerate(ts)]


def post(url, body, headers, raw=False):
    req = urllib.request.Request(url, data=body, headers=headers)
    return urllib.request.urlopen(req, timeout=120).read()


def synth(text, prev, nxt):
    key = hashlib.md5(f"{MODEL}|{VOICE}|{text}|{prev}|{nxt}".encode()).hexdigest()
    f = f"{AUD}/cache/{key}.mp3"
    if not os.path.exists(f):
        body = {"text": text, "model_id": MODEL, "previous_text": prev, "next_text": nxt,
                "voice_settings": {"stability": 0.5, "similarity_boost": 0.8, "style": 0.0, "use_speaker_boost": True}}
        data = post(f"https://api.elevenlabs.io/v1/text-to-speech/{VOICE}?output_format=mp3_44100_128",
                    json.dumps(body).encode(), {"xi-api-key": EL, "Content-Type": "application/json"})
        open(f, "wb").write(data)
    return f


def trim(src, dst):
    af = ("silenceremove=start_periods=1:start_threshold=-55dB:start_silence=0.03,areverse,"
          "silenceremove=start_periods=1:start_threshold=-55dB:start_silence=0.05,areverse,"
          "afade=t=in:d=0.025,loudnorm=I=-16:TP=-1.5")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", src, "-af", af, "-ar", "48000", "-ac", "1", dst], check=True)


def whisper(wav):
    flac = wav.replace(".wav", ".flac")
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", wav, "-ar", "16000", "-ac", "1", flac], check=True)
    b = uuid.uuid4().hex
    parts = []
    def fld(n, v): parts.append(f'--{b}\r\nContent-Disposition: form-data; name="{n}"\r\n\r\n{v}\r\n'.encode())
    fld("model", "whisper-large-v3"); fld("language", "en"); fld("response_format", "verbose_json")
    parts.append(f'--{b}\r\nContent-Disposition: form-data; name="timestamp_granularities[]"\r\n\r\nword\r\n'.encode())
    parts.append((f'--{b}\r\nContent-Disposition: form-data; name="file"; filename="a.flac"\r\nContent-Type: audio/flac\r\n\r\n').encode()
                 + open(flac, "rb").read() + b"\r\n")
    parts.append(f"--{b}--\r\n".encode())
    r = json.loads(post("https://api.groq.com/openai/v1/audio/transcriptions", b"".join(parts),
                        {"Authorization": f"Bearer {GROQ}", "Content-Type": f"multipart/form-data; boundary={b}",
                         "User-Agent": "curl/8"}))
    return r["text"], [{"w": w["word"], "s": w["start"], "e": w["end"]} for w in r.get("words", [])]


def norm(s):
    return re.sub(r"[^a-z0-9 ]", "", s.lower().replace("-", " ")).split()


out = {}
for n, (sc, i, t) in enumerate(flat):
    prev = flat[n - 1][2] if n else ""
    nxt = flat[n + 1][2] if n + 1 < len(flat) else ""
    mp3 = synth(t, prev, nxt)
    wav = f"{AUD}/s{sc:02d}_{i}.wav"
    trim(mp3, wav)
    dur = float(subprocess.check_output(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", wav]))
    jf = wav.replace(".wav", ".json")
    if os.path.exists(jf):
        text, words = json.load(open(jf))
    else:
        text, words = whisper(wav)
        json.dump([text, words], open(jf, "w"))
    exp, got = norm(t), norm(text)
    ok = exp == got
    print(f"{sc}.{i} {dur:5.2f}s {'OK ' if ok else 'DIFF'} {text.strip() if not ok else ''}")
    out.setdefault(sc, []).append({"text": t, "file": wav, "dur": dur, "words": words, "heard": text})
json.dump(out, open(f"{AUD}/timing.json", "w"), indent=1)
