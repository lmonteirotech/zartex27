//! Renders one vehicle model: vehsnap <game root> <city_type> <vehicle id> <out.png> [yaw] [pitch]
//!
//! What it does: loads the vehicle with `vehicle3d::load`, renders it with the software renderer of `city3d` into a 640 by
//! 400 PNG (default yaw 0.7, pitch 0.35), and prints the part and triangle counts. Includes `city3d.rs` and `vehicle3d.rs`
//! through `#[path]` (the same code as the app).
#[path = "../city3d.rs"]
mod city3d;
#[path = "../vehicle3d.rs"]
mod vehicle3d;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (ct, id): (i32, u32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let t = std::time::Instant::now();
    let m = vehicle3d::load(std::path::Path::new(&a[1]), ct, id).unwrap();
    println!("{} parts, {} triangles, center {:?} radius {:.2}, {} ms", m.parts, m.scene.verts.len() / 3, m.center, m.radius, t.elapsed().as_millis());
    let mut cam = city3d::Camera::orbit(m.center);
    cam.yaw = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.7);
    cam.pitch = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(0.35);
    cam.dist = m.radius * 2.6;
    let img = city3d::render(&m.scene, &cam, 640, 400);
    let mut raw = Vec::new();
    for p in &img.pixels { raw.extend_from_slice(&[p.r(), p.g(), p.b(), 255]); }
    image::save_buffer(&a[4], &raw, 640, 400, image::ColorType::Rgba8).unwrap();
}
