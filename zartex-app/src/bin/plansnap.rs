//! Builds the city plan of a city and writes it as a PNG: plansnap <game root> <city type> <out.png>
//!
//! What it does: opens the city with `CityStore::open`, builds its minimap picture with `CityStore::plan`, prints its size
//! and bounds, and saves it. Includes `city3d.rs` through `#[path]` (the same code as the app).
#[path = "../city3d.rs"]
mod city3d;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let store = city3d::CityStore::open(std::path::Path::new(&a[1]), a[2].parse().unwrap()).unwrap();
    let t = std::time::Instant::now();
    let p = store.plan().unwrap();
    println!("plan {}x{} px, bounds {:?} .. {:?}, {} ms", p.img.width(), p.img.height(), p.min, p.max, t.elapsed().as_millis());
    let mut raw = Vec::new();
    for c in &p.img.pixels { raw.extend_from_slice(&[c.r(), c.g(), c.b(), 255]); }
    image::save_buffer(&a[3], &raw, p.img.width() as u32, p.img.height() as u32, image::ColorType::Rgba8).unwrap();
}
