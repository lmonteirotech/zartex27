//! Character models for the preview box: `Missions\missionNN.dam` (one MDPC package of skinned models plus `UPXD`, the
//! table of the skins that the mission can use). Models are shown in their bind pose, without the skeleton.
//!
//! What it does: `skins` reads the list of skin hashes from a `.dam` file's `UPXD` table; `load` builds the model of one
//! skin: the model group at the same position as the skin in that table, and in the group the model with the most
//! triangles.
//! Uses: `city3d` (`Scene`, `texture_for`), `vehicle3d` (`VehicleModel`), `formats` (`chnk`, `mdpc`).
//! Used by: `modelbox` (`load`), `assets` (`skins` of a mission), and the `charsnap` tool, which includes this file.

use crate::city3d::{texture_for, Scene};
use crate::vehicle3d::VehicleModel;
use formats::chnk::Chunk;
use formats::mdpc;
use glam::Vec3;
use std::collections::HashMap;
use std::path::Path;

/// Reads the little-endian u32 at byte offset `o`; `None` when the data is too short.
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Skin hashes listed by the `UPXD` table, in table order. Entry `k` is drawn by the `k`-th distinct model.
pub fn skins(upxd: &[u8]) -> Vec<u32> {
    let n = u32_at(upxd, 0).unwrap_or(0) as usize;
    (0..n.min(64)).filter_map(|k| u32_at(upxd, 0x10 + 0xC0 * k)).collect()
}

/// Groups of model indices that share one model uid (a model and its twin), in package order. The model records are
/// `0x188` bytes long, starting at `off`; the uid is the 8 bytes at the start of a record (first word is the high half).
fn model_groups(pkg_data: &[u8], n: usize, off: usize) -> Vec<Vec<usize>> {
    let mut groups: Vec<(u64, Vec<usize>)> = Vec::new();
    for i in 0..n {
        // byte offset of model record `i`
        let o = off + 0x188 * i;
        let uid = (u32_at(pkg_data, o).unwrap_or(0) as u64) << 32 | u32_at(pkg_data, o + 4).unwrap_or(0) as u64;
        match groups.last_mut() {
            Some((u, v)) if *u == uid => v.push(i),
            _ => groups.push((uid, vec![i])),
        }
    }
    groups.into_iter().map(|g| g.1).collect()
}

/// Builds the bind-pose model of character skin `skin` from `Missions\<mission_stem>.dam`. The skin's position in the
/// `UPXD` table selects a model group. Errors when the file, the table or the skin is missing, or when the model has no
/// geometry.
pub fn load(game: &Path, mission_stem: &str, skin: u32) -> Result<VehicleModel, String> {
    // ---- the file: its UPXD table (skins) and MDPC chunk (models)
    let path = game.join("Missions").join(format!("{mission_stem}.dam"));
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root = Chunk::parse(&bytes).map_err(|e| e.to_string())?;
    let kids = root.entries.first().ok_or("empty .dam")?.children().map_err(|e| e.to_string())?;
    let upxd = kids.find("UPXD").next().ok_or("no UPXD table in the .dam")?;
    let mdpc_e = kids.find("MDPC").next().ok_or("no models in the .dam")?;
    let table = skins(upxd.data);
    let k = table.iter().position(|&h| h == skin).ok_or_else(|| format!("skin {skin:#x} is not in {mission_stem}.dam ({} skins)", table.len()))?;
    let n = u32_at(mdpc_e.data, 8).unwrap_or(0) as usize;
    let off = u32_at(mdpc_e.data, 12).unwrap_or(0) as usize;
    let groups = model_groups(mdpc_e.data, n, off);
    let group = groups.get(k).ok_or_else(|| format!("skin entry {k} has no model (the file has {} distinct models)", groups.len()))?;
    let pk = mdpc::load_packages(&kids).map_err(|e| e.to_string())?;
    let pkg = pk.first().ok_or("no package")?;
    // the file has one package, so there is no other package to refer to
    let uid_map: HashMap<u32, usize> = HashMap::new();
    // ---- triangles: each model of the group is built, and the one with the most triangles is kept
    // twins are variants of one model: show the one with the most triangles
    let mut best: Option<Scene> = None;
    for &mi in group {
        let Some(model) = pkg.models.get(mi) else { continue };
        if model.vb_index < 0 {
            continue;
        }
        let Some(lod) = model.lods[..5].iter().find(|l| l.count > 0) else { continue };
        let mut scene = Scene::default();
        let mut cache: HashMap<(usize, usize), u32> = HashMap::new();
        for li in &pkg.lod_instances[lod.inst_first..lod.inst_first + lod.count] {
            for sub in &pkg.submodels[li.sub_first..li.sub_first + li.sub_count] {
                let mut idx = Vec::new();
                if pkg.submodel_triangles(sub, &mut idx).is_err() {
                    continue;
                }
                let tex = texture_for(&pk, &uid_map, 0, sub.material, &mut cache, &mut scene.textures);
                for tri in idx.chunks_exact(3) {
                    for &v in tri {
                        match pkg.vertex(model.vb_index as usize, v as usize) {
                            Some((p, _, uv)) => {
                                scene.verts.push(Vec3::from(p));
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
        if best.as_ref().map(|b| b.verts.len() < scene.verts.len()).unwrap_or(true) {
            best = Some(scene);
        }
    }
    // ---- box of the model (vertices that are not finite or are far away are ignored)
    let scene = best.filter(|s| !s.verts.is_empty()).ok_or("the model has no geometry")?;
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &scene.verts {
        if v.is_finite() && v.abs().max_element() < 1000.0 {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    let center = (lo + hi) * 0.5;
    let radius = ((hi - lo).length() * 0.5).max(0.5);
    Ok(VehicleModel { scene, center, radius, parts: group.len() })
}
