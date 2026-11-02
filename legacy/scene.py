#!/usr/bin/env python3
"""One canvas, three panes. Each pane renders its own world, and a shared set of
comets flies across all of them, positioned from wall-clock time in global tab
coordinates fetched from herdr. Usage: scene.py torus|synth|galaxy"""
import json, math, os, random, signal, subprocess, sys, time

MODE = sys.argv[1]
PANE = os.environ.get("HERDR_PANE_ID", "")
GROUP = ["w1:p2", "w1:p3", "w1:p4"]


def layout():
    try:
        out = subprocess.run(["herdr", "pane", "layout"], capture_output=True, text=True, timeout=2).stdout
        panes = {p["pane_id"]: p["rect"] for p in json.loads(out)["result"]["layout"]["panes"]}
        rects = [panes[g] for g in GROUP if g in panes]
        x0 = min(r["x"] for r in rects); y0 = min(r["y"] for r in rects)
        x1 = max(r["x"] + r["width"] for r in rects); y1 = max(r["y"] + r["height"] for r in rects)
        return panes.get(PANE), (x0, y0, x1, y1)
    except Exception:
        return None, None


def hsv(h, s, v):
    h %= 1.0; i = int(h * 6); f = h * 6 - i
    p, q, t = v * (1 - s), v * (1 - f * s), v * (1 - (1 - f) * s)
    return [(v, t, p), (q, v, p), (p, v, t), (p, q, v), (t, p, v), (v, p, q)][i % 6]


def lerp(a, b, k):
    return tuple(a[i] + (b[i] - a[i]) * k for i in range(3))


# ---------------------------------------------------------------- worlds
class Torus:
    def frame(self, W, H, t):
        img = [[None] * W for _ in range(H)]
        z = [[0.0] * W for _ in range(H)]
        A, B = t * 1.4, t * 0.7
        cA, sA, cB, sB = math.cos(A), math.sin(A), math.cos(B), math.sin(B)
        K1 = min(W, H) * 5 * 3 / (8 * 3)
        th = 0.0
        while th < 6.283:
            ct, st = math.cos(th), math.sin(th)
            ph = 0.0
            while ph < 6.283:
                cp, sp = math.cos(ph), math.sin(ph)
                cx, cy = 2 + ct, st
                x = cx * (cB * cp + sA * sB * sp) - cy * cA * sB
                y = cx * (sB * cp - sA * cB * sp) + cy * cA * cB
                ooz = 1 / (5 + cA * cx * sp + cy * sA)
                xp, yp = int(W / 2 + K1 * ooz * x), int(H / 2 - K1 * ooz * y)
                if 0 <= xp < W and 0 <= yp < H and ooz > z[yp][xp]:
                    z[yp][xp] = ooz
                    L = max(cp * ct * sB - cA * ct * sp - sA * st + cB * (cA * st - ct * sA * sp), 0.05)
                    img[yp][xp] = hsv(t * 0.08 + L * 0.25 + th / 12.6, 0.7, min(1, 0.25 + L * 0.75))
                ph += 0.035
            th += 0.1
        for y in range(H):
            row = img[y]
            for x in range(W):
                if row[x] is None:
                    v = (math.sin(x * 0.08 + t) + math.sin(y * 0.1 - t * 0.7)
                         + math.sin(math.hypot(x - W / 2, y - H / 2) * 0.12 - t * 1.3)) / 3
                    row[x] = hsv(v * 0.5 + t * 0.03 + 0.55, 0.8, 0.10 + 0.08 * (v + 1))
        return img


class Synth:
    def __init__(self):
        rnd = random.Random(7)
        self.ridge = [rnd.random() for _ in range(64)]

    def mountain(self, u):
        n = len(self.ridge); u *= n; i = int(u) % n; f = u - int(u)
        f = f * f * (3 - 2 * f)
        return self.ridge[i] * (1 - f) + self.ridge[(i + 1) % n] * f

    def frame(self, W, H, t):
        hz = int(H * 0.55)
        img = []
        cx, cy, R = W / 2, hz - 1, min(W, H) * 0.42
        for y in range(H):
            row = []
            for x in range(W):
                if y < hz:
                    k = y / hz
                    c = lerp((0.05, 0.0, 0.15), (0.9, 0.2, 0.5), k ** 2)
                    d = math.hypot(x - cx, y - cy)
                    if d < R and y < cy:
                        sk = (cy - y) / R
                        stripe = (y + int(t * 3)) % 6 < (5 if sk > 0.45 else 3 if sk > 0.2 else 2)
                        if stripe:
                            c = lerp((1.0, 0.85, 0.2), (1.0, 0.2, 0.55), 1 - sk)
                    m = 0.15 + 0.5 * self.mountain(x / W * 0.6 + t * 0.01) * (0.6 + 0.4 * self.mountain(x / W * 2.3 + 0.3))
                    if (hz - y) / hz < m * 0.55:
                        c = lerp((0.08, 0.0, 0.18), (0.25, 0.0, 0.35), (hz - y) / hz / max(m, 0.01))
                    if (x * 7 + y * 13) % 97 == 0 and y < hz * 0.5:
                        c = (0.8, 0.8, 1.0) if math.sin(t * 3 + x) > -0.5 else c
                else:
                    dz = (y - hz + 1) / (H - hz)
                    depth = 1 / dz
                    zline = (depth * 2 + t * 4) % 1
                    xw = (x - cx) * depth / W * 6
                    xline = abs((xw + 0.5) % 1 - 0.5)
                    glow = max(0, 1 - zline * 6 * dz ** -0.3) + max(0, 1 - xline * 9 / depth ** 0.2)
                    glow = min(glow, 1.2)
                    base = (0.03, 0.0, 0.08)
                    c = lerp(base, (0.1, 0.9, 1.0) if glow > 0.6 else (0.9, 0.1, 0.8), min(1, glow) * min(1, dz * 2.5))
                row.append(c)
            img.append(row)
        return img


class Galaxy:
    def __init__(self):
        rnd = random.Random(42)
        self.stars = []
        for i in range(1400):
            arm = i % 3
            r = rnd.random() ** 1.6
            a = arm * 2.094 + r * 5.5 + rnd.gauss(0, 0.28 * (1.1 - r))
            hue = 0.6 - r * 0.5 + rnd.gauss(0, 0.05)
            self.stars.append((r, a, hue, rnd.random() * 6.28, 0.5 + rnd.random() * 0.8))

    def frame(self, W, H, t):
        img = [[[0.0, 0.0, 0.03] for _ in range(W)] for _ in range(H)]
        cx, cy, S = W / 2, H / 2, min(W / 2, H) * 0.95
        tilt = 0.55 + 0.15 * math.sin(t * 0.2)
        for r, a, hue, ph, b in self.stars:
            ang = a + t * 0.6 / (0.25 + r)  # differential rotation
            x = cx + math.cos(ang) * r * S
            y = cy + math.sin(ang) * r * S * tilt
            xi, yi = int(x), int(y)
            if 0 <= xi < W and 0 <= yi < H:
                br = b * (0.6 + 0.4 * math.sin(t * 4 + ph))
                cr, cg, cb = hsv(hue, 0.55, br)
                p = img[yi][xi]
                p[0] += cr * 0.6; p[1] += cg * 0.6; p[2] += cb * 0.6
        for y in range(H):
            for x in range(W):
                d = math.hypot((x - cx) / (S * 0.5), (y - cy) / (S * 0.5 * tilt))
                core = math.exp(-d * d * 18) * 1.3
                p = img[y][x]
                p[0] += core; p[1] += core * 0.8; p[2] += core * 0.5
        return img


# ---------------------------------------------------------------- comets
COMETS = [
    # (speed x, speed y, phase x, phase y, hue)
    (23.0, 17.0, 0.0, 0.3, 0.02),
    (31.0, 13.0, 0.5, 0.7, 0.55),
    (17.0, 29.0, 0.2, 0.1, 0.13),
    (41.0, 11.0, 0.8, 0.5, 0.80),
]


def bounce(v, t, ph, lo, hi):
    L = hi - lo
    if L <= 0:
        return lo
    u = (v * t + ph * 2 * L) % (2 * L)
    return lo + (u if u < L else 2 * L - u)


def comet_overlay(img, W, H, t, ox, oy, region):
    """ox, oy: global position of this pane's top-left cell. Pixels are cell-x, half-cell-y."""
    x0, y0, x1, y1 = region
    for vx, vy, px, py, hue in COMETS:
        for k in range(14):  # trail samples
            tk = t - k * 0.035
            gx = bounce(vx, tk, px, x0 + 1, x1 - 2)
            gy = bounce(vy, tk, py, (y0 + 1) * 2, (y1 - 1) * 2 - 1)
            lx, ly = gx - ox, gy - oy * 2
            rad = 3.2 if k == 0 else 2.2 * (1 - k / 14) + 0.4
            inten = 1.0 if k == 0 else 0.55 * (1 - k / 14)
            col = hsv(hue + k * 0.01, 0.6 if k else 0.2, 1.0)
            for yy in range(int(ly - rad - 1), int(ly + rad + 2)):
                if not 0 <= yy < H:
                    continue
                for xx in range(int(lx - rad - 1), int(lx + rad + 2)):
                    if not 0 <= xx < W:
                        continue
                    d = math.hypot(xx - lx, yy - ly) / rad
                    if d < 1:
                        g = (1 - d) ** 2 * inten
                        p = img[yy][xx]
                        img[yy][xx] = (p[0] + col[0] * g, p[1] + col[1] * g, p[2] + col[2] * g)


# ---------------------------------------------------------------- main loop
def main():
    world = {"torus": Torus, "synth": Synth, "galaxy": Galaxy}[MODE]()
    out = sys.stdout
    out.write("\x1b[?25l\x1b[2J")

    def bye(*_):
        out.write("\x1b[0m\x1b[?25h\x1b[2J\x1b[H"); out.flush(); sys.exit(0)
    signal.signal(signal.SIGINT, bye)
    signal.signal(signal.SIGTERM, bye)

    rect = region = None
    last_layout = 0
    while True:
        now = time.time()
        if now - last_layout > 2:
            nr, ng = layout()
            if nr:
                rect, region = nr, ng
            last_layout = now
        cols, rows = os.get_terminal_size()
        W, H = cols, rows * 2
        img = world.frame(W, H, now % 10000)
        if rect and region:
            ox = rect["x"] + (rect["width"] - cols) // 2
            oy = rect["y"] + (rect["height"] - rows) // 2
            comet_overlay(img, W, H, now % 10000, ox, oy, region)
        buf = ["\x1b[H"]
        for r in range(rows):
            top, bot = img[r * 2], img[r * 2 + 1]
            line = []
            for x in range(W):
                a, b = top[x], bot[x]
                line.append("\x1b[38;2;%d;%d;%dm\x1b[48;2;%d;%d;%dm▀" % (
                    min(255, int(a[0] * 255)), min(255, int(a[1] * 255)), min(255, int(a[2] * 255)),
                    min(255, int(b[0] * 255)), min(255, int(b[1] * 255)), min(255, int(b[2] * 255))))
            buf.append("".join(line) + "\x1b[0m" + ("\n" if r < rows - 1 else ""))
        out.write("".join(buf)); out.flush()
        time.sleep(max(0.0, 0.033 - (time.time() - now)))


main()
