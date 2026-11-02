struct U {
    w: u32, h: u32, cols: f32, rows: f32,
    t: f32, n_samples: u32, mode: u32, nworlds: u32,
    // shared space: tab coords in cells (x) and half-cells (y)
    region: vec4<f32>,             // x0, y0, x1, y1 of the three panes together
    me: vec4<f32>,                 // this pane's content origin + size
    worlds: array<vec4<f32>, 3>,   // each pane's content origin + size
    wmode: vec4<u32>,              // xyz: scene shown in each world; w bit0: ascii output
};

// Comet trail sample in local cell space (x = cell, y = half-cell), radius, intensity; colour.
struct Sample { pos: vec4<f32>, col: vec4<f32> };

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read_write> outp: array<u32>;
@group(0) @binding(2) var<storage, read> samples: array<Sample>;

// ------------------------------------------------------------------ helpers
fn hsv(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - 3.0);
    return v * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

// integer hashes: stable at large coordinates, unlike sin-based ones
fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
fn h2(p: vec2<i32>) -> f32 {
    return f32(pcg(bitcast<u32>(p.x) ^ pcg(bitcast<u32>(p.y)))) / 4294967295.0;
}
fn h3(p: vec3<i32>) -> f32 {
    return f32(pcg(bitcast<u32>(p.x) ^ pcg(bitcast<u32>(p.y) ^ pcg(bitcast<u32>(p.z))))) / 4294967295.0;
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = h2(i); let b = h2(i + vec2<i32>(1, 0));
    let c = h2(i + vec2<i32>(0, 1)); let d = h2(i + vec2<i32>(1, 1));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}
fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0; var a = 0.5; var v = 0.0;
    for (var i = 0; i < 5; i++) {
        v += a * vnoise(p);
        p = vec2<f32>(p.x * 1.6 - p.y * 1.2, p.x * 1.2 + p.y * 1.6);
        a *= 0.5;
    }
    return v;
}

// ------------------------------------------------------------------ torus
fn rot(p: vec3<f32>, a: f32, b: f32) -> vec3<f32> {
    let ca = cos(a); let sa = sin(a); let cb = cos(b); let sb = sin(b);
    let q = vec3<f32>(p.x, ca * p.y - sa * p.z, sa * p.y + ca * p.z);
    return vec3<f32>(cb * q.x - sb * q.y, sb * q.x + cb * q.y, q.z);
}
fn sd_torus(p: vec3<f32>, a: f32, b: f32) -> f32 {
    let r = rot(p, a, b);
    let q = vec2<f32>(length(r.xz) - 2.0, r.y);
    return length(q) - 1.0;
}

fn plasma(l: vec2<f32>, size: vec2<f32>) -> vec3<f32> {
    let t = u.t;
    let v = (sin(l.x * 0.08 + t) + sin(l.y * 0.1 - t * 0.7)
        + sin(length(l - size / 2.0) * 0.12 - t * 1.3)) / 3.0;
    return hsv(v * 0.5 + t * 0.03 + 0.55, 0.8, 0.10 + 0.08 * (v + 1.0));
}

struct TorusHit { col: vec3<f32>, hit: bool, n: vec3<f32>, rd: vec3<f32> };

// ldir: direction to the shared sun in this world's view space
fn torus_basic(uv: vec2<f32>, l: vec2<f32>, size: vec2<f32>, ldir: vec3<f32>) -> TorusHit {
    let t = u.t;
    var out: TorusHit;
    out.col = plasma(l, size);
    out.hit = false;
    let ro = vec3<f32>(0.0, 0.0, -5.0);
    let rd = normalize(vec3<f32>(uv, 1.25));
    out.rd = rd;
    let A = t * 1.4; let B = t * 0.7;
    var d = 0.0;
    for (var i = 0; i < 96; i++) {
        let s = sd_torus(ro + rd * d, A, B);
        if (s < 0.001) { out.hit = true; break; }
        d += s;
        if (d > 12.0) { break; }
    }
    if (!out.hit) { return out; }
    let p = ro + rd * d;
    let e = 0.0015;
    let n = normalize(vec3<f32>(
        sd_torus(p + vec3<f32>(e, 0.0, 0.0), A, B) - sd_torus(p - vec3<f32>(e, 0.0, 0.0), A, B),
        sd_torus(p + vec3<f32>(0.0, e, 0.0), A, B) - sd_torus(p - vec3<f32>(0.0, e, 0.0), A, B),
        sd_torus(p + vec3<f32>(0.0, 0.0, e), A, B) - sd_torus(p - vec3<f32>(0.0, 0.0, e), A, B)));
    out.n = n;
    let L = max(dot(n, ldir), 0.0);
    let r = rot(p, A, B);
    let th = atan2(r.y, length(r.xz) - 2.0);
    let base = hsv(t * 0.08 + th / 12.6, 0.55, 0.85);
    let hv = normalize(ldir - rd);
    let spec = pow(max(dot(n, hv), 0.0), 64.0) * 1.2;
    out.col = base * (0.12 + 0.95 * L * SUN) + SUN * spec;
    return out;
}

// ------------------------------------------------------------------ synthwave flyover
fn terrain(x: f32, z: f32) -> f32 {
    let valley = smoothstep(1.2, 7.0, abs(x));
    let n = fbm(vec2<f32>(x * 0.13, z * 0.13));
    return valley * (n * n * 9.0 + abs(x) * 0.15) - 0.02;
}

struct SynthHit { col: vec3<f32>, refl: f32 };

fn synth_basic(uv: vec2<f32>) -> SynthHit {
    let t = u.t;
    var out: SynthHit;
    out.refl = 0.0;
    let zcam = t * 6.0;
    let sway = sin(t * 0.25) * 0.8;
    let ro = vec3<f32>(sway, 1.1 + 0.15 * sin(t * 0.4), zcam);
    let rd = normalize(vec3<f32>(uv.x + sin(t * 0.25) * 0.08, uv.y - 0.12, 1.6));

    // sky: deep purple -> hot pink at the horizon, stars up high
    let hz = clamp(rd.y * 3.0 + 0.25, 0.0, 1.0);
    var sky = mix(vec3<f32>(1.0, 0.25, 0.55), vec3<f32>(0.06, 0.0, 0.16), pow(hz, 0.6));
    let sp = vec2<i32>(floor(rd.xy * 260.0));
    let st = h2(sp + vec2<i32>(17, 91));
    if (st > 0.996 && rd.y > 0.08) {
        sky += vec3<f32>(0.8, 0.8, 1.0) * (0.5 + 0.5 * sin(t * 3.0 + st * 600.0));
    }
    // striped retro sun
    let sdir = normalize(vec3<f32>(0.0, 0.1, 1.0));
    let sd = acos(clamp(dot(rd, sdir), -1.0, 1.0));
    let sr = 0.32;
    if (sd < sr) {
        let k = (rd.y - (sdir.y - sr)) / (2.0 * sr); // 0 bottom .. 1 top
        let band = fract(k * 9.0 - t * 0.6);
        if (k > 0.5 || band > 0.55 - (0.5 - k) * 0.9) {
            sky = mix(vec3<f32>(1.0, 0.15, 0.55), vec3<f32>(1.0, 0.85, 0.25), k);
        }
    }
    sky += vec3<f32>(1.0, 0.3, 0.6) * exp(-sd * 3.0) * 0.35; // sun glow

    // heightfield raymarch
    var d = 0.05;
    var hit = false;
    var p = ro;
    for (var i = 0; i < 140; i++) {
        p = ro + rd * d;
        let h = p.y - terrain(p.x, p.z);
        if (h < 0.002 * d) { hit = true; break; }
        d += max(h * 0.45, 0.01 + d * 0.004);
        if (d > 90.0) { break; }
    }
    if (!hit) { out.col = sky; return out; }

    let e = 0.05;
    let n = normalize(vec3<f32>(
        terrain(p.x - e, p.z) - terrain(p.x + e, p.z), 2.0 * e,
        terrain(p.x, p.z - e) - terrain(p.x, p.z + e)));
    // text cells average 4x8 pixels: thicker lines survive the downsample
    let w = (0.025 + d * 0.0025) * select(1.0, 2.6, (u.wmode.w & 1u) == 1u);
    let gx = abs(fract(p.x * 0.5) - 0.5);
    let gz = abs(fract(p.z * 0.5) - 0.5);
    let line = max(1.0 - smoothstep(0.0, w, gx), 1.0 - smoothstep(0.0, w, gz));
    let elev = clamp(p.y / 4.0, 0.0, 1.0);
    let neon = mix(vec3<f32>(0.1, 0.9, 1.0), vec3<f32>(1.0, 0.2, 0.85), elev);
    let shade = 0.25 + 0.75 * max(dot(n, normalize(vec3<f32>(0.0, 0.4, 1.0))), 0.0);
    var col = vec3<f32>(0.05, 0.0, 0.1) * shade + neon * line * 1.4;
    let fog = 1.0 - exp(-d * 0.028);
    col = mix(col, vec3<f32>(0.55, 0.12, 0.45), fog);
    out.col = col;
    // the flat valley floor is wet: it mirrors whatever is above it
    out.refl = (1.0 - smoothstep(0.02, 0.25, p.y)) * (1.0 - line) * (1.0 - fog) * 0.75;
    return out;
}

// ------------------------------------------------------------------ volumetric galaxy
// x: arm emission, y: core glow, z: dust
fn galaxy_density(p: vec3<f32>, t: f32) -> vec3<f32> {
    let r = length(p.xz);
    let th = atan2(p.z, p.x) + t * 0.15;
    let spiral = cos(2.0 * (th - 2.2 * log(r + 0.08)));
    let arm = pow(0.5 + 0.5 * spiral, 5.0);
    let lane = pow(0.5 + 0.5 * cos(2.0 * (th - 2.2 * log(r + 0.08)) - 0.9), 10.0);
    let thick = 0.06 + 0.05 * r;
    let disk = exp(-r / 1.4) * exp(-abs(p.y) / thick);
    let clump = 0.6 + 0.8 * vnoise(vec2<f32>(r * 6.0, th * 4.0));
    let core = exp(-(r * r + p.y * p.y * 6.0) * 6.0) * 6.0;
    return vec3<f32>((0.08 + arm * clump) * disk * 2.2, core, lane * disk * 3.0);
}

fn scene_galaxy(uv: vec2<f32>) -> vec3<f32> {
    let t = u.t;
    let a = t * 0.08;
    let e = 0.38 + 0.22 * sin(t * 0.11);
    let dist = 5.2;
    let ro = dist * vec3<f32>(cos(e) * sin(a), sin(e), -cos(e) * cos(a));
    let fwd = normalize(-ro);
    let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), fwd));
    let up = cross(fwd, right);
    let rd = normalize(fwd * 1.6 + right * uv.x + up * uv.y);

    // background stars
    var col = vec3<f32>(0.004, 0.003, 0.015);
    let sp = vec3<i32>(floor(rd * 180.0));
    let s = h3(sp);
    if (s > 0.9965) {
        col += hsv(s * 40.0, 0.3, 0.6 + 0.4 * sin(t * 2.0 + s * 900.0));
    }

    // march the disk slab
    let slab = 0.7;
    var t0 = 0.0; var t1 = 12.0;
    if (abs(rd.y) > 1e-4) {
        let ta = (slab - ro.y) / rd.y; let tb = (-slab - ro.y) / rd.y;
        t0 = max(min(ta, tb), 0.0); t1 = min(max(ta, tb), 12.0);
    } else if (abs(ro.y) > slab) {
        return col;
    }
    if (t1 <= t0) { return col; }
    let steps = 72;
    let dt = (t1 - t0) / f32(steps);
    var trans = 1.0;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < steps; i++) {
        let p = ro + rd * (t0 + (f32(i) + 0.5) * dt);
        let r = length(p.xz);
        if (r > 4.5) { continue; }
        let dn = galaxy_density(p, t);
        let emit_col = mix(vec3<f32>(1.0, 0.85, 0.6), vec3<f32>(0.45, 0.6, 1.0), smoothstep(0.2, 1.8, r));
        let pink = vec3<f32>(1.0, 0.35, 0.6) * pow(dn.x, 2.0) * 1.5; // star-forming knots
        acc += trans * (emit_col * dn.x + pink + vec3<f32>(1.0, 0.9, 0.7) * min(dn.y, 6.0) * 0.5) * dt * 2.2;
        // stars embedded in the disk
        let sc = vec3<i32>(floor(p * 28.0));
        let sh = h3(sc);
        if (sh > 0.985) {
            acc += trans * hsv(0.55 + sh * 3.0, 0.35, 1.0) * dn.x * 4.0 * dt;
        }
        trans *= exp(-dn.z * dt * 1.2);
    }
    col = col * trans + acc;
    return col / (1.0 + col * 0.35); // soft tone map
}

// ------------------------------------------------------------------ shared space
const SUN: vec3<f32> = vec3<f32>(1.0, 0.62, 0.45);
const SHOCK_PERIOD: f32 = 15.0;
const SEAM: f32 = 9.0; // border blend width, cells

fn ascii_out() -> bool { return (u.wmode.w & 1u) == 1u; }

fn world_uv(l: vec2<f32>, size: vec2<f32>) -> vec2<f32> {
    let m = min(size.x, size.y) / 2.0;
    return vec2<f32>((l.x - size.x / 2.0) / m, -(l.y - size.y / 2.0) / m);
}

fn rect_dist(g: vec2<f32>, w: vec4<f32>) -> f32 {
    let q = max(max(w.xy - g, g - (w.xy + w.zw)), vec2<f32>(0.0));
    return length(q);
}
fn rect_rect_dist(a: vec4<f32>, b: vec4<f32>) -> f32 {
    let q = max(max(a.xy - (b.xy + b.zw), b.xy - (a.xy + a.zw)), vec2<f32>(0.0));
    return length(q);
}

// Which pane's world owns tab point g: the containing pane, else the nearest one.
fn world_at(g: vec2<f32>) -> u32 {
    var best = 0u;
    var bd = 1e9;
    for (var k = 0u; k < u.nworlds; k++) {
        let dd = rect_dist(g, u.worlds[k]);
        if (dd < bd) { bd = dd; best = k; }
    }
    return best;
}

fn world_of_mode(mode: u32) -> i32 {
    for (var k = 0u; k < u.nworlds; k++) {
        if (u.wmode[k] == mode) { return i32(k); }
    }
    return -1;
}

// The synthwave sun, located in tab coords: it lights every world.
fn sun_tab() -> vec2<f32> {
    let k = world_of_mode(1u);
    if (k < 0) { return vec2<f32>(-1e4, -1e4); }
    let w = u.worlds[k];
    let m = min(w.z, w.w) / 2.0;
    // where synth's camera ray hits the sun direction (see synth_basic)
    let suv = vec2<f32>(-sin(u.t * 0.25) * 0.08, 0.28);
    return w.xy + vec2<f32>(suv.x * m + w.z / 2.0, -suv.y * m + w.w / 2.0);
}

// Direction to the sun in the view space of a world centred at c (y up, camera looks +z).
fn sun_dir_from(c: vec2<f32>) -> vec3<f32> {
    let dv = sun_tab() - c;
    return normalize(vec3<f32>(dv.x, -dv.y, -0.45 * length(dv)));
}

// Cheap render of whatever world is at tab point g (no reflections: WGSL forbids recursion).
fn env(g: vec2<f32>) -> vec3<f32> {
    let k = world_at(g);
    let w = u.worlds[k];
    let l = g - w.xy;
    let uv = world_uv(l, w.zw);
    let mode = u.wmode[k];
    if (mode == 1u) { return synth_basic(uv).col; }
    if (mode == 2u) { return scene_galaxy(uv); }
    return torus_basic(uv, l, w.zw, sun_dir_from(w.xy + w.zw / 2.0)).col;
}

// Full render of world k at tab point g, with cross-pane reflections.
fn render_full(k: u32, g: vec2<f32>) -> vec3<f32> {
    let w = u.worlds[k];
    let l = g - w.xy;
    let uv = world_uv(l, w.zw);
    let mode = u.wmode[k];
    let span = u.region.zw - u.region.xy;
    if (mode == 1u) {
        let sh = synth_basic(uv);
        var col = sh.col;
        if (sh.refl > 0.001) {
            // mirror across the horizon line: below it, the pane above shows upside down
            let m = min(w.z, w.w) / 2.0;
            let horizon = w.w / 2.0 - 0.12 * m;
            let ripple = sin(l.y * 1.3 + u.t * 3.0) * 0.5 + sin(l.y * 0.37 - u.t * 1.7) * 0.8;
            let gr = vec2<f32>(g.x + ripple, w.y - 4.0 - (l.y - horizon) * 1.6);
            col += env(gr) * sh.refl;
        }
        return col;
    }
    if (mode == 2u) { return scene_galaxy(uv); }
    let c = w.xy + w.zw / 2.0;
    let th = torus_basic(uv, l, w.zw, sun_dir_from(c));
    var col = th.col;
    if (th.hit) {
        // chrome: the reflected ray leaves the screen plane toward the neighbouring panes
        let r = reflect(th.rd, th.n);
        let reach = 0.75 * max(span.x, span.y);
        let gr = g + vec2<f32>(r.x, -r.y) * reach;
        let fres = 0.3 + 0.7 * pow(1.0 - max(dot(th.n, -th.rd), 0.0), 3.0);
        if (ascii_out()) {
            // glyphs need brightness, not mirror detail: lit body, light reflection
            col = col * 1.6 + vec3<f32>(0.12) + env(gr) * fres * 0.35;
        } else {
            col = col * (1.0 - fres * 0.6) + env(gr) * fres * 0.9;
        }
    }
    return col;
}

// Render tab point g, cross-fading into every neighbouring world near a border, weighted
// by distance, so both sides of each gap meet at the same mix and junctions have no edges.
fn render_blended(g: vec2<f32>) -> vec3<f32> {
    let k = world_at(g);
    var acc = vec3<f32>(0.0);
    var wsum = 0.0;
    var wts = array<f32, 3>(0.0, 0.0, 0.0);
    for (var q = 0u; q < u.nworlds; q++) {
        if (q == k) { continue; }
        let gap = rect_rect_dist(u.worlds[k], u.worlds[q]);
        wts[q] = 0.5 * (1.0 - smoothstep(gap, gap + SEAM, rect_dist(g, u.worlds[q])));
        wsum += wts[q];
    }
    let norm = select(1.0, 0.5 / wsum, wsum > 0.5);
    for (var q = 0u; q < u.nworlds; q++) {
        if (q == k || wts[q] < 0.002) { continue; }
        acc += render_full(q, g) * wts[q] * norm;
    }
    return render_full(k, g) * (1.0 - wsum * norm) + acc;
}

// One slow colour field over the whole tab; every world is tinted by it.
fn aurora(g: vec2<f32>) -> vec4<f32> {
    let t = u.t;
    let f = fbm(g * 0.012 + vec2<f32>(t * 0.035, -t * 0.02));
    let tint = hsv(f * 1.6 + t * 0.015, 0.45, 1.0);
    let curtain = pow(0.5 + 0.5 * sin(g.x * 0.045 + f * 7.0 + t * 0.4), 6.0)
        * smoothstep(0.0, 1.0, 1.0 - (g.y - u.region.y) / max(u.region.w - u.region.y, 1.0));
    return vec4<f32>(tint, curtain);
}

fn comets_at(g: vec2<f32>) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    for (var i = 0u; i < u.n_samples; i++) {
        let s = samples[i];
        let dd = length(g - s.pos.xy) / s.pos.z;
        if (dd < 1.0) { c += s.col.rgb * (1.0 - dd) * (1.0 - dd) * s.pos.w; }
    }
    return c;
}

fn rot2(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a); let s = sin(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.w || gid.y >= u.h) { return; }
    let t = u.t;
    let lc = vec2<f32>((f32(gid.x) + 0.5) / f32(u.w) * u.cols, (f32(gid.y) + 0.5) / f32(u.h) * u.rows * 2.0);

    var col: vec3<f32>;
    if (u.nworlds == 0u) {
        // geometry not known yet: just this pane's own scene, lit from the upper left
        let size = vec2<f32>(u.cols, u.rows * 2.0);
        let uv = world_uv(lc, size);
        if (u.mode == 1u) { col = synth_basic(uv).col; }
        else if (u.mode == 2u) { col = scene_galaxy(uv); }
        else { col = torus_basic(uv, lc, size, normalize(vec3<f32>(-0.5, 0.8, -0.6))).col; }
    } else {
        let g = u.me.xy + lc;
        let r0 = u.region;
        let span = r0.zw - r0.xy;
        let ctr = (r0.xy + r0.zw) / 2.0;
        let bh = ctr + vec2<f32>(0.36 * span.x * sin(t * 0.061), 0.30 * span.y * sin(t * 0.083 + 1.3));
        let rs = 0.065 * min(span.x, span.y);
        let d = g - bh;
        let r = max(length(d), 1e-3);
        let dir = d / r;

        let age = t % SHOCK_PERIOD;
        let front = age * 34.0;
        let fade = 1.0 - age / SHOCK_PERIOD;
        let wave = exp(-pow((r - front) / 5.0, 2.0)) * fade;

        let bend = rs * rs * 2.2 / r;
        let lp = g - dir * bend + dir * wave * 4.0;
        col = render_blended(lp);
        col += comets_at(lp);
        col += hsv(0.6 + r * 0.004 - t * 0.05, 0.6, 0.35) * wave;

        // shared aurora tint + glow curtains
        let au = aurora(lp);
        col = col * mix(vec3<f32>(1.0), au.rgb * 1.15, 0.17) + au.rgb * au.a * 0.06;
        // the sun's glow spills across pane borders
        let sd = length(lp - sun_tab()) / max(span.x, span.y);
        col += SUN * (exp(-sd * 9.0) * 0.35 + exp(-sd * 3.0) * 0.06);

        // black hole overlay (unchanged)
        let tilt = 0.12;
        let dq = vec2<f32>(d.x, d.y / tilt);
        let rho = length(dq) / rs;
        let swirl = rot2(dq, t * 2.6 / pow(max(rho, 1.0), 1.5));
        let sp = swirl * (0.2 / rs);
        let tex = 0.65 * vnoise(sp + vec2<f32>(rho * 0.5, 0.0)) + 0.35 * vnoise(sp * 2.3 + vec2<f32>(7.0, rho));
        let beam = 1.0 + 0.75 * (d.x / max(length(dq), 1e-3));
        let heat = clamp((rho - 2.0) / 3.0, 0.0, 1.0);
        let disk_col = mix(vec3<f32>(1.0, 0.95, 0.85), vec3<f32>(1.0, 0.35, 0.08), heat);
        let band = smoothstep(1.9, 2.3, rho) * (1.0 - smoothstep(4.0, 5.2, rho));
        let disk = disk_col * band * (0.35 + 0.9 * tex) * beam * 1.6;
        let rr = r / rs;
        let halo_band = smoothstep(1.02, 1.12, rr) * (1.0 - smoothstep(1.35, 1.9, rr));
        let halo_tex = vnoise(rot2(d, t * 1.4) * (0.8 / rs));
        let halo = vec3<f32>(1.0, 0.7, 0.35) * halo_band * (0.45 + 0.55 * max(-dir.y, 0.0)) * (0.4 + 0.8 * halo_tex) * (1.0 + 0.5 * dir.x);
        col += halo;
        if (d.y < 0.0) { col += disk; }
        if (rr < 1.0) { col = vec3<f32>(0.0); }
        col += vec3<f32>(1.0, 0.85, 0.6) * exp(-pow((rr - 1.04) / 0.035, 2.0)) * (1.2 + 0.8 * fade * fade);
        if (d.y >= 0.0) { col += disk; }
    }

    let c = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    let r8 = u32(c.r * 255.0); let g8 = u32(c.g * 255.0); let b8 = u32(c.b * 255.0);
    outp[gid.y * u.w + gid.x] = r8 | (g8 << 8u) | (b8 << 16u) | (255u << 24u);
}
