//! Vehicle models for the preview box: the city's `.vvs` (one package and one part hierarchy per vehicle id), with the
//! shared car materials of `Vehicles\<City>\CarGlobals<City>.vgt`.
//!
//! What it does: `load` finds a vehicle id in the city's `.vvs` (or `.vvv`) file, places each body part and wheel with the
//! vehicle's part hierarchy, and returns the triangles as a `VehicleModel` for the model box.
//! Uses: `city3d` (`Scene`, `texture_for`), `formats` (`chnk`, `mdpc` and `vehicle` readers). Used by: `modelbox` (`load`),
//! `assets` (`city_name` and the city's vehicle ids), `plassets` and `character3d` (`VehicleModel`), and the `vehsnap` and
//! `charsnap` tools, which include this file.

use crate::city3d::{texture_for, Scene};
use formats::chnk::Chunk;
use formats::{mdpc, vehicle};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// A vehicle or character model ready to draw: its triangles, the centre and radius of its box, and the number of
/// parts that were drawn (for a character, the models of the chosen group).
pub struct VehicleModel {
    /// The triangles, in model space.
    pub scene: Scene,
    /// Centre of the box around the triangles.
    pub center: Vec3,
    /// Half of the box diagonal, at least 1.0; used to set the camera distance.
    pub radius: f32,
    /// Number of drawn parts.
    pub parts: usize,
}

/// Name of a city as used in the vehicle file names (`miami.vvs`, `CarGlobalsMiami.vgt`) for city type 1 to 6; `None` for other types.
pub fn city_name(city_type: i32) -> Option<&'static str> {
    Some(match city_type {
        1 | 2 => "Miami",
        3 | 4 => "Nice",
        5 | 6 => "Istanbul",
        _ => return None,
    })
}

/// Builds the model of vehicle `id` of the city `city_type`. The vehicle is looked up in the city's `.vvs` file, then in
/// its `.vvv` file (extra vehicles such as the police car). Errors name the file or the vehicle that is missing.
pub fn load(game: &Path, city_type: i32, id: u32) -> Result<VehicleModel, String> {
    let city = city_name(city_type).ok_or("unknown city")?;
    let lower = city.to_lowercase();
    let read = |p: std::path::PathBuf| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()));
    // ---- the vehicle file: one package and one part hierarchy per vehicle
    let vvs = read(game.join("Vehicles").join(format!("{lower}.vvs")))?;
    let root = Chunk::parse(&vvs).map_err(|e| e.to_string())?;
    let mut hs = vehicle::load_hierarchies(&root);
    let mut pk = mdpc::load_packages(&root).map_err(|e| e.to_string())?;
    if pk.len() != hs.len() {
        return Err(format!("{lower}.vvs: {} packages vs {} hierarchies", pk.len(), hs.len()));
    }
    let mut pkh: Vec<usize> = (0..hs.len()).collect();
    // vehicles that are not in the traffic pack live in the city's `.vvv` (the police car, ...)
    let vvv = std::fs::read(game.join("Vehicles").join(format!("{lower}.vvv"))).ok();
    let vvv_root = vvv.as_deref().and_then(|b| Chunk::parse(b).ok());
    if !hs.iter().any(|h| h.uid == id) {
        if let Some(r) = vvv_root.as_ref() {
            let (h2, p2) = (vehicle::load_hierarchies(r), mdpc::load_packages(r));
            if let Ok(p2) = p2 {
                if p2.len() == 1 || p2.len() == h2.len() {
                    let base = pk.len();
                    let shared = p2.len() == 1;
                    pkh.extend((0..h2.len()).map(|i| base + if shared { 0 } else { i }));
                    hs.extend(h2);
                    pk.extend(p2);
                }
            }
        }
    }
    let hi = hs.iter().position(|h| h.uid == id).ok_or_else(|| format!("vehicle {id} is not in {lower}.vvs or {lower}.vvv"))?;
    let pi = pkh[hi];
    // ---- shared car materials (package uid 0x1D, the CarGlobals file); appended as the last package
    let vgt = read(game.join("Vehicles").join(city).join(format!("CarGlobals{city}.vgt")))?;
    let vgt_root = Chunk::parse(&vgt).map_err(|e| e.to_string())?;
    let vgt_pk = mdpc::load_packages(&vgt_root).map_err(|e| e.to_string())?;
    let vgt_uid = vgt_pk.first().map(|p| p.uid).ok_or("empty CarGlobals")?;
    pk.extend(vgt_pk);
    let uid_map: HashMap<u32, usize> = [(vgt_uid, pk.len() - 1)].into();

    // ---- part transforms: each part's matrix is its parent's matrix times its own axis matrix
    let h = &hs[hi];
    let parents = h.parents();
    let mut world: Vec<Mat4> = Vec::with_capacity(h.parts.len());
    for (i, p) in h.parts.iter().enumerate() {
        let local = usize::try_from(p.axis_id).ok().and_then(|a| h.instances.get(a)).map(Mat4::from_cols_array).unwrap_or(Mat4::IDENTITY);
        let parent = parents[i].map(|j| world[j]).unwrap_or(Mat4::IDENTITY);
        world.push(parent * local);
    }
    // ---- triangles: every drawn part, with its world matrix
    let mut scene = Scene::default();
    let mut cache: HashMap<(usize, usize), u32> = HashMap::new();
    let mut parts = 0;
    for (i, p) in h.parts.iter().enumerate() {
        // ordinary body parts and wheels; markers, damage variants and lamps are not drawn
        if !(p.ptype == 1 || p.ptype == 2) || p.model_id == 0xFF {
            continue;
        }
        let pkg = &pk[pi];
        let Some(model) = pkg.models.get(p.model_id as usize) else { continue };
        if model.vb_index < 0 {
            continue;
        }
        let Some(lod) = model.lods[..5].iter().find(|l| l.count > 0) else { continue };
        parts += 1;
        for li in &pkg.lod_instances[lod.inst_first..lod.inst_first + lod.count] {
            // the per-LOD matrices are not applied to vehicles (applied, the parts of a car fly apart)
            let local = Mat4::IDENTITY;
            let _ = li.use_transform;
            let m = world[i] * local;
            for sub in &pkg.submodels[li.sub_first..li.sub_first + li.sub_count] {
                let mut idx = Vec::new();
                if pkg.submodel_triangles(sub, &mut idx).is_err() {
                    continue;
                }
                let tex = texture_for(&pk, &uid_map, pi, sub.material, &mut cache, &mut scene.textures);
                for tri in idx.chunks_exact(3) {
                    for &k in tri {
                        match pkg.vertex(model.vb_index as usize, k as usize) {
                            Some((pos, _, uv)) => {
                                scene.verts.push(m.transform_point3(Vec3::from(pos)));
                                scene.uvs.push(uv);
                            }
                            None => {
                                scene.verts.push(Vec3::ZERO);
                                scene.uvs.push([0.0, 0.0]);
                            }
                        }
                    }
                    scene.tri_tex.push(tex);
                }
            }
        }
    }
    if scene.verts.is_empty() {
        return Err(format!("vehicle {id}: no drawable parts"));
    }
    // ---- box of the model; vertices that are not finite or are far away (over 1000 units) are ignored
    let (mut lo, mut hi_) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &scene.verts {
        if v.is_finite() && v.abs().max_element() < 1000.0 {
            lo = lo.min(*v);
            hi_ = hi_.max(*v);
        }
    }
    let center = (lo + hi_) * 0.5;
    let radius = ((hi_ - lo).length() * 0.5).max(1.0);
    scene.instances = parts;
    Ok(VehicleModel { scene, center, radius, parts })
}
