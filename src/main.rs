//! GPU torus pane: a wgpu compute shader renders each frame, which is pushed into
//! the pane through herdr's `pane.graphics.set` layer. Comets use the same f64 math
//! as scene.py so they still cross into the neighbouring panes.

mod ascii;

use base64::Engine;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wgpu::util::DeviceExt;

// herdr panes showing the torus, synth and galaxy worlds, in that order
// (WORLD_PANES="w1:p2,w1:p3,w1:p4" overrides).
fn group() -> [String; 3] {
    let v: Vec<String> = std::env::var("WORLD_PANES")
        .unwrap_or_else(|_| "w1:p2,w1:p3,w1:p4".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let get = |i: usize| v.get(i).cloned().unwrap_or_default();
    [get(0), get(1), get(2)]
}
const COMETS: [[f64; 5]; 4] = [
    [23.0, 17.0, 0.0, 0.3, 0.02],
    [31.0, 13.0, 0.5, 0.7, 0.55],
    [17.0, 29.0, 0.2, 0.1, 0.13],
    [41.0, 11.0, 0.8, 0.5, 0.80],
];
const TRAIL: usize = 14;
const MAX_IMAGE_BYTES: f64 = 500_000.0;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    w: u32,
    h: u32,
    cols: f32,
    rows: f32,
    t: f32,
    n_samples: u32,
    mode: u32,
    nworlds: u32,
    region: [f32; 4],
    me: [f32; 4],
    worlds: [[f32; 4]; 3],
    wmode: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Sample {
    pos: [f32; 4],
    col: [f32; 4],
}

#[derive(Clone, Copy, Default)]
struct Geo {
    me: (f64, f64, f64, f64),
    region: (f64, f64, f64, f64),
    group: [Option<(f64, f64, f64, f64)>; 3],
    ok: bool,
}

fn rpc(sock: &str, body: &serde_json::Value) -> Option<serde_json::Value> {
    let mut s = UnixStream::connect(sock).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    s.write_all(format!("{body}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

fn poll_layout(sock: String, pane: String, geo: Arc<Mutex<Geo>>) {
    loop {
        let req = serde_json::json!({"id": "l", "method": "pane.layout", "params": {"pane_id": pane}});
        let out = rpc(&sock, &req).or_else(|| {
            let o = std::process::Command::new("herdr").args(["pane", "layout"]).output().ok()?;
            serde_json::from_slice(&o.stdout).ok()
        });
        if let Some(v) = out {
            if let Some(panes) = v.pointer("/result/layout/panes").and_then(|p| p.as_array()) {
                let rect = |id: &str| {
                    panes.iter().find(|p| p["pane_id"] == id).map(|p| {
                        let r = &p["rect"];
                        let f = |k: &str| r[k].as_f64().unwrap_or(0.0);
                        (f("x"), f("y"), f("width"), f("height"))
                    })
                };
                if let Some(me) = rect(&pane) {
                    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for g in &group() {
                        if let Some((x, y, w, h)) = rect(g.as_str()) {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x + w);
                            y1 = y1.max(y + h);
                        }
                    }
                    let g3 = group();
                    let group = [rect(&g3[0]), rect(&g3[1]), rect(&g3[2])];
                    *geo.lock().unwrap() = Geo { me, region: (x0, y0, x1, y1), group, ok: true };
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn term_size() -> (u32, u32) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) };
    (ws.ws_col as u32, ws.ws_row as u32)
}

fn hsv(h: f64, s: f64, v: f64) -> [f32; 4] {
    let h = h.rem_euclid(1.0);
    let i = (h * 6.0) as i32;
    let f = h * 6.0 - i as f64;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r as f32, g as f32, b as f32, 1.0]
}

fn bounce(v: f64, t: f64, ph: f64, lo: f64, hi: f64) -> f64 {
    let l = hi - lo;
    if l <= 0.0 {
        return lo;
    }
    let u = (v * t + ph * 2.0 * l).rem_euclid(2.0 * l);
    lo + if u < l { u } else { 2.0 * l - u }
}

fn comet_samples(t: f64, g: &Geo, cols: u32, rows: u32) -> Vec<Sample> {
    let mut out = Vec::with_capacity(COMETS.len() * TRAIL);
    if !g.ok {
        return out;
    }
    let _ = (cols, rows);
    let (x0, y0, x1, y1) = g.region;
    for c in COMETS {
        for k in 0..TRAIL {
            let tk = t - k as f64 * 0.035;
            let gx = bounce(c[0], tk, c[2], x0 + 1.0, x1 - 2.0);
            let gy = bounce(c[1], tk, c[3], (y0 + 1.0) * 2.0, (y1 - 1.0) * 2.0 - 1.0);
            let kf = k as f64 / TRAIL as f64;
            let (rad, inten, sat) = if k == 0 { (3.2, 1.0, 0.2) } else { (2.2 * (1.0 - kf) + 0.4, 0.55 * (1.0 - kf), 0.6) };
            out.push(Sample {
                pos: [gx as f32, gy as f32, rad as f32, inten as f32],
                col: hsv(c[4] + k as f64 * 0.01, sat, 1.0),
            });
        }
    }
    out
}

fn main() {
    let sock = std::env::var("HERDR_SOCKET_PATH").expect("run inside herdr");
    let pane = std::env::var("HERDR_PANE_ID").expect("run inside herdr");
    let scale: f64 = std::env::var("TORUS_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let log_path = std::env::var("TORUS_LOG").ok();
    let dump_path = std::env::var("TORUS_DUMP").ok();
    let scene = std::env::args().nth(1).unwrap_or_else(|| "torus".into());
    let mode: u32 = match scene.as_str() {
        "synth" => 1,
        "galaxy" => 2,
        _ => 0,
    };

    let geo = Arc::new(Mutex::new(Geo::default()));
    {
        let (s, p, g) = (sock.clone(), pane.clone(), geo.clone());
        std::thread::spawn(move || poll_layout(s, p, g));
    }

    // ASCII when the attached terminal has no image transport (ASCII=1/0 forces it)
    let force_ascii = std::env::var("ASCII").ok().map(|v| v == "1");
    let want_ascii = Arc::new(AtomicBool::new(force_ascii.unwrap_or(false)));
    if force_ascii.is_none() {
        let (s, p, a) = (sock.clone(), pane.clone(), want_ascii.clone());
        std::thread::spawn(move || loop {
            if let Some(v) = rpc(&s, &serde_json::json!({"id": "g", "method": "pane.graphics.info", "params": {"pane_id": p}})) {
                let images = v.pointer("/result/file_frame_transport").map_or(false, |t| !t.is_null());
                a.store(!images, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_secs(2));
        });
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut asc = ascii::Ascii::new(std::env::var("ASCII_COLOR").map_or(true, |v| v != "0"));
    let mut cur_ascii = false;
    let mut ascii_bytes = 0usize;

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || r.store(false, Ordering::SeqCst)).unwrap();
    }

    // GPU setup
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("no GPU adapter");
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None)).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[Sample { pos: [0.0; 4], col: [0.0; 4] }; COMETS.len() * TRAIL]),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    // terminal: blank text layer, the image does the drawing
    print!("\x1b[?25l\x1b[2J\x1b[H");
    std::io::stdout().flush().ok();
    eprintln!("gpu: {} ({:?})", info.name, info.backend);

    let b64 = base64::engine::general_purpose::STANDARD;
    // keep a few frames in flight so herdr's per-frame round trip doesn't cap the rate
    let inflight: usize = std::env::var("TORUS_INFLIGHT").ok().and_then(|s| s.parse().ok()).unwrap_or(8);
    let pending = Arc::new((Mutex::new(0usize), std::sync::Condvar::new()));
    let last_reply = Arc::new(Mutex::new(String::new()));
    let accepted = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let mut rgb: Vec<u8> = Vec::new();
    let mut size = (0u32, 0u32, 0u32, 0u32); // cols, rows, w, h
    let mut out_buf: Option<(wgpu::Buffer, wgpu::Buffer, wgpu::BindGroup)> = None;
    let (mut frames, mut log_t) = (0u32, Instant::now());
    let mut msg = String::new();
    let mut seq: u64 = 0;

    while running.load(Ordering::SeqCst) {
        let start = Instant::now();
        let (cols, rows) = term_size();
        if cols == 0 || rows == 0 {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let ascii_mode = want_ascii.load(Ordering::Relaxed);
        if ascii_mode != cur_ascii {
            if ascii_mode {
                rpc(&sock, &serde_json::json!({"id": "c", "method": "pane.graphics.clear", "params": {"pane_id": pane, "layer_id": scene}}));
            } else {
                print!("\x1b[0m\x1b[2J");
                std::io::stdout().flush().ok();
            }
            asc.reset();
            cur_ascii = ascii_mode;
            out_buf = None;
        }
        if (cols, rows) != (size.0, size.1) || out_buf.is_none() {
            let cell = rpc(&sock, &serde_json::json!({"id": "i", "method": "pane.graphics.info", "params": {"pane_id": pane}}));
            let cw = cell.as_ref().and_then(|v| v.pointer("/result/cell_width_px")?.as_f64()).unwrap_or(10.0);
            let ch = cell.as_ref().and_then(|v| v.pointer("/result/cell_height_px")?.as_f64()).unwrap_or(20.0);
            // herdr rejects images over ~512 KB (image_too_large); fit rgb frames under it
            let (pw, ph) = (cols as f64 * cw, rows as f64 * ch);
            let fit = (pw * ph * 3.0 / MAX_IMAGE_BYTES).sqrt();
            let s = scale.max(fit);
            let (w, h) = if ascii_mode {
                (cols * ascii::SUB_X, rows * ascii::SUB_Y)
            } else {
                (((pw / s) as u32).max(16), ((ph / s) as u32).max(16))
            };
            let bytes = (w * h * 4) as u64;
            let storage = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: storage.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: sbuf.as_entire_binding() },
                ],
            });
            out_buf = Some((storage, readback, bg));
            size = (cols, rows, w, h);
            print!("\x1b[2J");
            std::io::stdout().flush().ok();
        }
        let (_, _, w, h) = size;
        let (storage, readback, bind) = out_buf.as_ref().unwrap();

        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() % 10000.0;
        let g = *geo.lock().unwrap();
        let samples = comet_samples(t, &g, cols, rows);
        // pane content rects in tab coords; other panes share this pane's border inset
        let (nworlds, region, me, worlds) = if g.ok {
            let (_, _, mw, mh) = g.me;
            let ix = ((mw - cols as f64) / 2.0).floor();
            let iy = ((mh - rows as f64) / 2.0).floor();
            let content = |(x, y, w, h): (f64, f64, f64, f64)| {
                [(x + ix) as f32, ((y + iy) * 2.0) as f32, (w - 2.0 * ix) as f32, ((h - 2.0 * iy) * 2.0) as f32]
            };
            let mut worlds = [[0f32; 4]; 3];
            let mut n = 0;
            for (k, r) in g.group.iter().enumerate() {
                match r {
                    Some(r) => worlds[k] = content(*r),
                    None => break,
                }
                n = k as u32 + 1;
            }
            let (x0, y0, x1, y1) = g.region;
            (n, [x0 as f32, (y0 * 2.0) as f32, x1 as f32, (y1 * 2.0) as f32], content(g.me), worlds)
        } else {
            (0, [0.0; 4], [0.0; 4], [[0.0; 4]; 3])
        };
        if !samples.is_empty() {
            queue.write_buffer(&sbuf, 0, bytemuck::cast_slice(&samples));
        }
        let uni = Uniforms {
            w,
            h,
            cols: cols as f32,
            rows: rows as f32,
            t: t as f32,
            n_samples: samples.len() as u32,
            mode,
            nworlds,
            region,
            me,
            worlds,
            wmode: [0, 1, 2, ascii_mode as u32],
        };
        queue.write_buffer(&ubuf, 0, bytemuck::bytes_of(&uni));

        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        enc.copy_buffer_to_buffer(storage, 0, readback, 0, (w * h * 4) as u64);
        queue.submit(Some(enc.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);

        if ascii_mode {
            if let Some(p) = &dump_path {
                let req = format!("{p}.req");
                if std::path::Path::new(&req).exists() {
                    let mut f = format!("{w} {h} {t}\n").into_bytes();
                    f.extend(slice.get_mapped_range().chunks_exact(4).flat_map(|p| p[..3].to_vec()));
                    let _ = std::fs::write(p, f);
                    let _ = std::fs::remove_file(req);
                }
            }
            ascii_bytes += asc.draw(&slice.get_mapped_range(), cols, rows);
            readback.unmap();
            if let Some(p) = &log_path {
                frames += 1;
                let el = log_t.elapsed();
                if el > Duration::from_secs(2) {
                    let _ = std::fs::write(
                        p,
                        format!(
                            "ascii {:.1} fps  {}x{} cells  {:.1} KB/s  gpu={}\n",
                            frames as f64 / el.as_secs_f64(),
                            cols,
                            rows,
                            ascii_bytes as f64 / el.as_secs_f64() / 1024.0,
                            info.name
                        ),
                    );
                    frames = 0;
                    ascii_bytes = 0;
                    log_t = Instant::now();
                }
            }
            // phones are usually on a network link: 30 fps is plenty for text
            if let Some(d) = Duration::from_millis(33).checked_sub(start.elapsed()) {
                std::thread::sleep(d);
            }
            continue;
        }

        msg.clear();
        seq += 1;
        msg.push_str(&format!(
            "{{\"id\":\"f{seq}\",\"method\":\"pane.graphics.set\",\"params\":{{\"pane_id\":\"{pane}\",\"layer_id\":\"{scene}\",\
             \"format\":\"rgb\",\"image_width\":{w},\"image_height\":{h},\
             \"placement\":{{\"grid_cols\":{cols},\"grid_rows\":{rows},\"viewport_col\":0,\"viewport_row\":0}},\"data_base64\":\""
        ));
        {
            let px = slice.get_mapped_range();
            rgb.clear();
            for p in px.chunks_exact(4) {
                rgb.extend_from_slice(&p[..3]);
            }
        }
        // TORUS_DUMP=<path>: write one raw frame ("w h t\n" + rgb) when <path>.req exists
        if let Some(p) = &dump_path {
            let req = format!("{p}.req");
            if std::path::Path::new(&req).exists() {
                let mut f = format!("{w} {h} {t}\n").into_bytes();
                f.extend_from_slice(&rgb);
                let _ = std::fs::write(p, f);
                let _ = std::fs::remove_file(req);
            }
        }
        b64.encode_string(&rgb, &mut msg);
        msg.push_str("\"}}\n");
        readback.unmap();

        // herdr answers one request per connection; bound in-flight frames so we pace
        // to what it can composite without waiting a full round trip per frame
        {
            let (lock, cv) = &*pending;
            let mut n = lock.lock().unwrap();
            while *n >= inflight {
                n = cv.wait(n).unwrap();
            }
            *n += 1;
        }
        {
            let (body, sock, pending, last, acc) =
                (std::mem::take(&mut msg), sock.clone(), pending.clone(), last_reply.clone(), accepted.clone());
            std::thread::spawn(move || {
                let mut reply = String::new();
                if let Ok(mut c) = UnixStream::connect(&sock) {
                    if c.write_all(body.as_bytes()).is_ok() {
                        let _ = BufReader::new(c).read_line(&mut reply);
                    }
                }
                if reply.contains("\"ok\"") {
                    acc.fetch_add(1, Ordering::Relaxed);
                } else {
                    *last.lock().unwrap() = if reply.is_empty() { "no reply".into() } else { reply };
                }
                let (lock, cv) = &*pending;
                *lock.lock().unwrap() -= 1;
                cv.notify_one();
            });
        }
        let reply = last_reply.lock().unwrap().clone();

        if let Some(p) = &log_path {
            frames += 1;
            let el = log_t.elapsed();
            if el > Duration::from_secs(2) {
                let _ = std::fs::write(
                    p,
                    format!(
                        "{:.1} fps shown ({:.1} rendered)  {}x{} px  gpu={} ({:?})  last_error={}\n",
                        accepted.swap(0, Ordering::Relaxed) as f64 / el.as_secs_f64(),
                        frames as f64 / el.as_secs_f64(),
                        w,
                        h,
                        info.name,
                        info.backend,
                        reply.trim().chars().take(80).collect::<String>()
                    ),
                );
                frames = 0;
                log_t = Instant::now();
            }
        }
        if let Some(d) = Duration::from_millis(16).checked_sub(start.elapsed()) {
            std::thread::sleep(d);
        }
    }

    rpc(&sock, &serde_json::json!({"id": "c", "method": "pane.graphics.clear", "params": {"pane_id": pane, "layer_id": scene}}));
    print!("\x1b[0m\x1b[?25h\x1b[2J\x1b[H");
    std::io::stdout().flush().ok();
}
