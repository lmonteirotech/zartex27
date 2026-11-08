//! What the game offers to build missions with: skies, vehicles, character skins (data for the Assets tab).
//!
//! What it does: lists the sky files of the game folder, the vehicle ids of a city, the skins of a mission, and turns a
//! skin hash into its name from the generated table `skin_names`.
//! Uses: `skin_names` (`SKINS`), `vehicle3d` (`city_name`), `character3d` (`skins`), `formats` (`chnk`, `mdpc`, `vehicle`).
//! Used by: `main.rs` (the Assets tab).

use crate::skin_names::SKINS;
use formats::chnk::Chunk;
use formats::{mdpc, vehicle};
use std::path::Path;

/// `Skies\*.d3s` file names of the game folder, sorted. Empty when the folder cannot be read.
pub fn skies(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join("Skies"))
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.to_lowercase().ends_with(".d3s")).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Vehicle ids available in a city (its `.vvs`, plus the `.vvv` extras), sorted and without repeats. Empty for an
/// unknown city type.
pub fn vehicle_ids(root: &Path, city_type: i32) -> Vec<u32> {
    let Some(city) = crate::vehicle3d::city_name(city_type) else { return Vec::new() };
    let lower = city.to_lowercase();
    let mut ids = Vec::new();
    for ext in ["vvs", "vvv"] {
        if let Ok(bytes) = std::fs::read(root.join("Vehicles").join(format!("{lower}.{ext}"))) {
            if let Ok(c) = Chunk::parse(&bytes) {
                for h in vehicle::load_hierarchies(&c) {
                    if !ids.contains(&h.uid) {
                        ids.push(h.uid);
                    }
                }
            }
        }
    }
    ids.sort_unstable();
    ids
}

/// Skin hashes listed by the `UPXD` table of the mission's `.dam` file (`Missions\<stem>.dam`). Empty when the file is missing.
pub fn mission_skins(root: &Path, stem: &str) -> Vec<u32> {
    let Ok(bytes) = std::fs::read(root.join("Missions").join(format!("{stem}.dam"))) else { return Vec::new() };
    let Ok(c) = Chunk::parse(&bytes) else { return Vec::new() };
    let Some(Ok(k)) = c.entries.first().map(|e| e.children()) else { return Vec::new() };
    // keeps the `mdpc` import in use; the models themselves are loaded by the preview
    let _ = mdpc::load_packages; // (models are loaded on demand by the preview)
    let skins = k.find("UPXD").next().map(|u| crate::character3d::skins(u.data)).unwrap_or_default();
    skins
}

/// Display name of a skin hash: its name in `SKINS` without the `SKIN_` prefix, or the hash in hex when it is not in the table.
pub fn skin_name(hash: u32) -> String {
    SKINS.iter().find(|s| s.1 == hash).map(|s| s.0.trim_start_matches("SKIN_").to_string()).unwrap_or_else(|| format!("{hash:#010x}"))
}
