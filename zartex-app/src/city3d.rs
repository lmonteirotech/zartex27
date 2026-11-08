//! Real city geometry (Driv3r `.d3c` + `SuperRegions.pcs`) around a point, plus a small z-buffered, textured software renderer.
//! Loading and decoding come from the `formats` crate (game engine Rust Rework research).
//!
//! What it exposes: `CityStore` (the city files in memory; `region` builds the triangles near a point, `plan` builds the
//! minimap picture), `Scene` (triangles in world space with their textures), `Camera`, `Lighting`, `load_sky`, and the
//! renderers `render`, `render_lit` and `render_vp`. The renderers try the GPU hook first (see `gpu`) and draw on the
//! CPU when it returns nothing.
//! Uses: `formats` (`chnk`, `d3c`, `dds`, `mdpc`, `aiex`) to read the files; `eframe` for `ColorImage`; `glam` for maths.
//! Used by: `view3d` (City 3D tab and minimap), `gpu` (GPU renderer), `plassets`, `vehicle3d`, `character3d`, `modelbox`,
//! `mood` (builds `Lighting`), `main.rs` (declares the module), and the tools in `src\bin\` that include this file.

use eframe::egui::ColorImage;
use formats::chnk::Chunk;
use formats::d3c::{City, Scope};
use formats::dds::{self, Format};
use formats::mdpc::{self, Package};
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// Folder name of a city (for example `Miami_DAY_PC`) for a mission's city type 1 to 6; `None` for any other type.
pub fn city_dir(city_type: i32) -> Option<&'static str> {
    Some(match city_type {
        1 => "Miami_DAY_PC",
        2 => "Miami_NIGHT_PC",
        3 => "Nice_DAY_PC",
        4 => "Nice_NIGHT_PC",
        5 => "Istanbul_DAY_PC",
        6 => "Istanbul_NIGHT_PC",
        _ => return None,
    })
}

/// A decoded texture (one mip level, at most 128 pixels on a side).
pub struct Tex {
    /// Width in pixels.
    pub w: usize,
    /// Height in pixels.
    pub h: usize,
    /// Pixels, 4 bytes each (red, green, blue, alpha), row by row from the top.
    pub rgba: Vec<u8>,
    /// True when some pixel is not fully opaque.
    pub alpha: bool,
    /// Mean colour, used when a texture is missing a detail the renderer needs.
    pub avg: [u8; 3],
}

/// Texture index meaning "this triangle has no texture".
const NO_TEX: u32 = u32::MAX;
/// High 16 bits of a material handle that mean "no material".
const MATERIAL_NONE: u32 = 0xCCCC;
/// High 16 bits of a material handle that mean "the material is in the package being drawn".
const MATERIAL_LOCAL: u32 = 0xFFFD;

/// The GPU path (installed by the app when a usable Vulkan device exists): returns `None` to fall back to the CPU.
/// Arguments: the scene, the view-projection matrix, the eye position, width, height, the sky scene and the lighting.
pub type GpuHook = fn(&Scene, &Mat4, Vec3, usize, usize, Option<&Scene>, Option<&Lighting>) -> Option<ColorImage>;
// the hook is set once at start-up; later calls to `set_gpu_hook` are ignored
static GPU_HOOK: std::sync::OnceLock<GpuHook> = std::sync::OnceLock::new();

/// Installs the GPU renderer used by `render_vp`. Only the first call has an effect.
pub fn set_gpu_hook(h: GpuHook) {
    let _ = GPU_HOOK.set(h);
}

/// Mission lighting (from the mood file): ambient + one directional light, and distance fog.
#[derive(Clone, Debug, PartialEq)]
pub struct Lighting {
    /// Ambient light colour (RGB, 0 to 1).
    pub ambient: [f32; 3],
    /// Colour of the directional light (RGB, 0 to 1).
    pub dir_col: [f32; 3],
    /// direction the light travels
    pub dir_vec: Vec3,
    /// Distance at which the fog starts.
    pub fog_near: f32,
    /// Distance at which the fog reaches its far amount.
    pub fog_far: f32,
    /// fog amount (percent) at the near and far distances
    pub fog_near_v: f32,
    pub fog_far_v: f32,
    /// Fog colour (RGB, 0 to 1).
    pub fog_col: [f32; 3],
}

/// Triangles in world space (3 vertices per triangle).
#[derive(Default)]
pub struct Scene {
    /// Vertex positions; triangle `t` uses entries `3t` to `3t + 2`.
    pub verts: Vec<Vec3>,
    /// Texture coordinate of each vertex, in the same order as `verts`.
    pub uvs: Vec<[f32; 2]>,
    /// Texture of each triangle (`NO_TEX` when it has none)
    pub tri_tex: Vec<u32>,
    /// Decoded textures that `tri_tex` refers to.
    pub textures: Vec<Tex>,
    /// Number of placed models (instances) that were included.
    pub instances: usize,
    /// Time taken to build the scene, in milliseconds.
    pub load_ms: u128,
    /// Height of the sea (the city's water tiles): drawn as a water surface by the GPU renderer only.
    pub water_level: Option<f32>,
}

/// City files kept in memory so that moving the area is cheap.
pub struct CityStore {
    /// Contents of the city's `.d3c` file (instance list).
    d3c: Vec<u8>,
    /// Contents of the `SuperRegions.pcs` file (model packages).
    pcs: Vec<u8>,
    /// `Interiors.pcs` (rooms inside buildings); empty when the city has none
    ipcs: Vec<u8>,
}

/// Converts a 16-bit RGB565 colour to 8-bit RGB, repeating the top bits in the low bits so that full
/// intensity maps to 255.
fn rgb565(c: u16) -> [u8; 3] {
    let (r, g, b) = ((c >> 11) & 31, (c >> 5) & 63, c & 31);
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

/// Decodes one 8-byte BC1 colour block into 16 RGBA pixels. With `with_alpha`, a block whose first colour value is not
/// larger than the second has a transparent index (3); otherwise the block is opaque.
fn decode_bc1_block(b: &[u8], out: &mut [[u8; 4]; 16], with_alpha: bool) {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    let mix = |a: u8, b: u8, wa: u32, wb: u32, d: u32| ((a as u32 * wa + b as u32 * wb) / d) as u8;
    if c0 > c1 || !with_alpha {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 2, 1, 3);
            pal[3][k] = mix(p0[k], p1[k], 1, 2, 3);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 1, 1, 2);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
}

/// Decodes one mip level into RGBA.
fn decode_level(fmt: Format, data: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgba = vec![255u8; w * h * 4];
    match fmt {
        Format::Bgra8 => {
            for i in 0..w * h {
                let s = &data[i * 4..i * 4 + 4];
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
        }
        _ => {
            let bw = w.div_ceil(4);
            let bh = h.div_ceil(4);
            let bsize = if fmt == Format::Bc1 { 8 } else { 16 };
            for by in 0..bh {
                for bx in 0..bw {
                    let o = (by * bw + bx) * bsize;
                    let Some(blk) = data.get(o..o + bsize) else { continue };
                    let mut px = [[0u8; 4]; 16];
                    match fmt {
                        Format::Bc1 => decode_bc1_block(blk, &mut px, true),
                        // BC2: 16 explicit 4-bit alpha values in the first 8 bytes, colour from the BC1 block after them
                        Format::Bc2 => {
                            decode_bc1_block(&blk[8..], &mut px, false);
                            for (i, p) in px.iter_mut().enumerate() {
                                let nib = (blk[i / 2] >> (4 * (i & 1))) & 15;
                                p[3] = nib * 17;
                            }
                        }
                        // BC3: two 8-bit alpha end values and 16 3-bit indices (6 bytes) in the first 8 bytes, colour from
                        // the BC1 block after them
                        _ => {
                            decode_bc1_block(&blk[8..], &mut px, false);
                            let (a0, a1) = (blk[0] as u32, blk[1] as u32);
                            let mut al = [0u32; 8];
                            al[0] = a0;
                            al[1] = a1;
                            if a0 > a1 {
                                for k in 1..7 {
                                    al[k + 1] = ((7 - k as u32) * a0 + k as u32 * a1) / 7;
                                }
                            } else {
                                for k in 1..5 {
                                    al[k + 1] = ((5 - k as u32) * a0 + k as u32 * a1) / 5;
                                }
                                al[6] = 0;
                                al[7] = 255;
                            }
                            let mut bits = 0u64;
                            for k in 0..6 {
                                bits |= (blk[2 + k] as u64) << (8 * k);
                            }
                            for (i, p) in px.iter_mut().enumerate() {
                                p[3] = al[((bits >> (3 * i)) & 7) as usize] as u8;
                            }
                        }
                    }
                    for y in 0..4 {
                        for x in 0..4 {
                            let (X, Y) = (bx * 4 + x, by * 4 + y);
                            if X < w && Y < h {
                                rgba[(Y * w + X) * 4..(Y * w + X) * 4 + 4].copy_from_slice(&px[y * 4 + x]);
                            }
                        }
                    }
                }
            }
        }
    }
    rgba
}

/// Decodes a DDS texture into RGBA, using the first mip level whose larger side is at most 256 pixels, and computes
/// its average colour. `None` when the file cannot be read, a block-compressed texture has a size that is not a
/// multiple of 4, or the pixel data is too short.
fn make_tex(bytes: &[u8]) -> Option<Tex> {
    let d = dds::parse(bytes)?;
    if d.format.is_block() && (d.width % 4 != 0 || d.height % 4 != 0) {
        return None;
    }
    // walk down the mip chain until the larger side is at most 256 px
    let (mut w, mut h) = (d.width, d.height);
    let mut off = 0usize;
    let mut level = 0;
    while level + 1 < d.mips && w.max(h) > 256 {
        off += d.format.level_size(w, h);
        w = (w / 2).max(1);
        h = (h / 2).max(1);
        level += 1;
    }
    let size = d.format.level_size(w, h);
    let rgba = decode_level(d.format, d.data.get(off..off + size)?, w as usize, h as usize);
    let alpha = rgba.chunks_exact(4).any(|p| p[3] < 250);
    let n = (rgba.len() / 4).max(1) as u64;
    let mut sum = [0u64; 3];
    for p in rgba.chunks_exact(4) {
        for k in 0..3 {
            sum[k] += p[k] as u64;
        }
    }
    Some(Tex { w: w as usize, h: h as usize, rgba, alpha, avg: [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8] })
}

/// Texture of a submodel material handle (`uid << 16 | material index`), cached per (package, texture).
/// The texture is decoded once and pushed to `out`; the returned value is its index in `out` with bit 31 set for a
/// cut-out material, or `NO_TEX` when there is none. `cache` maps (package, texture) to that result.
pub fn texture_for(packages: &[Package], uid_to_pkg: &HashMap<u32, usize>, own: usize, handle: u32, cache: &mut HashMap<(usize, usize), u32>, out: &mut Vec<Tex>) -> u32 {
    // bit 31 marks a cut-out material (the substance has the alpha-channel flag 0x4); other alphas are masks, not transparency
    let cut = |sub_flags: u32| if sub_flags & 0x4 != 0 { 1u32 << 31 } else { 0 };
    let hi = handle >> 16;
    if hi == MATERIAL_NONE {
        return NO_TEX;
    }
    let pi = if hi == packages[own].uid || hi == MATERIAL_LOCAL { own } else { uid_to_pkg.get(&hi).copied().unwrap_or(own) };
    let pkg = &packages[pi];
    let Some(mat) = pkg.materials.get((handle & 0xFFFF) as usize) else { return NO_TEX };
    let Some(&si) = mat.substances.first() else { return NO_TEX };
    let Some(sub) = pkg.substances.get(si) else { return NO_TEX };
    let Some(&ti) = sub.textures.first() else { return NO_TEX };
    let cutbit = cut(sub.flags);
    if let Some(&id) = cache.get(&(pi, ti)) {
        return if id == NO_TEX { NO_TEX } else { id | cutbit };
    }
    let id = match pkg.texture_bytes(ti).and_then(make_tex) {
        Some(t) => {
            out.push(t);
            out.len() as u32 - 1
        }
        None => NO_TEX,
    };
    cache.insert((pi, ti), id);
    if id == NO_TEX { NO_TEX } else { id | cutbit }
}

impl CityStore {
    /// Reads the city's `.d3c`, `SuperRegions.pcs` and (when present) `Interiors.pcs` from the Driv3r game folder.
    /// `city_type` is the mission's city type (1 to 6). A missing `.d3c` or `SuperRegions.pcs` is an error;
    /// a missing `Interiors.pcs` is not.
    pub fn open(game: &Path, city_type: i32) -> Result<CityStore, String> {
        let name = city_dir(city_type).ok_or("this mission has no known city")?;
        let d3c_path = game.join("Cities").join(format!("{name}.d3c"));
        let pcs_path = game.join("Cities").join(name).join("SuperRegions.pcs");
        let ipcs = std::fs::read(game.join("Cities").join(name).join("Interiors.pcs")).unwrap_or_default();
        Ok(CityStore {
            ipcs,
            d3c: std::fs::read(&d3c_path).map_err(|e| format!("{}: {e}", d3c_path.display()))?,
            pcs: std::fs::read(&pcs_path).map_err(|e| format!("{}: {e}", pcs_path.display()))?,
        })
    }

    /// Geometry of the exterior and interior instances whose XZ position is within `radius` of `center`. A model that
    /// is large (a terrain tile, a long wall) is included when any part of it is in range. Returns a `Scene` with
    /// the triangles in world space.
    pub fn region(&self, center: [f32; 3], radius: f32) -> Result<Scene, String> {
        // ---- reading the files
        let t0 = std::time::Instant::now();
        let d3c_root = Chunk::parse(&self.d3c).map_err(|e| e.to_string())?;
        let pcs_root = Chunk::parse(&self.pcs).map_err(|e| e.to_string())?;
        let city = City::parse(&d3c_root);
        let mut packages = mdpc::load_packages(&pcs_root).map_err(|e| e.to_string())?;
        let uid_to_pkg: HashMap<u32, usize> = packages.iter().enumerate().map(|(i, p)| (p.uid, i)).collect();
        // ---- interior packages
        // Interiors.pcs: one "external" package (uid 0x3B) per interior in IRCT order, and "internal" ones (uid 0x1E)
        // for the interiors that have one; the uids repeat, so interior instances pick theirs by interior index.
        let mut ext: Vec<usize> = Vec::new();
        let mut int: Vec<usize> = Vec::new();
        if let Ok(root) = Chunk::parse(&self.ipcs) {
            for p in mdpc::load_packages(&root).map_err(|e| e.to_string())? {
                match p.uid {
                    0x3B => ext.push(packages.len()),
                    0x1E => int.push(packages.len()),
                    _ => {}
                }
                packages.push(p);
            }
        }
        let mut with_internal: Vec<u32> = city.instances.iter().filter(|i| i.scope == Scope::Interior && i.package() == 0x1E).map(|i| i.interior).collect();
        with_internal.sort_unstable();
        with_internal.dedup();
        let one_to_one = int.len() == ext.len();
        // finds the package (index into `packages`) that holds the model of an instance
        let resolve = |i: &formats::d3c::Instance| -> Option<usize> {
            if i.scope == Scope::Interior {
                match i.package() {
                    0x3B => ext.get(i.interior as usize).copied(),
                    0x1E if one_to_one => int.get(i.interior as usize).copied(),
                    0x1E => with_internal.iter().position(|&k| k == i.interior).and_then(|j| int.get(j).copied()),
                    p => uid_to_pkg.get(&p).copied(),
                }
            } else {
                uid_to_pkg.get(&i.package()).copied()
            }
        };
        // ---- building the triangles, one placed instance at a time
        let mut scene = Scene::default();
        let mut cache: HashMap<(usize, usize), u32> = HashMap::new();
        for inst in &city.instances {
            if inst.is_placeholder() {
                continue;
            }
            let Some(pi) = resolve(inst) else { continue };
            let pkg = &packages[pi];
            let Some(model) = pkg.models.get(inst.model() as usize) else { continue };
            if model.vb_index < 0 {
                continue;
            }
            // a big model (terrain tile, long wall) matters when any part of it is in range, not only its origin
            let extent = model.bbox.iter().filter(|c| c.iter().all(|v| v.is_finite() && v.abs() < 1e5)).map(|c| (c[0] * c[0] + c[2] * c[2]).sqrt()).fold(0.0f32, f32::max).min(400.0);
            let (dx, dz) = (inst.pos[0] - center[0], inst.pos[2] - center[2]);
            if (dx * dx + dz * dz).sqrt() - extent > radius {
                continue;
            }
            // yaw-only instances turn around the Y axis; the others use their stored quaternion
            let rot = if inst.is_yaw() {
                Quat::from_rotation_y(-inst.d[1].atan2(inst.d[0]))
            } else {
                Quat::from_xyzw(inst.d[0], inst.d[1], inst.d[2], inst.s).normalize()
            };
            let world = Mat4::from_rotation_translation(rot, Vec3::from(inst.pos));
            // the most detailed LOD that has geometry (slot 0 is empty for some models)
            let Some(lod) = model.lods[..5].iter().find(|l| l.count > 0) else { continue };
            scene.instances += 1;
            for li in &pkg.lod_instances[lod.inst_first..lod.inst_first + lod.count] {
                // the stored matrix of a LOD instance is used only when bit 0 of `use_transform` is set
                let local = if li.use_transform & 1 != 0 { Mat4::from_cols_array(&li.matrix).transpose() } else { Mat4::IDENTITY };
                let m = world * local;
                for sub in &pkg.submodels[li.sub_first..li.sub_first + li.sub_count] {
                    let mut idx = Vec::new();
                    if pkg.submodel_triangles(sub, &mut idx).is_err() {
                        continue;
                    }
                    let tex = texture_for(&packages, &uid_to_pkg, pi, sub.material, &mut cache, &mut scene.textures);
                    for tri in idx.chunks_exact(3) {
                        for &i in tri {
                            match pkg.vertex(model.vb_index as usize, i as usize) {
                                Some((p, _, uv)) => {
                                    scene.verts.push(m.transform_point3(Vec3::from(p)));
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
        // the sea height is taken from the first water entry of the city file
        scene.load_ms = t0.elapsed().as_millis();
        scene.water_level = city.water.first().map(|w| w.pos[1]);
        Ok(scene)
    }
}

/// Orbit camera: it looks at `target` from `dist` units away, at the angles `yaw` (around Y) and `pitch` (up from the
/// ground plane), with a vertical field of view of `fov_deg` degrees.
#[derive(Clone, Copy)]
pub struct Camera {
    /// Point the camera looks at.
    pub target: Vec3,
    /// Angle around the Y axis, in radians.
    pub yaw: f32,
    /// Angle above the horizontal, in radians.
    pub pitch: f32,
    /// Distance from `target` to the eye.
    pub dist: f32,
    /// Vertical field of view, in degrees.
    pub fov_deg: f32,
}

impl Camera {
    /// A camera that looks at `target` from the default distance (90) and angles.
    pub fn orbit(target: Vec3) -> Camera {
        Camera { target, yaw: 0.8, pitch: 0.55, dist: 90.0, fov_deg: 55.0 }
    }
    /// Position of the eye: `target` moved `dist` away along the direction given by `yaw` and `pitch`.
    pub fn eye(&self) -> Vec3 {
        let (cy, sy, cp, sp) = (self.yaw.cos(), self.yaw.sin(), self.pitch.cos(), self.pitch.sin());
        self.target + Vec3::new(sy * cp, sp, cy * cp) * self.dist
    }
    /// Projection times view matrix for an aspect ratio (width / height). Near plane 0.5, far plane 6000.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        // right-handed, as in the game engine Rust Rework research; a camera object's stored forward vector points backwards (look = -forward)
        let v = Mat4::look_at_rh(self.eye(), self.target, Vec3::Y);
        let p = Mat4::perspective_rh(self.fov_deg.to_radians(), aspect, 0.5, 6000.0);
        p * v
    }
}

/// Screen position (pixels) and depth (0..1) of a world point, `None` when behind the camera.
pub fn project(vp: &Mat4, p: Vec3, w: f32, h: f32) -> Option<(f32, f32, f32)> {
    project4(vp, p, w, h).map(|(x, y, z, _)| (x, y, z))
}

/// As `project`, plus the clip-space w (for perspective-correct interpolation).
pub fn project4(vp: &Mat4, p: Vec3, w: f32, h: f32) -> Option<(f32, f32, f32, f32)> {
    let c = *vp * p.extend(1.0);
    if c.w <= 0.01 {
        return None;
    }
    let (x, y, z) = (c.x / c.w, c.y / c.w, c.z / c.w);
    Some(((x * 0.5 + 0.5) * w, (1.0 - (y * 0.5 + 0.5)) * h, z, c.w))
}

/// Draws a scene from an orbit camera with the default shading (no mission lighting).
pub fn render(scene: &Scene, cam: &Camera, w: usize, h: usize) -> ColorImage {
    render_lit(scene, cam, w, h, None)
}

/// As `render`, with the mission's lighting and fog when `light` is given.
pub fn render_lit(scene: &Scene, cam: &Camera, w: usize, h: usize, light: Option<&Lighting>) -> ColorImage {
    let vp = cam.view_proj(w as f32 / h as f32);
    render_vp(scene, &vp, cam.eye(), w, h, None, light)
}

/// Render with an arbitrary view-projection (used for the cutscene camera preview). `sky` is drawn first, centred on
/// the camera and scaled to sit inside the far plane, without lighting.
pub fn render_vp(scene: &Scene, vp: &Mat4, eye: Vec3, w: usize, h: usize, sky: Option<&Scene>, light: Option<&Lighting>) -> ColorImage {
    // the GPU goes first; `None` from it means "draw this frame on the CPU"
    if let Some(hook) = GPU_HOOK.get() {
        if let Some(img) = hook(scene, vp, eye, w, h, sky, light) {
            return img;
        }
    }
    let (sky_top, sky_bot) = match light {
        Some(l) => {
            let c = [(l.fog_col[0].clamp(0.0, 1.0) * 255.0) as u8, (l.fog_col[1].clamp(0.0, 1.0) * 255.0) as u8, (l.fog_col[2].clamp(0.0, 1.0) * 255.0) as u8];
            (c, c)
        }
        None => ([0xB9u8, 0xD3, 0xEA], [0xEEu8, 0xF3, 0xF8]),
    };
    // background: a vertical gradient from `sky_top` (top row) to `sky_bot` (bottom row)
    let mut color = vec![0u8; w * h * 4];
    for y in 0..h {
        let t = y as f32 / h as f32;
        for x in 0..w {
            let o = (y * w + x) * 4;
            for k in 0..3 {
                color[o + k] = (sky_top[k] as f32 * (1.0 - t) + sky_bot[k] as f32 * t) as u8;
            }
            color[o + 3] = 255;
        }
    }
    let mut depth = vec![f32::MAX; w * h];
    if let Some(sk) = sky {
        // the dome follows the camera: its vertices are around the origin
        let radius = sk.verts.iter().filter(|v| v.is_finite()).map(|v| v.length()).fold(1.0f32, f32::max);
        let k = 2500.0 / radius;
        let moved = Scene {
            verts: sk.verts.iter().map(|v| eye + *v * k).collect(),
            uvs: sk.uvs.clone(),
            tri_tex: sk.tri_tex.clone(),
            textures: Vec::new(),
            instances: 0,
            load_ms: 0,
            water_level: None,
        };
        draw_tris(&moved, &sk.textures, vp, eye, w, h, &mut color, &mut depth, true, None);
        depth.iter_mut().for_each(|d| *d = f32::MAX);
    }
    draw_tris(scene, &scene.textures, vp, eye, w, h, &mut color, &mut depth, false, light);
    ColorImage::from_rgba_unmultiplied([w, h], &color)
}

/// Rasterises the triangles of `scene` into `color` (RGBA bytes) with a depth buffer `depth`. Each triangle is clipped at
/// the near plane, then filled pixel by pixel: nearest texel of its texture (or a flat colour), cut-out pixels
/// skipped, and lit or fogged. `unlit` is used for the sky dome: no shading and no fog.
#[allow(clippy::too_many_arguments)]
fn draw_tris(scene: &Scene, textures: &[Tex], vp: &Mat4, eye: Vec3, w: usize, h: usize, color: &mut [u8], depth: &mut [f32], unlit: bool, lighting: Option<&Lighting>) {
    let vp = *vp;
    let light = Vec3::new(0.4, 0.8, 0.45).normalize();
    let (wf, hf) = (w as f32, h as f32);
    const NEAR_W: f32 = 0.05;
    type Cv = (glam::Vec4, [f32; 2]);
    for (ti, tri) in scene.verts.chunks_exact(3).enumerate() {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        let mut n = (b - a).cross(c - a);
        let len = n.length();
        if len < 1e-9 {
            continue;
        }
        n /= len;
        // the normal is turned to face the camera, so both sides of a wall are lit
        if n.dot(eye - a) < 0.0 {
            n = -n;
        }
        let cv: [Cv; 3] = [(vp * a.extend(1.0), scene.uvs[ti * 3]), (vp * b.extend(1.0), scene.uvs[ti * 3 + 1]), (vp * c.extend(1.0), scene.uvs[ti * 3 + 2])];
        if cv.iter().all(|v| v.0.w <= NEAR_W) {
            continue;
        }
        // clip against the near plane so that big floors and walls around the camera are not dropped
        let mut poly: Vec<Cv> = Vec::with_capacity(4);
        for k in 0..3 {
            let (p0, p1) = (cv[k], cv[(k + 1) % 3]);
            let (in0, in1) = (p0.0.w > NEAR_W, p1.0.w > NEAR_W);
            if in0 {
                poly.push(p0);
            }
            if in0 != in1 {
                let t = (NEAR_W - p0.0.w) / (p1.0.w - p0.0.w);
                poly.push((p0.0 + (p1.0 - p0.0) * t, [p0.1[0] + (p1.1[0] - p0.1[0]) * t, p0.1[1] + (p1.1[1] - p0.1[1]) * t]));
            }
        }
        if poly.len() < 3 {
            continue;
        }
        // without mission lighting: a fixed light direction (a half-lit range from 0.5 to 1.0)
        let lit = if unlit { 1.0 } else { 0.5 + 0.5 * n.dot(light).max(0.0) };
        // mission lighting: ambient + directional colour (as the mood file gives them) instead of the fixed shading
        let shade: [f32; 3] = match lighting {
            Some(l) if !unlit => {
                let ndl = n.dot(-l.dir_vec.normalize_or_zero()).max(0.0);
                [(l.ambient[0] + l.dir_col[0] * ndl).min(1.4), (l.ambient[1] + l.dir_col[1] * ndl).min(1.4), (l.ambient[2] + l.dir_col[2] * ndl).min(1.4)]
            }
            _ => [lit, lit, lit],
        };
        // bit 31 of the texture value marks a cut-out material (see `texture_for`)
        let raw = scene.tri_tex.get(ti).copied().filter(|&t| t != NO_TEX);
        let cutout = raw.map(|r| r >> 31 == 1).unwrap_or(false);
        let tex = raw.and_then(|r| textures.get((r & 0x7FFF_FFFF) as usize));
        // colour of an untextured triangle: grey for surfaces that face up, beige for the others
        let base = if n.y.max(0.0) > 0.85 { [0.62, 0.64, 0.66] } else { [0.80, 0.77, 0.72] };
        let flat = [(base[0] * shade[0] * 255.0).min(255.0) as u8, (base[1] * shade[1] * 255.0).min(255.0) as u8, (base[2] * shade[2] * 255.0).min(255.0) as u8];
        // screen vertices: x, y, depth, 1/w, u, v
        let sv: Vec<[f32; 6]> = poly
            .iter()
            .map(|(c, uv)| {
                let iw = 1.0 / c.w;
                [(c.x * iw * 0.5 + 0.5) * wf, (1.0 - (c.y * iw * 0.5 + 0.5)) * hf, c.z * iw, iw, uv[0], uv[1]]
            })
            .collect();
        // the clipped polygon (3 or 4 corners) is split into a fan of triangles from its first corner
        for f in 1..sv.len() - 1 {
            let (pa, pb, pc) = (sv[0], sv[f], sv[f + 1]);
            let min_x = pa[0].min(pb[0]).min(pc[0]).floor().max(0.0) as i32;
            let max_x = pa[0].max(pb[0]).max(pc[0]).ceil().min(wf - 1.0) as i32;
            let min_y = pa[1].min(pb[1]).min(pc[1]).floor().max(0.0) as i32;
            let max_y = pa[1].max(pb[1]).max(pc[1]).ceil().min(hf - 1.0) as i32;
            if min_x > max_x || min_y > max_y {
                continue;
            }
            let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
            if area.abs() < 1e-6 {
                continue;
            }
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((pb[0] - px) * (pc[1] - py) - (pb[1] - py) * (pc[0] - px)) / area;
                    let w1 = ((pc[0] - px) * (pa[1] - py) - (pc[1] - py) * (pa[0] - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    // depth test first, so that hidden pixels cost nothing more
                    let z = w0 * pa[2] + w1 * pb[2] + w2 * pc[2];
                    let i = y as usize * w + x as usize;
                    if z >= depth[i] {
                        continue;
                    }
                    // iw is the interpolated 1/w; texture coordinates are interpolated divided by w and then
                    // divided by iw again (perspective-correct texturing)
                    let iw = w0 * pa[3] + w1 * pb[3] + w2 * pc[3];
                    let col = match tex {
                        Some(t) => {
                            let u = (w0 * pa[4] * pa[3] + w1 * pb[4] * pb[3] + w2 * pc[4] * pc[3]) / iw;
                            let v = (w0 * pa[5] * pa[3] + w1 * pb[5] * pb[3] + w2 * pc[5] * pc[3]) / iw;
                            let (fu, fv) = (u - u.floor(), v - v.floor());
                            let tx = ((fu * t.w as f32) as usize).min(t.w - 1);
                            let ty = ((fv * t.h as f32) as usize).min(t.h - 1);
                            let s = &t.rgba[(ty * t.w + tx) * 4..(ty * t.w + tx) * 4 + 4];
                            if cutout && t.alpha && s[3] < 128 {
                                continue;
                            }
                            [(s[0] as f32 * shade[0]).min(255.0) as u8, (s[1] as f32 * shade[1]).min(255.0) as u8, (s[2] as f32 * shade[2]).min(255.0) as u8]
                        }
                        None => flat,
                    };
                    let mut col = col;
                    if let (Some(l), false) = (lighting, unlit) {
                        // distance fog toward the mood's fog colour
                        let d = 1.0 / iw;
                        let t = ((d - l.fog_near) / (l.fog_far - l.fog_near).max(1.0)).clamp(0.0, 1.0);
                        let a = ((l.fog_near_v + (l.fog_far_v - l.fog_near_v) * t) / 100.0).clamp(0.0, 1.0);
                        for k in 0..3 {
                            col[k] = (col[k] as f32 * (1.0 - a) + l.fog_col[k].clamp(0.0, 1.0) * 255.0 * a) as u8;
                        }
                    }
                    depth[i] = z;
                    color[i * 4..i * 4 + 3].copy_from_slice(&col);
                }
            }
        }
    }
}

/// The sky dome model of a mood (`Skies\<name>.d3s`): one MDPC package whose first model is the dome.
/// `name` may include a folder path; only its last part is used. Errors when the file or the model is missing or empty.
pub fn load_sky(game: &Path, name: &str) -> Result<Scene, String> {
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let path = game.join("Skies").join(file);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let root = Chunk::parse(&bytes).map_err(|e| e.to_string())?;
    let pk = mdpc::load_packages(&root).map_err(|e| e.to_string())?;
    let pkg = pk.first().ok_or("empty sky file")?;
    let model = pkg.models.iter().find(|m| m.vb_index >= 0 && m.lods[..5].iter().any(|l| l.count > 0)).ok_or("no sky model")?;
    let lod = model.lods[..5].iter().find(|l| l.count > 0).unwrap();
    // the sky file has one package, so no other package can be referred to
    let uid_map: HashMap<u32, usize> = HashMap::new();
    let mut scene = Scene::default();
    let mut cache: HashMap<(usize, usize), u32> = HashMap::new();
    for li in &pkg.lod_instances[lod.inst_first..lod.inst_first + lod.count] {
        let local = if li.use_transform & 1 != 0 { Mat4::from_cols_array(&li.matrix).transpose() } else { Mat4::IDENTITY };
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
                            scene.verts.push(local.transform_point3(Vec3::from(p)));
                            scene.uvs.push(uv);
                        }
                        None => {
                            scene.verts.push(Vec3::ZERO);
                            scene.uvs.push([0.0, 0.0]);
                        }
                    }
                }
                // the cut-out bit is cleared for the sky (its texture is always drawn); NO_TEX keeps its value
                scene.tri_tex.push(tex & !(1u32 << 31) | if tex == NO_TEX { 1u32 << 31 } else { 0 });
            }
        }
    }
    if scene.verts.is_empty() {
        return Err("the sky model has no geometry".into());
    }
    Ok(scene)
}


// ---------------------------------------------------------------- city plan (minimap background)

/// A top-down picture of the whole city: building footprints (darker when taller) over the road network. World
/// x grows to the right, world z downwards.
pub struct Plan {
    /// The picture, in pixels.
    pub img: ColorImage,
    /// World (x, z) of the top-left corner of the picture.
    pub min: [f32; 2],
    /// World (x, z) of the bottom-right corner of the picture.
    pub max: [f32; 2],
}

/// Convex hull of 2D points (monotone chain). Returns the hull corners in order; fewer than 3 points are returned as
/// they are, after sorting and removing duplicates.
fn hull(mut p: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lo: Vec<[f32; 2]> = Vec::new();
    for &q in &p {
        while lo.len() >= 2 && cross(lo[lo.len() - 2], lo[lo.len() - 1], q) <= 0.0 {
            lo.pop();
        }
        lo.push(q);
    }
    let mut up: Vec<[f32; 2]> = Vec::new();
    for &q in p.iter().rev() {
        while up.len() >= 2 && cross(up[up.len() - 2], up[up.len() - 1], q) <= 0.0 {
            up.pop();
        }
        up.push(q);
    }
    lo.pop();
    up.pop();
    lo.extend(up);
    lo
}

/// Fills the convex polygon `pts` (pixel coordinates) with `col` in the RGBA image `img` (`w` pixels wide, `h` high),
/// scanline by scanline through the pixel centres.
fn fill_convex(img: &mut [u8], w: usize, h: usize, pts: &[[f32; 2]], col: [u8; 3]) {
    if pts.len() < 3 {
        return;
    }
    let (ymin, ymax) = pts.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p[1]), a.1.max(p[1])));
    for y in (ymin.floor().max(0.0) as i32)..=(ymax.ceil().min(h as f32 - 1.0) as i32) {
        let yc = y as f32 + 0.5;
        let (mut x0, mut x1) = (f32::MAX, f32::MIN);
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a[1] <= yc && b[1] > yc) || (b[1] <= yc && a[1] > yc) {
                let x = a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                x0 = x0.min(x);
                x1 = x1.max(x);
            }
        }
        if x0 <= x1 {
            for x in (x0.floor().max(0.0) as i32)..=(x1.ceil().min(w as f32 - 1.0) as i32) {
                let o = (y as usize * w + x as usize) * 4;
                img[o..o + 3].copy_from_slice(&col);
            }
        }
    }
}

/// Draws a line of `thick` by `thick` pixel squares from `a` to `b` (pixel coordinates) into the RGBA image, by
/// stepping along the line and clipping at the image edges.
fn line(img: &mut [u8], w: usize, h: usize, a: [f32; 2], b: [f32; 2], col: [u8; 3], thick: i32) {
    let n = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let (x, y) = ((a[0] + (b[0] - a[0]) * t) as i32, (a[1] + (b[1] - a[1]) * t) as i32);
        for dy in 0..thick {
            for dx in 0..thick {
                let (px, py) = (x + dx - thick / 2, y + dy - thick / 2);
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    let o = (py as usize * w + px as usize) * 4;
                    img[o..o + 3].copy_from_slice(&col);
                }
            }
        }
    }
}

impl CityStore {
    /// Builds the minimap picture of the city. Each building is the convex hull of its bounding box, projected to the
    /// ground; footprints that are very small, very large or flat are skipped. Roads (from the `aiex` data) are drawn
    /// under the buildings. The picture is 2048 pixels on its longer side, plus a 40 m margin.
    pub fn plan(&self) -> Result<Plan, String> {
        let d3c_root = Chunk::parse(&self.d3c).map_err(|e| e.to_string())?;
        let pcs_root = Chunk::parse(&self.pcs).map_err(|e| e.to_string())?;
        let city = City::parse(&d3c_root);
        let packages = mdpc::load_packages(&pcs_root).map_err(|e| e.to_string())?;
        let uid_to_pkg: HashMap<u32, usize> = packages.iter().enumerate().map(|(i, p)| (p.uid, i)).collect();
        struct Poly {
            pts: Vec<[f32; 2]>,
            height: f32,
        }
        // ---- building footprints: one convex polygon per exterior instance
        let mut polys: Vec<Poly> = Vec::new();
        for inst in &city.instances {
            if inst.scope == Scope::Interior || inst.is_placeholder() {
                continue;
            }
            let Some(&pi) = uid_to_pkg.get(&inst.package()) else { continue };
            let Some(model) = packages[pi].models.get(inst.model() as usize) else { continue };
            let rot = if inst.is_yaw() { Quat::from_rotation_y(-inst.d[1].atan2(inst.d[0])) } else { Quat::from_xyzw(inst.d[0], inst.d[1], inst.d[2], inst.s).normalize() };
            let m = Mat4::from_rotation_translation(rot, Vec3::from(inst.pos));
            let mut pts = Vec::with_capacity(8);
            let (mut y0, mut y1) = (f32::MAX, f32::MIN);
            for c in model.bbox.iter().filter(|c| c.iter().all(|v| v.is_finite() && v.abs() < 5000.0)) {
                let w = m.transform_point3(Vec3::from(*c));
                pts.push([w.x, w.z]);
                y0 = y0.min(w.y);
                y1 = y1.max(w.y);
            }
            if pts.len() < 4 {
                continue;
            }
            let hp = hull(pts);
            // area of the footprint by the shoelace formula
            let area = hp.iter().enumerate().map(|(i, a)| a[0] * hp[(i + 1) % hp.len()][1] - hp[(i + 1) % hp.len()][0] * a[1]).sum::<f32>().abs() * 0.5;
            // props and lamp posts are noise; ground tiles and the sea would cover everything
            if !(4.0..40000.0).contains(&area) || (y1 - y0) < 1.5 {
                continue;
            }
            polys.push(Poly { pts: hp, height: y1 - y0 });
        }
        // ---- bounds of the picture
        // road network of the city (None when the file has none)
        let roads = formats::aiex::load(&d3c_root);
        let (mut mn, mut mx) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in polys.iter().flat_map(|p| p.pts.iter()) {
            for k in 0..2 {
                mn[k] = mn[k].min(p[k]);
                mx[k] = mx[k].max(p[k]);
            }
        }
        if mn[0] >= mx[0] {
            return Err("no buildings found for the city plan".into());
        }
        // ---- drawing: background, roads, then buildings
        let pad = 40.0;
        let (mn, mx) = ([mn[0] - pad, mn[1] - pad], [mx[0] + pad, mx[1] + pad]);
        let (ex, ez) = (mx[0] - mn[0], mx[1] - mn[1]);
        // pixels per metre: the longer side of the city gets 2048 pixels
        let ppm = 2048.0 / ex.max(ez);
        let (w, h) = (((ex * ppm).ceil() as usize).max(8), ((ez * ppm).ceil() as usize).max(8));
        let mut img = vec![0u8; w * h * 4];
        for px in img.chunks_exact_mut(4) {
            px.copy_from_slice(&[0xE4, 0xE8, 0xEC, 0xFF]);
        }
        // world (x, z) to picture pixels
        let to_px = |p: [f32; 2]| [(p[0] - mn[0]) * ppm, (p[1] - mn[1]) * ppm];
        if let Some(a) = &roads {
            for s in 0..a.segments.len() {
                let pl = a.segment_polyline(s);
                for pair in pl.windows(2) {
                    line(&mut img, w, h, to_px([pair[0][0], pair[0][2]]), to_px([pair[1][0], pair[1][2]]), [0x8A, 0x91, 0x99], 3);
                }
            }
        }
        // low buildings first so that tall ones stay visible
        polys.sort_by(|a, b| a.height.total_cmp(&b.height));
        for p in &polys {
            let shade = 1.0 - (p.height.min(90.0) / 90.0) * 0.45;
            let col = [(0xDE as f32 * shade) as u8, (0xD2 as f32 * shade) as u8, (0xBC as f32 * shade) as u8];
            let px: Vec<[f32; 2]> = p.pts.iter().map(|q| to_px(*q)).collect();
            fill_convex(&mut img, w, h, &px, col);
        }
        Ok(Plan { img: ColorImage::from_rgba_unmultiplied([w, h], &img), min: mn, max: mx })
    }
}
