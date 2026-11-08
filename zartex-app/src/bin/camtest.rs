//! Ground truth for the camera matrix: cameras with `pLookAt` set must face that actor. Which reading of the stored
//! right/up/forward vectors agrees?
//!
//! usage: camtest <folder that has a Missions subfolder>
//! What it does: for each mission (`.mpc`, except templates) and each camera with a `pLookAt` target, it compares the stored
//! camera vectors with the direction to the target and prints the mean cosine of each reading.
//! Includes `model.rs` and `camera.rs` through `#[path]` (the same code as the app).
#[path = "../model.rs"]
mod model;
#[path = "../camera.rs"]
mod camera;
use glam::Vec3;
use model::*;
use zartex_core::objects::MissionObject;

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut rows: Vec<(f32, f32, f32, f32)> = Vec::new();
    for e in std::fs::read_dir(format!("{dir}/Missions")).unwrap().flatten() {
        let p = e.path();
        if p.extension().map(|x| x != "mpc").unwrap_or(true) || p.file_stem().unwrap().to_string_lossy().to_lowercase().starts_with("template") {
            continue;
        }
        let Ok(proj) = Project::open_bytes(&p.to_string_lossy(), &std::fs::read(&p).unwrap()) else { continue };
        let v = proj.view();
        let objs = &v.mission.objects.objects;
        for a in v.mission.logic.actors.definitions.iter() {
            if !actor_type_name(a.type_id).contains("Camera") {
                continue;
            }
            let Some(look) = camera::prop_int(v, &a.properties, "pLookAt").filter(|x| *x >= 0) else { continue };
            let Some(MissionObject::Camera { right, up, forward, position, .. }) = objs.get(a.object_id.max(0) as usize) else { continue };
            let Some(t) = v.mission.logic.actors.definitions.get(look as usize) else { continue };
            let Some(tp) = objs.get(t.object_id.max(0) as usize).and_then(object_position) else { continue };
            let d = (Vec3::from(tp) - Vec3::new(position.x, position.y, position.z)).normalize_or_zero();
            let (r, u, f) = (Vec3::new(right.x, right.y, right.z), Vec3::new(up.x, up.y, up.z), Vec3::new(forward.x, forward.y, forward.z));
            let col_z = Vec3::new(r.z, u.z, f.z);
            let col_x = Vec3::new(r.x, u.x, f.x);
            rows.push((f.dot(d), (-f).dot(d), col_z.dot(d), col_x.dot(d)));
        }
    }
    let n = rows.len().max(1) as f32;
    println!("cameras with pLookAt and a positioned target: {}", rows.len());
    println!("mean cos with the direction to the target:");
    println!("  +forward row : {:.3}", rows.iter().map(|r| r.0).sum::<f32>() / n);
    println!("  -forward row : {:.3}", rows.iter().map(|r| r.1).sum::<f32>() / n);
    println!("  z column     : {:.3}", rows.iter().map(|r| r.2).sum::<f32>() / n);
    println!("  x column     : {:.3}", rows.iter().map(|r| r.3).sum::<f32>() / n);
    for r in rows.iter().take(8) {
        println!("   +f {:.2}  -f {:.2}  colz {:.2}  colx {:.2}", r.0, r.1, r.2, r.3);
    }
}
