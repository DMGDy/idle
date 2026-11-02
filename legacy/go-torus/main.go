// Compiled torus pane for the cross-pane scene. Same comet math as scene.py so
// comets still fly between this pane and the Python panes.
package main

import (
	"bufio"
	"encoding/json"
	"math"
	"os"
	"os/exec"
	"os/signal"
	"strconv"
	"sync"
	"syscall"
	"time"
	"unsafe"
)

type rect struct{ X, Y, Width, Height int }

var group = []string{"w1:p2", "w1:p3", "w1:p4"}

var (
	geoMu  sync.Mutex
	myRect *rect
	region [4]float64
	haveG  bool
)

func pollLayout(pane string) {
	for {
		out, err := exec.Command("herdr", "pane", "layout").Output()
		if err == nil {
			var r struct {
				Result struct {
					Layout struct {
						Panes []struct {
							PaneID string `json:"pane_id"`
							Rect   rect   `json:"rect"`
						} `json:"panes"`
					} `json:"layout"`
				} `json:"result"`
			}
			if json.Unmarshal(out, &r) == nil {
				m := map[string]rect{}
				for _, p := range r.Result.Layout.Panes {
					m[p.PaneID] = p.Rect
				}
				if me, ok := m[pane]; ok {
					x0, y0, x1, y1 := 1e9, 1e9, -1e9, -1e9
					for _, g := range group {
						if q, ok := m[g]; ok {
							x0 = math.Min(x0, float64(q.X))
							y0 = math.Min(y0, float64(q.Y))
							x1 = math.Max(x1, float64(q.X+q.Width))
							y1 = math.Max(y1, float64(q.Y+q.Height))
						}
					}
					geoMu.Lock()
					myRect, region, haveG = &me, [4]float64{x0, y0, x1, y1}, true
					geoMu.Unlock()
				}
			}
		}
		time.Sleep(2 * time.Second)
	}
}

func termSize() (int, int) {
	var ws struct{ Row, Col, X, Y uint16 }
	syscall.Syscall(syscall.SYS_IOCTL, os.Stdout.Fd(), syscall.TIOCGWINSZ, uintptr(unsafe.Pointer(&ws)))
	return int(ws.Col), int(ws.Row)
}

type rgb struct{ r, g, b float64 }

func hsv(h, s, v float64) rgb {
	h = math.Mod(h, 1)
	if h < 0 {
		h++
	}
	i := int(h * 6)
	f := h*6 - float64(i)
	p, q, t := v*(1-s), v*(1-f*s), v*(1-(1-f)*s)
	switch i % 6 {
	case 0:
		return rgb{v, t, p}
	case 1:
		return rgb{q, v, p}
	case 2:
		return rgb{p, v, t}
	case 3:
		return rgb{p, q, v}
	case 4:
		return rgb{t, p, v}
	}
	return rgb{v, p, q}
}

func bounce(v, t, ph, lo, hi float64) float64 {
	L := hi - lo
	if L <= 0 {
		return lo
	}
	u := math.Mod(v*t+ph*2*L, 2*L)
	if u < L {
		return lo + u
	}
	return lo + 2*L - u
}

var comets = [][5]float64{
	{23, 17, 0.0, 0.3, 0.02},
	{31, 13, 0.5, 0.7, 0.55},
	{17, 29, 0.2, 0.1, 0.13},
	{41, 11, 0.8, 0.5, 0.80},
}

func clamp8(x float64) uint32 {
	if x >= 1 {
		return 255
	}
	if x <= 0 {
		return 0
	}
	return uint32(x * 255)
}

// pack quantizes to keep the dim plasma from changing every cell every frame.
func pack(c rgb, q uint32) uint32 {
	r, g, b := clamp8(c.r)&^q, clamp8(c.g)&^q, clamp8(c.b)&^q
	return r<<16 | g<<8 | b
}

func main() {
	pane := os.Getenv("HERDR_PANE_ID")
	go pollLayout(pane)

	w := bufio.NewWriterSize(os.Stdout, 1<<20)
	w.WriteString("\x1b[?25l\x1b[2J")
	w.Flush()
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		<-sig
		os.Stdout.WriteString("\x1b[0m\x1b[?25h\x1b[2J\x1b[H")
		os.Exit(0)
	}()

	var img, bgBuf []rgb
	frame := 0
	var zb []float64
	var mask []bool
	var prev []uint64
	pw, ph := 0, 0
	num := make([]byte, 0, 16)
	itoa := func(n uint32) []byte { return strconv.AppendUint(num[:0], uint64(n), 10) }

	logPath := os.Getenv("TORUS_LOG")
	logFrames, logStart := 0, time.Now()
	for {
		start := time.Now()
		if logPath != "" {
			logFrames++
			if el := time.Since(logStart); el > 2*time.Second {
				os.WriteFile(logPath, []byte(strconv.FormatFloat(float64(logFrames)/el.Seconds(), 'f', 1, 64)+" fps\n"), 0o644)
				logFrames, logStart = 0, time.Now()
			}
		}
		cols, rows := termSize()
		if cols <= 0 || rows <= 0 {
			time.Sleep(100 * time.Millisecond)
			continue
		}
		W, H := cols, rows*2
		if W != pw || rows != ph {
			img, zb, mask = make([]rgb, W*H), make([]float64, W*H), make([]bool, W*H)
			bgBuf = make([]rgb, W*H)
			frame = 0
			prev = make([]uint64, cols*rows)
			for i := range prev {
				prev[i] = ^uint64(0)
			}
			pw, ph = W, rows
			w.WriteString("\x1b[2J")
		}
		t := math.Mod(float64(time.Now().UnixNano())/1e9, 10000)

		// plasma background: refreshed at ~6 fps, it is most of the output bytes
		cx, cy := float64(W)/2, float64(H)/2
		if frame%5 == 0 {
			for y := 0; y < H; y++ {
				fy := float64(y)
				sy := math.Sin(fy*0.1 - t*0.7)
				for x := 0; x < W; x++ {
					fx := float64(x)
					v := (math.Sin(fx*0.08+t) + sy + math.Sin(math.Hypot(fx-cx, fy-cy)*0.12-t*1.3)) / 3
					bgBuf[y*W+x] = hsv(v*0.5+t*0.03+0.55, 0.8, 0.10+0.08*(v+1))
				}
			}
		}
		frame++
		copy(img, bgBuf)
		for i := range zb {
			zb[i], mask[i] = 0, false
		}

		// torus, sampled densely enough for the pane's pixel size
		A, B := t*1.4, t*0.7
		cA, sA, cB, sB := math.Cos(A), math.Sin(A), math.Cos(B), math.Sin(B)
		K1 := math.Min(float64(W), float64(H)) * 5 * 3 / (8 * 3)
		nth := int(K1*0.6) + 40
		nph := int(K1*1.8) + 120
		for i := 0; i < nth; i++ {
			th := 2 * math.Pi * float64(i) / float64(nth)
			ct, st := math.Cos(th), math.Sin(th)
			for j := 0; j < nph; j++ {
				phi := 2 * math.Pi * float64(j) / float64(nph)
				cp, sp := math.Cos(phi), math.Sin(phi)
				ccx, ccy := 2+ct, st
				x := ccx*(cB*cp+sA*sB*sp) - ccy*cA*sB
				y := ccx*(sB*cp-sA*cB*sp) + ccy*cA*cB
				ooz := 1 / (5 + cA*ccx*sp + ccy*sA)
				xp, yp := int(cx+K1*ooz*x), int(cy-K1*ooz*y)
				if xp < 0 || xp >= W || yp < 0 || yp >= H {
					continue
				}
				k := yp*W + xp
				if ooz > zb[k] {
					zb[k] = ooz
					L := math.Max(cp*ct*sB-cA*ct*sp-sA*st+cB*(cA*st-ct*sA*sp), 0.05)
					img[k] = hsv(t*0.08+L*0.25+th/12.6, 0.7, math.Min(1, 0.25+L*0.75))
					mask[k] = true
				}
			}
		}

		// comets in global tab coordinates
		geoMu.Lock()
		mr, rg, ok := myRect, region, haveG
		geoMu.Unlock()
		if ok {
			ox := float64(mr.X + (mr.Width-cols)/2)
			oy := float64(mr.Y + (mr.Height-rows)/2)
			for _, c := range comets {
				for k := 0; k < 14; k++ {
					tk := t - float64(k)*0.035
					gx := bounce(c[0], tk, c[2], rg[0]+1, rg[2]-2)
					gy := bounce(c[1], tk, c[3], (rg[1]+1)*2, (rg[3]-1)*2-1)
					lx, ly := gx-ox, gy-oy*2
					rad, inten := 3.2, 1.0
					sat := 0.2
					if k > 0 {
						rad = 2.2*(1-float64(k)/14) + 0.4
						inten = 0.55 * (1 - float64(k)/14)
						sat = 0.6
					}
					col := hsv(c[4]+float64(k)*0.01, sat, 1)
					for yy := int(ly - rad - 1); yy < int(ly+rad+2); yy++ {
						if yy < 0 || yy >= H {
							continue
						}
						for xx := int(lx - rad - 1); xx < int(lx+rad+2); xx++ {
							if xx < 0 || xx >= W {
								continue
							}
							d := math.Hypot(float64(xx)-lx, float64(yy)-ly) / rad
							if d < 1 {
								g := (1 - d) * (1 - d) * inten
								p := &img[yy*W+xx]
								p.r += col.r * g
								p.g += col.g * g
								p.b += col.b * g
								mask[yy*W+xx] = true
							}
						}
					}
				}
			}
		}

		// emit only changed cells, reusing the current pen where possible
		w.WriteString("\x1b[?2026h")
		curFg, curBg := ^uint32(0), ^uint32(0)
		lastX, lastY := -2, -2
		for r := 0; r < rows; r++ {
			for x := 0; x < cols; x++ {
				ti, bi := (r*2)*W+x, (r*2+1)*W+x
				qt, qb := uint32(0x07), uint32(0x07)
				if mask[ti] {
					qt = 0
				}
				if mask[bi] {
					qb = 0
				}
				fg, bg := pack(img[ti], qt), pack(img[bi], qb)
				key := uint64(fg)<<32 | uint64(bg)
				if prev[r*cols+x] == key {
					continue
				}
				prev[r*cols+x] = key
				if !(r == lastY && x == lastX+1) {
					w.WriteString("\x1b[")
					w.Write(itoa(uint32(r + 1)))
					w.WriteByte(';')
					w.Write(itoa(uint32(x + 1)))
					w.WriteByte('H')
				}
				if fg != curFg {
					w.WriteString("\x1b[38;2;")
					w.Write(itoa(fg >> 16))
					w.WriteByte(';')
					w.Write(itoa(fg >> 8 & 255))
					w.WriteByte(';')
					w.Write(itoa(fg & 255))
					w.WriteByte('m')
					curFg = fg
				}
				if bg != curBg {
					w.WriteString("\x1b[48;2;")
					w.Write(itoa(bg >> 16))
					w.WriteByte(';')
					w.Write(itoa(bg >> 8 & 255))
					w.WriteByte(';')
					w.Write(itoa(bg & 255))
					w.WriteByte('m')
					curBg = bg
				}
				w.WriteString("▀")
				lastX, lastY = x, r
			}
		}
		w.WriteString("\x1b[0m\x1b[?2026l")
		w.Flush()

		if d := 33*time.Millisecond - time.Since(start); d > 0 {
			time.Sleep(d)
		}
	}
}
