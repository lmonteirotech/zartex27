//! Headless check: render the city around a mission's player start to a PNG.
//! usage: citysnap <game root> <mission.mpc> <out.png> [radius] [yaw] [pitch] [dist]
//!
//! What it does: reads the mission's city type and first character position (or the summary start), loads the city
//! geometry around it with `CityStore::region`, and renders a 1280 by 720 PNG. Environment variables: `CENTER` (x,y,z)
//! moves the centre, `CAMPOS` and `CAMFWD` (x,y,z each) render from a given eye and direction instead, and `ZM_CPU` set
//! means the CPU renderer is used. The renderer used is printed. Includes `city3d.rs` and `gpu.rs` through `#[path]`.
#[path = "../city3d.rs"]
mod city3d;
#[path = "../gpu.rs"]
mod gpu;

use glam::Vec3;
use zartex_core::mission::MissionFile;
use zartex_core::objects::MissionObject;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: citysnap <game root> <mission.mpc> <out.png> [radius] [yaw] [pitch] [dist]");
        return;
    }
    let num = |i: usize, d: f32| a.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(d);
    let bytes = std::fs::read(&a[2]).unwrap();
    let mf = MissionFile::from_bytes(&bytes).unwrap();
    let city = mf.summary.as_ref().map(|s| s.city_type).unwrap_or(1);
    let mut start = None;
    for o in &mf.mission.objects.objects {
        if let MissionObject::Character { position, .. } = o {
            start = Some([position.x, position.y, position.z]);
            break;
        }
    }
    let sp = mf.summary.as_ref().map(|s| s.start_position);
    println!("city_type {city} summary start {:?} first character {:?}", sp.map(|v| (v.x, v.y)), start);
    let c = start.unwrap_or([sp.map(|v| v.x).unwrap_or(0.0), 0.0, sp.map(|v| v.y).unwrap_or(0.0)]);
    let c = std::env::var("CENTER").ok().and_then(|s| { let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect(); (v.len() == 3).then(|| [v[0], v[1], v[2]]) }).unwrap_or(c);
    gpu::install_hook();
    gpu::apply_preference(std::env::var("ZM_CPU").is_ok());
    println!("renderer: {}", if gpu::enabled() { "GPU (Vulkan)" } else { "CPU" });
    if let gpu::Support::Unavailable(w) = gpu::support() {
        println!("Vulkan unavailable: {w}");
    }
    let store = city3d::CityStore::open(std::path::Path::new(&a[1]), city).unwrap();
    let scene = store.region(c, num(4, 250.0)).unwrap();
    println!("{} instances, {} triangles, {} ms", scene.instances, scene.verts.len() / 3, scene.load_ms);
    let mut cam = city3d::Camera::orbit(Vec3::from(c));
    cam.yaw = num(5, 0.8);
    cam.pitch = num(6, 0.55);
    cam.dist = num(7, 90.0);
    let t = std::time::Instant::now();
    let vec = |k: &str| std::env::var(k).ok().and_then(|s| { let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect(); (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2])) });
    let img = if let (Some(eye), Some(f)) = (vec("CAMPOS"), vec("CAMFWD")) {
        let view = glam::Mat4::look_to_rh(eye, f, Vec3::Y);
        let proj = glam::Mat4::perspective_rh(55f32.to_radians(), 16.0 / 9.0, 0.3, 6000.0);
        city3d::render_vp(&scene, &(proj * view), eye, 1280, 720, None, None)
    } else {
        city3d::render(&scene, &cam, 1280, 720)
    };
    println!("render {} ms", t.elapsed().as_millis());
    let mut raw = Vec::new();
    for p in &img.pixels {
        raw.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
    }
    image::save_buffer(&a[3], &raw, 1280, 720, image::ColorType::Rgba8).unwrap();
}
