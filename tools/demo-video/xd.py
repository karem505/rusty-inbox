"""xd: drive a private X display with XTest (smooth mouse, clicks, keys, typing).

    from xd import X; x = X(":78"); x.move(400, 300); x.click(); x.key("ctrl+a"); x.type("hi")
"""
import ctypes
import ctypes.util
import math
import subprocess
import time

_x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
_xt = ctypes.CDLL(ctypes.util.find_library("Xtst"))
_x11.XOpenDisplay.restype = ctypes.c_void_p
_x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
_x11.XStringToKeysym.restype = ctypes.c_ulong
_x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
_x11.XKeysymToKeycode.restype = ctypes.c_ubyte
_x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
_x11.XFlush.argtypes = [ctypes.c_void_p]
_x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
_xt.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
_xt.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
_xt.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]

SHIFTED = {'!': 'exclam', '#': 'numbersign', '@': 'at', '?': 'question', ':': 'colon', '"': 'quotedbl',
           '(': 'parenleft', ')': 'parenright', '_': 'underscore', '+': 'plus', '&': 'ampersand', '%': 'percent'}
PLAIN = {' ': 'space', '.': 'period', ',': 'comma', '-': 'minus', '/': 'slash', "'": 'apostrophe',
         ';': 'semicolon', '=': 'equal'}


class X:
    def __init__(self, display=":78"):
        self.name = display
        self.d = _x11.XOpenDisplay(display.encode())
        if not self.d:
            raise RuntimeError(f"cannot open {display}")
        self.pos = (960, 540)

    def _flush(self):
        _x11.XSync(self.d, 0)

    def warp(self, x, y):
        _xt.XTestFakeMotionEvent(self.d, -1, int(x), int(y), 0)
        self._flush()
        self.pos = (x, y)

    def move(self, x, y, dur=None):
        """Glide to (x, y) with ease-in-out at ~120 Hz."""
        x0, y0 = self.pos
        dist = math.hypot(x - x0, y - y0)
        dur = dur if dur is not None else min(0.85, 0.25 + dist / 2200)
        steps = max(2, int(dur * 120))
        for i in range(1, steps + 1):
            t = i / steps
            e = t * t * (3 - 2 * t)
            _xt.XTestFakeMotionEvent(self.d, -1, int(x0 + (x - x0) * e), int(y0 + (y - y0) * e), 0)
            self._flush()
            time.sleep(dur / steps)
        self.pos = (x, y)

    def click(self, x=None, y=None, button=1, pause=0.12):
        if x is not None:
            self.move(x, y)
            time.sleep(pause)
        _xt.XTestFakeButtonEvent(self.d, button, 1, 0)
        self._flush()
        time.sleep(0.07)
        _xt.XTestFakeButtonEvent(self.d, button, 0, 0)
        self._flush()

    def scroll(self, clicks, x=None, y=None):
        if x is not None:
            self.move(x, y)
        b = 5 if clicks > 0 else 4
        for _ in range(abs(clicks)):
            self.click(button=b, pause=0)
            time.sleep(0.05)

    def _code(self, name):
        ks = _x11.XStringToKeysym(name.encode())
        if not ks:
            raise ValueError(f"unknown key {name}")
        return _x11.XKeysymToKeycode(self.d, ks)

    def key(self, combo, hold=0.06):
        """'e', 'Escape', 'ctrl+a', 'shift+numbersign', 'ctrl+comma'."""
        parts = combo.split("+")
        mods = {"ctrl": "Control_L", "shift": "Shift_L", "alt": "Alt_L", "super": "Super_L"}
        codes = [self._code(mods.get(p, p)) for p in parts]
        for c in codes:
            _xt.XTestFakeKeyEvent(self.d, c, 1, 0)
            self._flush()
        time.sleep(hold)
        for c in reversed(codes):
            _xt.XTestFakeKeyEvent(self.d, c, 0, 0)
            self._flush()
        time.sleep(0.03)

    def type(self, text, cps=16):
        for ch in text:
            if ch in SHIFTED:
                self.key("shift+" + SHIFTED[ch], hold=0.02)
            elif ch in PLAIN:
                self.key(PLAIN[ch], hold=0.02)
            elif ch.isupper():
                self.key("shift+" + ch.lower(), hold=0.02)
            else:
                self.key(ch, hold=0.02)
            time.sleep(1 / cps)

    def shot(self, path):
        subprocess.run(["import", "-display", self.name, "-window", "root", path], check=True)
