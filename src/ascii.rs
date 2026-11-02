//! ASCII output for terminals without image support (e.g. a phone attached to herdr).
//! The GPU renders each cell as a 4x8 block; that block is reduced to a 2x4 coverage
//! pattern and matched against hand-made 2x4 outlines of ASCII glyphs, so edges come
//! out as / \ | _ ( ) and fills as dense glyphs instead of a plain brightness ramp.

use std::io::Write;

pub const SUB_X: u32 = 4;
pub const SUB_Y: u32 = 8;

// Coverage per glyph, rows top to bottom, [left, right] halves of each row.
#[rustfmt::skip]
const GLYPHS: &[(u8, [f32; 8])] = &[
    (b' ', [0.0, 0.0,  0.0, 0.0,  0.0, 0.0,  0.0, 0.0]),
    (b'.', [0.0, 0.0,  0.0, 0.0,  0.0, 0.0,  0.3, 0.3]),
    (b',', [0.0, 0.0,  0.0, 0.0,  0.0, 0.0,  0.35, 0.15]),
    (b'_', [0.0, 0.0,  0.0, 0.0,  0.0, 0.0,  0.8, 0.8]),
    (b'`', [0.3, 0.1,  0.0, 0.0,  0.0, 0.0,  0.0, 0.0]),
    (b'\'',[0.2, 0.2,  0.0, 0.0,  0.0, 0.0,  0.0, 0.0]),
    (b'^', [0.45, 0.45, 0.1, 0.1, 0.0, 0.0,  0.0, 0.0]),
    (b'"', [0.5, 0.5,  0.15, 0.15, 0.0, 0.0, 0.0, 0.0]),
    (b'-', [0.0, 0.0,  0.0, 0.0,  0.45, 0.45, 0.0, 0.0]),
    (b'~', [0.0, 0.0,  0.3, 0.4,  0.4, 0.3,  0.0, 0.0]),
    (b':', [0.0, 0.0,  0.3, 0.3,  0.0, 0.0,  0.3, 0.3]),
    (b';', [0.0, 0.0,  0.3, 0.3,  0.0, 0.0,  0.35, 0.25]),
    (b'=', [0.0, 0.0,  0.5, 0.5,  0.5, 0.5,  0.0, 0.0]),
    (b'+', [0.0, 0.0,  0.35, 0.35, 0.6, 0.6, 0.15, 0.15]),
    (b'*', [0.2, 0.2,  0.55, 0.55, 0.4, 0.4, 0.0, 0.0]),
    (b'|', [0.45, 0.45, 0.45, 0.45, 0.45, 0.45, 0.45, 0.45]),
    (b'/', [0.0, 0.65, 0.15, 0.5, 0.5, 0.15, 0.65, 0.0]),
    (b'\\',[0.65, 0.0, 0.5, 0.15, 0.15, 0.5, 0.0, 0.65]),
    (b'(', [0.05, 0.5, 0.5, 0.05, 0.5, 0.05, 0.05, 0.5]),
    (b')', [0.5, 0.05, 0.05, 0.5, 0.05, 0.5, 0.5, 0.05]),
    (b'[', [0.7, 0.3,  0.6, 0.0,  0.6, 0.0,  0.7, 0.3]),
    (b']', [0.3, 0.7,  0.0, 0.6,  0.0, 0.6,  0.3, 0.7]),
    (b'<', [0.0, 0.4,  0.45, 0.1, 0.45, 0.1, 0.0, 0.4]),
    (b'>', [0.4, 0.0,  0.1, 0.45, 0.1, 0.45, 0.4, 0.0]),
    (b'v', [0.0, 0.0,  0.4, 0.4,  0.4, 0.4,  0.2, 0.2]),
    (b'x', [0.0, 0.0,  0.45, 0.45, 0.4, 0.4, 0.45, 0.45]),
    (b'o', [0.0, 0.0,  0.5, 0.5,  0.5, 0.5,  0.5, 0.5]),
    (b'c', [0.0, 0.0,  0.55, 0.3, 0.55, 0.0, 0.55, 0.3]),
    (b'L', [0.6, 0.0,  0.6, 0.0,  0.6, 0.0,  0.8, 0.6]),
    (b'J', [0.0, 0.6,  0.0, 0.6,  0.0, 0.6,  0.6, 0.8]),
    (b'T', [0.85, 0.85, 0.35, 0.35, 0.35, 0.35, 0.35, 0.35]),
    (b'7', [0.8, 0.85, 0.0, 0.55, 0.25, 0.3, 0.5, 0.0]),
    (b'b', [0.6, 0.0,  0.75, 0.45, 0.7, 0.55, 0.75, 0.5]),
    (b'd', [0.0, 0.6,  0.45, 0.75, 0.55, 0.7, 0.5, 0.75]),
    (b'P', [0.75, 0.6, 0.75, 0.6, 0.7, 0.2, 0.6, 0.0]),
    (b'Y', [0.5, 0.5,  0.45, 0.45, 0.35, 0.35, 0.35, 0.35]),
    (b'a', [0.0, 0.0,  0.5, 0.6,  0.7, 0.7,  0.7, 0.75]),
    (b'e', [0.0, 0.0,  0.65, 0.6, 0.75, 0.6, 0.6, 0.55]),
    (b'O', [0.6, 0.6,  0.6, 0.6,  0.6, 0.6,  0.6, 0.6]),
    (b'0', [0.7, 0.7,  0.65, 0.7, 0.7, 0.65, 0.7, 0.7]),
    (b'$', [0.6, 0.7,  0.75, 0.5, 0.5, 0.75, 0.7, 0.6]),
    (b'%', [0.7, 0.45, 0.45, 0.75, 0.75, 0.45, 0.45, 0.7]),
    (b'&', [0.6, 0.5,  0.7, 0.4,  0.8, 0.7,  0.75, 0.8]),
    (b'#', [0.75, 0.75, 0.85, 0.85, 0.85, 0.85, 0.75, 0.75]),
    (b'M', [0.9, 0.9,  0.9, 0.9,  0.8, 0.8,  0.8, 0.8]),
    (b'@', [0.85, 0.85, 0.95, 0.95, 0.95, 0.95, 0.85, 0.85]),
];

// Flat cells: classic density ramp.
const RAMP: &[u8] = b" .:-=+*#%@";
// Glyphs whose outline is worth matching on edges (fills are left to the ramp).
const EDGE: &[u8] = b"/\\|_-()[]<>^v'`,.LJ7T\"~";
const EDGE_STD: f32 = 0.11;

fn pick(cov: &[f32; 8]) -> u8 {
    let mean = cov.iter().sum::<f32>() / 8.0;
    let var = cov.iter().map(|c| (c - mean) * (c - mean)).sum::<f32>() / 8.0;
    let std = var.sqrt();
    if mean < 0.06 {
        return b' ';
    }
    if std < EDGE_STD {
        let i = ((mean * (RAMP.len() as f32 - 0.01)) as usize).min(RAMP.len() - 1);
        return RAMP[i];
    }
    // shape only: compare mean-removed, unit-norm patterns (normalised correlation)
    let mut best = (f32::MIN, RAMP[((mean * 9.99) as usize).min(9)]);
    for (ch, gl) in GLYPHS {
        if !EDGE.contains(ch) {
            continue;
        }
        let gm = gl.iter().sum::<f32>() / 8.0;
        let (mut dot, mut gn) = (0.0, 0.0);
        for k in 0..8 {
            let g = gl[k] - gm;
            dot += (cov[k] - mean) * g;
            gn += g * g;
        }
        let score = dot / (gn.sqrt() * std * 8f32.sqrt() + 1e-6);
        if score > best.0 {
            best = (score, *ch);
        }
    }
    if best.0 < 0.55 {
        // no glyph outline fits well: fall back to density
        return RAMP[((mean * 9.99) as usize).min(9)];
    }
    best.1
}

#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: u8,
    pub color: u8,
}

pub struct Ascii {
    // auto-exposure: luminance range mapped onto the glyph ramp, eased over time
    lo: f32,
    hi: f32,
    lum: Vec<[f32; 8]>,
    rgb: Vec<[f32; 3]>,
    prev: Vec<Option<Cell>>,
    cols: u32,
    rows: u32,
    color: bool,
    out: Vec<u8>,
}

fn to256(r: f32, g: f32, b: f32) -> u8 {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    if mx - mn < 0.06 {
        // greys look nicer from the 24-step ramp than the cube's diagonal
        let l = ((r + g + b) / 3.0).clamp(0.0, 1.0);
        return 232 + (l * 23.0).round() as u8;
    }
    let q = |v: f32| (v.clamp(0.0, 1.0) * 5.0).round() as u8;
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

impl Ascii {
    pub fn new(color: bool) -> Self {
        Ascii { lo: 0.0, hi: 0.6, lum: Vec::new(), rgb: Vec::new(), prev: Vec::new(), cols: 0, rows: 0, color, out: Vec::with_capacity(1 << 16) }
    }

    /// Convert an RGBA frame of cols*SUB_X by rows*SUB_Y pixels and draw the cells that changed.
    pub fn draw(&mut self, px: &[u8], cols: u32, rows: u32) -> usize {
        if (cols, rows) != (self.cols, self.rows) {
            self.prev = vec![None; (cols * rows) as usize];
            self.cols = cols;
            self.rows = rows;
            self.out.extend_from_slice(b"\x1b[0m\x1b[2J");
        }
        let w = (cols * SUB_X) as usize;
        let mut pen: Option<u8> = None;
        let mut cursor: Option<(u32, u32)> = None;
        self.out.extend_from_slice(b"\x1b[?2026h");
        let n_cells = (cols * rows) as usize;
        self.lum.resize(n_cells, [0.0; 8]);
        self.rgb.resize(n_cells, [0.0; 3]);
        let mut hist = [0u32; 256];
        for cy in 0..rows {
            for cx in 0..cols {
                // 2x4 luminance from 2x2 pixel groups, plus the cell's mean colour
                let idx = (cy * cols + cx) as usize;
                let (mut sr, mut sg, mut sb) = (0f32, 0f32, 0f32);
                for gy in 0..4u32 {
                    for gx in 0..2u32 {
                        let mut l = 0.0;
                        for oy in 0..2u32 {
                            for ox in 0..2u32 {
                                let x = (cx * SUB_X + gx * 2 + ox) as usize;
                                let y = (cy * SUB_Y + gy * 2 + oy) as usize;
                                let i = (y * w + x) * 4;
                                let (r, g, b) = (px[i] as f32 / 255.0, px[i + 1] as f32 / 255.0, px[i + 2] as f32 / 255.0);
                                l += 0.2126 * r + 0.7152 * g + 0.0722 * b;
                                sr += r;
                                sg += g;
                                sb += b;
                            }
                        }
                        let l = l / 4.0;
                        self.lum[idx][(gy * 2 + gx) as usize] = l;
                        hist[((l * 255.0) as usize).min(255)] += 1;
                    }
                }
                let n = (SUB_X * SUB_Y) as f32;
                self.rgb[idx] = [sr / n, sg / n, sb / n];
            }
        }
        // exposure: 5th..99.5th percentile -> full ramp, eased so it doesn't pump
        let total = (n_cells * 8) as f32;
        let pct = |p: f32| {
            let mut acc = 0u32;
            for (b, c) in hist.iter().enumerate() {
                acc += c;
                if acc as f32 >= p * total {
                    return b as f32 / 255.0;
                }
            }
            1.0
        };
        let (lo, hi) = (pct(0.08), pct(0.985).max(pct(0.08) + 0.15));
        self.lo += (lo - self.lo) * 0.08;
        self.hi += (hi - self.hi) * 0.08;
        let (elo, ehi) = (self.lo, self.hi);

        for cy in 0..rows {
            for cx in 0..cols {
                let idx = (cy * cols + cx) as usize;
                let mut cov = [0f32; 8];
                for k in 0..8 {
                    cov[k] = ((self.lum[idx][k] - elo) / (ehi - elo)).clamp(0.0, 1.0).powf(1.35);
                }
                let ch = pick(&cov);
                // colour carries hue; the glyph already carries brightness
                let [r, g, b] = self.rgb[idx];
                // real colour, exposed like the glyphs and pushed away from grey
                let ex = |c: f32| ((c - elo * 0.5) / (ehi - elo * 0.5)).clamp(0.0, 1.0);
                let (r, g, b) = (ex(r), ex(g), ex(b));
                let l = (r + g + b) / 3.0;
                let sat = |c: f32| (l + (c - l) * 1.45).clamp(0.0, 1.0);
                let (r, g, b) = (sat(r), sat(g), sat(b));
                let mx = r.max(g).max(b).max(1e-4);
                let lift = (0.35 + 0.65 * mx) / mx; // dim cells stay visible but dim
                let color = if self.color { to256(r * lift, g * lift, b * lift) } else { 252 };
                let cell = Cell { ch, color };

                if self.prev[idx] == Some(cell) {
                    continue;
                }
                self.prev[idx] = Some(cell);
                if cursor != Some((cx, cy)) {
                    let _ = write!(self.out, "\x1b[{};{}H", cy + 1, cx + 1);
                }
                if self.color && pen != Some(color) {
                    let _ = write!(self.out, "\x1b[38;5;{}m", color);
                    pen = Some(color);
                }
                self.out.push(cell.ch);
                cursor = Some((cx + 1, cy));
            }
        }
        self.out.extend_from_slice(b"\x1b[0m\x1b[?2026l");
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(&self.out);
        let _ = so.flush();
        let n = self.out.len();
        self.out.clear();
        n
    }

    pub fn reset(&mut self) {
        self.cols = 0;
        self.rows = 0;
    }
}
