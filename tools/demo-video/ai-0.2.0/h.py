import sys, time, os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from xd import X
x = X(":78")
def s(n):
    x.shot(f"/tmp/vs-{n}.png"); print("shot", n)
