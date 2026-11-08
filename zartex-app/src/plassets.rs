//! Parallel Lines assets: the models and textures of a city's `.d4c` package files, read a package at a time.
//!
//! What it does: `PlStore` reads one package at a time from a city file, for the Assets and model preview of the editor
//! (`texture_image`, `model_scene`). `PlWorld` opens a whole Parallel Lines city (its terrain tiles, props, interiors and
//! roads), keeps the packages it needs in memory, and builds the geometry near a point (`scene`), the minimap picture
//! (`plan`), and the ground height used to place props (`ground_height`).
//! Uses: `zartex_core::plmodel` (reads the `.d4c` format), `city3d` (`Scene`, `Tex`, `Plan`, `Camera`, `render`), and
//! `vehicle3d` (`VehicleModel`). Used by: `view3d` (the Parallel Lines view and its minimap) and `main.rs` (the Assets tab
//! and the Parallel Lines preview; `PlStore` is a field of the app).

use crate::city3d::{Scene, Tex};
use crate::vehicle3d::VehicleModel;
use eframe::egui::ColorImage;
use glam::Vec3;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::rc::Rc;
use zartex_core::plmodel::{self, PackageRef, PlPackage};

/// Reader of the packages of one Parallel Lines city file, one package at a time.
#[derive(Default)]
pub struct PlStore {
    /// The city file that is open.
    path: Option<PathBuf>,
    /// Package table of the open city file (one entry per package).
    pub index: Vec<PackageRef>,
    /// Error from the last `open`, with the file name.
    pub error: Option<String>,
    /// Packages read so far, by index.
    pkgs: HashMap<usize, Rc<PlPackage>>,
}

/// The `*_pc.d4c` city files of a Parallel Lines folder (the `Cities` subfolder), sorted by name. Empty when the
/// folder cannot be read.
pub fn city_files(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root.join("Cities"))
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().ends_with("_pc.d4c")).unwrap_or(false)).collect())
        .unwrap_or_default();
    v.sort();
    v
}

impl PlStore {
    /// Reads the package table of a city file (only when the file changes). A read error is kept in `error`
    /// and clears the table.
    pub fn open(&mut self, path: &Path) {
        if self.path.as_deref() == Some(path) {
            return;
        }
        self.path = Some(path.to_path_buf());
        self.pkgs.clear();
        match plmodel::index_packages(path) {
            Ok(i) => {
                self.index = i;
                self.error = None;
            }
            Err(e) => {
                self.index.clear();
                self.error = Some(format!("{}: {e}", path.display()));
            }
        }
    }

    /// The package with index `i` in the table, read on first use and kept for later calls.
    pub fn package(&mut self, i: usize) -> Result<Rc<PlPackage>, String> {
        if let Some(p) = self.pkgs.get(&i) {
            return Ok(p.clone());
        }
        let path = self.path.clone().ok_or("no city file")?;
        let r = self.index.get(i).ok_or("no such package")?.clone();
        let p = Rc::new(plmodel::load_package(&path, &r)?);
        self.pkgs.insert(i, p.clone());
        Ok(p)
    }

    /// A texture as an egui image (mip 0).
    pub fn texture_image(&mut self, pkg: usize, tex: usize) -> Result<ColorImage, String> {
        let p = self.package(pkg)?;
        let t = p.textures.get(tex).ok_or("no such texture")?;
        let rgba = plmodel::decode_rgba(t);
        Ok(ColorImage::from_rgba_unmultiplied([t.width as usize, t.height as usize], &rgba))
    }

    /// A model as a scene for the model box. Materials resolve in the package first, then in the global one (GEGR).
    /// Errors when the model has no triangles.
    pub fn model_scene(&mut self, pkg: usize, model: usize) -> Result<VehicleModel, String> {
        let p = self.package(pkg)?;
        let global = if pkg != 0 { self.package(0).ok() } else { None };
        let m = p.models.get(model).ok_or("no such model")?;
        let mut scene = Scene { verts: vec![], uvs: vec![], tri_tex: vec![], textures: vec![], instances: 1, load_ms: 0, water_level: None };
        let mut cache: HashMap<(bool, usize), u32> = HashMap::new();
        for sm in &m.submodels {
            let use_global = sm.material.1 == 0 && pkg != 0;
            let src: &PlPackage = if use_global { global.as_deref().unwrap_or(&p) } else { &p };
            let mut tex_id = u32::MAX;
            if sm.has_material() {
                if let Some(sub) = src.materials.get(sm.material.0 as usize).and_then(|mt| mt.substances.first()) {
                    if let Some(&ti) = sub.tex_refs.first() {
                        let id = *cache.entry((use_global, ti)).or_insert_with(|| match src.textures.get(ti) {
                            Some(t) => {
                                let rgba = plmodel::decode_rgba(t);
                                let n = (rgba.len() / 4).max(1) as u64;
                                let mut sum = [0u64; 3];
                                for px in rgba.chunks_exact(4) {
                                    for k in 0..3 {
                                        sum[k] += px[k] as u64;
                                    }
                                }
                                let alpha = rgba.chunks_exact(4).any(|px| px[3] < 250);
                                scene.textures.push(Tex { w: t.width as usize, h: t.height as usize, rgba, alpha, avg: [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8] });
                                scene.textures.len() as u32 - 1
                            }
                            None => u32::MAX,
                        });
                        if id != u32::MAX {
                            tex_id = id | if sub.alpha_cutout { 1u32 << 31 } else { 0 };
                        }
                    }
                }
            }
            for t in sm.indices.chunks_exact(3) {
                for &i in t {
                    let q = sm.positions[i as usize];
                    // the data is Y-up already (terrain tiles carry their height in Y) and in world coordinates
                    scene.verts.push(Vec3::new(q[0], q[1], q[2]));
                    scene.uvs.push(sm.uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
                }
                scene.tri_tex.push(tex_id);
            }
        }
        if scene.verts.is_empty() {
            return Err("This model has no triangles.".into());
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for v in &scene.verts {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        let parts = m.submodels.len();
        Ok(VehicleModel { scene, center: (lo + hi) * 0.5, radius: ((hi - lo).length() * 0.5).max(0.05), parts })
    }
}

/// The Parallel Lines city of one `.d4c`: the terrain tiles, the props of every region, and the global package they
/// come from. Building it parses the package (slow: run it in a thread); `scene` then builds what is near a point.
pub struct PlWorld {
    /// The city file the world was read from.
    path: PathBuf,
    /// Package table of the city file (super regions).
    index: Vec<plmodel::PackageRef>,
    /// Packages of the interiors, referenced by `INTERIOR_PKG_BASE` plus their position.
    interiors: Vec<plmodel::PackageRef>,
    /// Road polylines (X, Z)
    pub roads: Vec<Vec<[f32; 2]>>,
    /// Loaded packages (the global one stays; the super regions are dropped when too many are open)
    pkgs: std::sync::Mutex<HashMap<usize, Arc<PlPackage>>>,
    /// Terrain tiles: model index and world offset (x, z) of each.
    tiles: Vec<plmodel::TilePlacement>,
    /// Props (small placed models) with their package, model, position and yaw.
    props: Vec<plmodel::PropPlacement>,
    /// Props by 128 m cell
    prop_grid: HashMap<(i32, i32), Vec<u32>>,
    /// Lowest upward-facing surface per 8 m cell, from the tiles (props stand on it)
    ground: HashMap<(i32, i32), f32>,
    /// Decoded textures, by (package, texture index); `None` for a texture that cannot be decoded.
    tex: std::sync::Mutex<HashMap<(usize, usize), Option<Tex>>>,
}

/// Size of a ground cell, in metres, for the height lookup.
const GROUND_CELL: f32 = 8.0;

impl PlWorld {
    /// Reads a Parallel Lines city file: its terrain tiles, props, interior props, interior index, roads and the global
    /// package (package 0). Slow, because the global package is parsed here; call it in a thread.
    pub fn open(city_file: &Path) -> Result<PlWorld, String> {
        let tiles = plmodel::read_tiles(city_file)?;
        let mut props = plmodel::read_props(city_file).unwrap_or_default();
        props.extend(plmodel::read_interior_props(city_file).unwrap_or_default());
        let interiors = plmodel::index_interiors(city_file).unwrap_or_default();
        let roads = plmodel::read_roads(city_file).unwrap_or_default();
        let idx = plmodel::index_packages(city_file)?;
        let pkg = Arc::new(plmodel::load_package(city_file, idx.first().ok_or("no global package")?)?);
        // ---- ground: the lowest upward-facing triangle height per 8 m cell, taken from the terrain tiles
        let mut ground: HashMap<(i32, i32), f32> = HashMap::new();
        for t in &tiles {
            let Some(m) = pkg.models.get(t.model as usize) else { continue };
            for sm in &m.submodels {
                for tri in sm.indices.chunks_exact(3) {
                    let p: Vec<[f32; 3]> = tri.iter().map(|&i| sm.positions[i as usize]).collect();
                    let n = (Vec3::from(p[1]) - Vec3::from(p[0])).cross(Vec3::from(p[2]) - Vec3::from(p[0]));
                    // only nearly horizontal triangles (normal within about 45 degrees of vertical) count as ground
                    if n.length() < 1e-6 || (n.y.abs() / n.length()) < 0.7 {
                        continue;
                    }
                    for q in &p {
                        let key = (((q[0] + t.x) / GROUND_CELL).floor() as i32, ((q[2] + t.z) / GROUND_CELL).floor() as i32);
                        let e = ground.entry(key).or_insert(f32::MAX);
                        *e = e.min(q[1]);
                    }
                }
            }
        }
        // ---- index: props are sorted into 128 m cells so that a point's neighbourhood is found quickly
        let mut prop_grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, p) in props.iter().enumerate() {
            prop_grid.entry(((p.x / 128.0).floor() as i32, (p.z / 128.0).floor() as i32)).or_default().push(i as u32);
        }
        let mut pkgs = HashMap::new();
        pkgs.insert(0usize, pkg);
        Ok(PlWorld { path: city_file.to_path_buf(), index: idx, interiors, roads, pkgs: std::sync::Mutex::new(pkgs), tiles, props, prop_grid, ground, tex: Default::default() })
    }

    /// A top-down picture of the tiles for the minimap, with the world bounds it covers. It is rendered from a camera
    /// 5800 units above the fixed point (250, 800) looking straight down, with an 80 degree field of view, into 1024 by 1024 pixels.
    pub fn plan(&self) -> crate::city3d::Plan {
        // only the tiles are drawn (no props: the radius is 0 and `with_props` is false)
        let scene = self.scene([0.0; 3], 0.0, false);
        let (cx, cz) = (250.0f32, 800.0f32);
        let mut cam = crate::city3d::Camera::orbit(Vec3::new(cx, 0.0, cz));
        cam.yaw = 0.0;
        cam.pitch = 1.5707;
        cam.dist = 5800.0;
        cam.fov_deg = 80.0;
        let half = 5800.0 * (40.0f32).to_radians().tan();
        let img = crate::city3d::render(&scene, &cam, 1024, 1024);
        crate::city3d::Plan { img, min: [cx - half, cz - half], max: [cx + half, cz + half] }
    }

    /// Package `k` of the world, read on first use. Indexes from `INTERIOR_PKG_BASE` up are interior packages. When more
    /// than five packages are open, all but the global one are dropped first.
    fn package(&self, k: usize) -> Option<Arc<PlPackage>> {
        let mut m = self.pkgs.lock().unwrap();
        if let Some(p) = m.get(&k) {
            return Some(p.clone());
        }
        if m.len() > 5 {
            m.retain(|&i, _| i == 0);
        }
        let r = if k >= plmodel::INTERIOR_PKG_BASE as usize { self.interiors.get(k - plmodel::INTERIOR_PKG_BASE as usize)? } else { self.index.get(k)? };
        let p = Arc::new(plmodel::load_package(&self.path, r).ok()?);
        m.insert(k, p.clone());
        Some(p)
    }

    /// Height of the ground at (x, z), from the tiles (0 when nothing is known there).
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        self.ground_at(x, z)
    }

    /// Looks for the lowest ground height in the 8 m cell of (x, z), then in the rings of cells around it (up to three
    /// cells away), and returns the first height found; 0 when there is none.
    fn ground_at(&self, x: f32, z: f32) -> f32 {
        let (cx, cz) = ((x / GROUND_CELL).floor() as i32, (z / GROUND_CELL).floor() as i32);
        for r in 0..=3 {
            let mut best = f32::MAX;
            for dx in -r..=r {
                for dz in -r..=r {
                    if let Some(&h) = self.ground.get(&(cx + dx, cz + dz)) {
                        best = best.min(h);
                    }
                }
            }
            if best < f32::MAX {
                return best;
            }
        }
        0.0
    }

    /// Every terrain tile, and the props within `radius` metres (X and Z) of `center` when `with_props`. Props are
    /// placed on the ground (or at their own height), turned by their yaw, and stop at 25000 props.
    pub fn scene(&self, center: [f32; 3], radius: f32, with_props: bool) -> Scene {
        // ---- start: empty scene, and a per-call cache of textures already added to it
        let t0 = std::time::Instant::now();
        let mut scene = Scene { verts: vec![], uvs: vec![], tri_tex: vec![], textures: vec![], instances: 0, load_ms: 0, water_level: None };
        let mut local: HashMap<(usize, usize), u32> = HashMap::new();
        let mut tex_cache = self.tex.lock().unwrap();
        // adds the triangles of one model to `scene`; `place` maps each model vertex to a world position
        let mut add_model = |scene: &mut Scene, pk: usize, model: usize, place: &dyn Fn([f32; 3]) -> Vec3| {
            let Some(pkg) = self.package(pk) else { return };
            let Some(m) = pkg.models.get(model) else { return };
            let global = if pk != 0 { self.package(0) } else { None };
            for sm in &m.submodels {
                let mut tex_id = u32::MAX;
                if sm.has_material() {
                    // material UID 0 names the global package; anything else is this package's own
                    let (mk, src): (usize, &PlPackage) = if sm.material.1 == 0 && pk != 0 { (0, global.as_deref().unwrap_or(&pkg)) } else { (pk, &pkg) };
                    if let Some(sub) = src.materials.get(sm.material.0 as usize).and_then(|mt| mt.substances.first()) {
                        if let Some(&ti) = sub.tex_refs.first() {
                            let id = *local.entry((mk, ti)).or_insert_with(|| {
                                let t = tex_cache.entry((mk, ti)).or_insert_with(|| src.textures.get(ti).map(make_tex));
                                match t {
                                    Some(t) => {
                                        scene.textures.push(Tex { w: t.w, h: t.h, rgba: t.rgba.clone(), alpha: t.alpha, avg: t.avg });
                                        scene.textures.len() as u32 - 1
                                    }
                                    None => u32::MAX,
                                }
                            });
                            if id != u32::MAX {
                                tex_id = id | if sub.alpha_cutout { 1u32 << 31 } else { 0 };
                            }
                        }
                    }
                }
                for tri in sm.indices.chunks_exact(3) {
                    for &i in tri {
                        scene.verts.push(place(sm.positions[i as usize]));
                        scene.uvs.push(sm.uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
                    }
                    scene.tri_tex.push(tex_id);
                }
            }
        };
        // ---- terrain tiles: all of them, from package 0
        for t in &self.tiles {
            let (tx, tz) = (t.x, t.z);
            add_model(&mut scene, 0, t.model as usize, &move |q| Vec3::new(q[0] + tx, q[1], q[2] + tz));
        }
        scene.instances = self.tiles.len();
        // ---- props near the centre, found through the 128 m cells
        if with_props {
            let (c0, c1) = (((center[0] - radius) / 128.0).floor() as i32, ((center[0] + radius) / 128.0).floor() as i32);
            let (r0, r1) = (((center[2] - radius) / 128.0).floor() as i32, ((center[2] + radius) / 128.0).floor() as i32);
            let mut n = 0;
            'outer: for cx in c0..=c1 {
                for cz in r0..=r1 {
                    let Some(list) = self.prop_grid.get(&(cx, cz)) else { continue };
                    for &pi in list {
                        let p = &self.props[pi as usize];
                        if (p.x - center[0]).abs() > radius || (p.z - center[2]).abs() > radius {
                            continue;
                        }
                        // height: the prop's own, or the ground under it; then rotation by its yaw
                        let y = p.y.unwrap_or_else(|| self.ground_at(p.x, p.z));
                        let (s, c) = p.yaw.sin_cos();
                        let (px, pz) = (p.x, p.z);
                        add_model(&mut scene, p.pkg as usize, p.model as usize, &move |q| Vec3::new(px + q[0] * c + q[2] * s, y + q[1], pz - q[0] * s + q[2] * c));
                        n += 1;
                        if n >= 25000 {
                            break 'outer;
                        }
                    }
                }
            }
            scene.instances += n;
        }
        scene.load_ms = t0.elapsed().as_millis();
        scene
    }
}

/// Decodes a Parallel Lines texture (mip 0) into a `Tex`, with its average colour and whether some pixel is not fully opaque.
fn make_tex(t: &plmodel::PlTexture) -> Tex {
    let rgba = plmodel::decode_rgba(t);
    let n = (rgba.len() / 4).max(1) as u64;
    let mut sum = [0u64; 3];
    for px in rgba.chunks_exact(4) {
        for k in 0..3 {
            sum[k] += px[k] as u64;
        }
    }
    let alpha = rgba.chunks_exact(4).any(|px| px[3] < 250);
    Tex { w: t.width as usize, h: t.height as usize, rgba, alpha, avg: [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8] }
}
