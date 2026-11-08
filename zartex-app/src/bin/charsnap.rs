//! Renders one character model: charsnap <game root> <mission stem> <skin hash> <out.png> [yaw]
//!
//! usage: the skin hash is a decimal number; [yaw] is the turn of the camera in radians (default 0.5).
//! What it does: loads the character with `character3d::load`, renders it with the software renderer of `city3d` into a
//! 480 by 640 PNG, and prints the triangle count and the box. Includes `city3d.rs`, `vehicle3d.rs` and `character3d.rs`
//! through `#[path]` (the same code as the app).
#[path = "../city3d.rs"]
mod city3d;
#[path = "../vehicle3d.rs"]
mod vehicle3d;
#[path = "../character3d.rs"]
mod character3d;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let skin: u32 = a[3].parse().unwrap();
    let m = match character3d::load(std::path::Path::new(&a[1]), &a[2], skin) { Ok(m) => m, Err(e) => { println!("ERR {e}"); return; } };
    println!("{} triangles, center {:?}, radius {:.2}", m.scene.verts.len() / 3, m.center, m.radius);
    let mut cam = city3d::Camera::orbit(m.center);
    cam.yaw = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.5);
    cam.pitch = 0.15;
    cam.dist = m.radius * 2.6;
    let img = city3d::render(&m.scene, &cam, 480, 640);
    let mut raw = Vec::new();
    for p in &img.pixels { raw.extend_from_slice(&[p.r(), p.g(), p.b(), 255]); }
    image::save_buffer(&a[4], &raw, 480, 640, image::ColorType::Rgba8).unwrap();
}
