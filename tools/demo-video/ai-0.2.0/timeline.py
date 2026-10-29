"""Timeline of the 0.2.0 video from the VO sentence durations (audio/timing.json)."""
import json, os

MEDIA = os.path.expanduser("~/side projects/rusty-inbox-video-media")
TIM = json.load(open(f"{MEDIA}/audio/timing.json"))
GAP_IN = 0.25      # between sentences of a scene
GAP_SC = 0.75      # last word of a scene -> first word of the next (incl. the whoosh)
LEAD = 0.55        # first sentence of the video
TAIL = 2.6         # after the last sentence

S = {}             # scene -> dict(start, end, sent=[(t0, t1, text, file, words)])
t = LEAD
for sc in sorted(TIM, key=int):
    sents = []
    for i, s in enumerate(TIM[sc]):
        t0 = t
        t1 = t0 + s["dur"]
        words = [{"w": w["w"].strip(), "s": t0 + w["s"], "e": t0 + w["e"]} for w in s["words"]]
        sents.append({"t0": t0, "t1": t1, "text": s["text"], "file": s["file"], "words": words})
        t = t1 + (GAP_IN if i < len(TIM[sc]) - 1 else GAP_SC)
    S[int(sc)] = {"sents": sents, "first": sents[0]["t0"], "last": sents[-1]["t1"]}
keys = sorted(S)
for k in keys:
    S[k]["start"] = 0.0 if k == keys[0] else S[k]["first"] - 0.5
for a, b in zip(keys, keys[1:]):
    S[a]["end"] = S[b]["start"]
S[keys[-1]]["end"] = S[keys[-1]]["last"] + TAIL
TOTAL = S[keys[-1]]["end"]


def W(sc, sub, nth=0, sent=None):
    """Absolute start of the nth word containing `sub` (case-insensitive) in scene sc (optionally one sentence)."""
    n = 0
    for i, s in enumerate(S[sc]["sents"]):
        if sent is not None and i != sent:
            continue
        for w in s["words"]:
            if sub.lower() in w["w"].lower():
                if n == nth:
                    return round(w["s"], 2)
                n += 1
    raise KeyError((sc, sub, nth))


if __name__ == "__main__":
    for k in keys:
        sc = S[k]
        print(f"scene {k}: {sc['start']:.2f}-{sc['end']:.2f}  ({sc['end'] - sc['start']:.1f}s)")
        for s in sc["sents"]:
            print(f"   {s['t0']:6.2f}-{s['t1']:6.2f}  {s['text'][:70]}")
    print("TOTAL", round(TOTAL, 2))
