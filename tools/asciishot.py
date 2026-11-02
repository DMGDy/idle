# Render the three panes' live ANSI output into an HTML page laid out like the tab, then screenshot it.
import json, subprocess, re, html, sys, os
lay = {p["pane_id"]: p["rect"] for p in json.loads(subprocess.run(["herdr","pane","layout"],capture_output=True,text=True).stdout)["result"]["layout"]["panes"]}
panes = ["w1:p2","w1:p3","w1:p4"]
x0 = min(lay[p]["x"] for p in panes); y0 = min(lay[p]["y"] for p in panes)
def c256(n):
    if n < 16:
        base=[(0,0,0),(205,0,0),(0,205,0),(205,205,0),(0,0,238),(205,0,205),(0,205,205),(229,229,229),(127,127,127),(255,0,0),(0,255,0),(255,255,0),(92,92,255),(255,0,255),(0,255,255),(255,255,255)]
        return base[n]
    if n >= 232: v = 8 + (n-232)*10; return (v,v,v)
    n -= 16; lv=[0,95,135,175,215,255]
    return (lv[n//36], lv[(n//6)%6], lv[n%6])
CW, CH = 9, 18
out = []
for p in panes:
    r = lay[p]
    txt = subprocess.run(["herdr","pane","read",p,"--source","visible","--format","ansi"],capture_output=True,text=True).stdout
    rows = []
    for line in txt.split("\n"):
        cur = (200,200,200); spans = []
        for tok in re.split(r"(\x1b\[[0-9;]*m)", line):
            m = re.fullmatch(r"\x1b\[([0-9;]*)m", tok)
            if m:
                ps = [int(x) for x in m.group(1).split(";") if x]
                if ps[:2] == [38,5]: cur = c256(ps[2])
                elif ps == [0] or not ps: cur = (200,200,200)
            elif tok:
                spans.append(f'<span style="color:rgb{cur}">{html.escape(tok)}</span>')
        rows.append("".join(spans))
    out.append(f'<pre style="position:absolute;left:{(r["x"]+1-x0)*CW}px;top:{(r["y"]+1-y0)*CH}px;margin:0">' + "\n".join(rows) + "</pre>")
W = (max(lay[p]["x"]+lay[p]["width"] for p in panes)-x0)*CW
H = (max(lay[p]["y"]+lay[p]["height"] for p in panes)-y0)*CH
page = f'''<html><body style="margin:0;background:#000;width:{W}px;height:{H}px;position:relative;
font:{CH-3}px/{CH}px 'DejaVu Sans Mono',monospace;letter-spacing:{CW-8.7:.1f}px">{"".join(out)}</body></html>'''
open("asciishot.html","w").write(page)
outpng = sys.argv[1] if len(sys.argv) > 1 else "asciishot.png"
subprocess.run(["chromium","--headless=new","--disable-gpu","--no-sandbox","--hide-scrollbars",f"--user-data-dir={os.getcwd()}/chrome-prof",
                f"--window-size={W},{H}",f"--screenshot={os.getcwd()}/{outpng}",f"file://{os.getcwd()}/asciishot.html"],capture_output=True,timeout=60)
print("wrote", outpng, W, H)
