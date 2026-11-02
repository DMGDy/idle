import json, subprocess, zlib, struct, sys
lay = json.loads(subprocess.run(["herdr","pane","layout"],capture_output=True,text=True).stdout)["result"]["layout"]["panes"]
rects = {p["pane_id"]: p["rect"] for p in lay}
panes = {"w1:p2":"torus","w1:p3":"synth","w1:p4":"galaxy"}
x0 = min(rects[p]["x"] for p in panes); y0 = min(rects[p]["y"] for p in panes)
x1 = max(rects[p]["x"]+rects[p]["width"] for p in panes); y1 = max(rects[p]["y"]+rects[p]["height"] for p in panes)
PX, PY = 8, 16                      # output px per cell (keeps the 13x25 cell aspect roughly)
W, H = (x1-x0)*PX, (y1-y0)*PY
img = bytearray(b"\x30\x30\x30" * (W*H))   # grey = pane borders
for p, m in panes.items():
    head, data = open(f"dump-{m}.raw","rb").read().split(b"\n",1)
    iw, ih, t = head.split(); iw, ih = int(iw), int(ih)
    r = rects[p]
    # content area: 1-cell border inset on each side
    cx, cy, cw, ch = r["x"]+1-x0, r["y"]+1-y0, r["width"]-2, r["height"]-2
    ox, oy, ow, oh = cx*PX, cy*PY, cw*PX, ch*PY
    for y in range(oh):
        sy = y*ih//oh
        row = data[sy*iw*3:(sy+1)*iw*3]
        base = ((oy+y)*W + ox)*3
        for x in range(ow):
            sx = x*iw//ow*3
            img[base+x*3:base+x*3+3] = row[sx:sx+3]
    print(m, iw, ih, "t=", t.decode())
raw = b"".join(b"\x00" + bytes(img[y*W*3:(y+1)*W*3]) for y in range(H))
png = b"\x89PNG\r\n\x1a\n"
def chunk(k, d): return struct.pack(">I", len(d)) + k + d + struct.pack(">I", zlib.crc32(k+d) & 0xffffffff)
png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
open(sys.argv[1] if len(sys.argv) > 1 else "composite.png","wb").write(png)
print("wrote", W, "x", H)
