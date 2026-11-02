#!/usr/bin/env python3
"""Truecolor spinning torus over a drifting plasma field. Ctrl-C to quit."""
import math, os, sys, time, signal

def size():
    c, r = os.get_terminal_size()
    return c, r - 1

def hsv(h, s, v):
    i = int(h * 6) % 6; f = h * 6 - int(h * 6)
    p, q, t = v * (1 - s), v * (1 - f * s), v * (1 - (1 - f) * s)
    r, g, b = [(v, t, p), (q, v, p), (p, v, t), (p, q, v), (t, p, v), (v, p, q)][i]
    return int(r * 255), int(g * 255), int(b * 255)

def main():
    sys.stdout.write("\x1b[?25l\x1b[2J")
    signal.signal(signal.SIGINT, lambda *_: (sys.stdout.write("\x1b[0m\x1b[?25h\x1b[2J\x1b[H"), sys.exit(0)))
    A = B = 0.0
    t0 = time.time()
    while True:
        W, H = size()
        H2 = H * 2  # half-block rendering: two pixels per cell vertically
        t = time.time() - t0
        z = [[0.0] * W for _ in range(H2)]
        lum = [[-1.0] * W for _ in range(H2)]
        cA, sA, cB, sB = math.cos(A), math.sin(A), math.cos(B), math.sin(B)
        R1, R2, K2 = 1.0, 2.0, 5.0
        K1 = min(W, H2) * K2 * 3 / (8 * (R1 + R2))
        th = 0.0
        while th < 6.283:
            ct, st = math.cos(th), math.sin(th)
            ph = 0.0
            while ph < 6.283:
                cp, sp = math.cos(ph), math.sin(ph)
                cx, cy = R2 + R1 * ct, R1 * st
                x = cx * (cB * cp + sA * sB * sp) - cy * cA * sB
                y = cx * (sB * cp - sA * cB * sp) + cy * cA * cB
                zz = K2 + cA * cx * sp + cy * sA
                ooz = 1 / zz
                xp = int(W / 2 + K1 * ooz * x * 0.5 * 2)
                yp = int(H2 / 2 - K1 * ooz * y)
                L = cp * ct * sB - cA * ct * sp - sA * st + cB * (cA * st - ct * sA * sp)
                if 0 <= xp < W and 0 <= yp < H2 and ooz > z[yp][xp]:
                    z[yp][xp] = ooz
                    lum[yp][xp] = max(L, 0.05)
                ph += 0.03
            th += 0.09

        def px(x, y):
            l = lum[y][x]
            if l >= 0:
                return hsv((t * 0.08 + l * 0.25) % 1, 0.75, min(1, 0.25 + l * 0.75))
            v = (math.sin(x * 0.08 + t) + math.sin(y * 0.1 - t * 0.7)
                 + math.sin((x + y) * 0.05 + t * 0.5) + math.sin(math.hypot(x - W / 2, y - H2 / 2) * 0.12 - t * 1.3)) / 4
            return hsv((v * 0.5 + t * 0.03 + 0.55) % 1, 0.8, 0.12 + 0.1 * (v + 1))

        out = ["\x1b[H"]
        for row in range(H):
            line = []
            for x in range(W):
                top, bot = px(x, row * 2), px(x, row * 2 + 1)
                line.append("\x1b[38;2;%d;%d;%dm\x1b[48;2;%d;%d;%dm▀" % (*top, *bot))
            out.append("".join(line) + "\x1b[0m\n")
        title = " ◐ herdr × claude "
        out.append("\x1b[1;38;2;255;200;120m" + title.center(W)[:W] + "\x1b[0m")
        sys.stdout.write("".join(out)); sys.stdout.flush()
        A += 0.07; B += 0.035
        time.sleep(0.02)

main()
