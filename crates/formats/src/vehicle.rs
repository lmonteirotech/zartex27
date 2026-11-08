//! `UPVH` ("Vehicle Hierarchy", signature `AWHF`): the part tree of a vehicle and the tables that place each part's model inside the vehicle.
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! Exposes `Hierarchy::parse`, `load_hierarchies`, `wheel_radius` and the types `Part` and `Hierarchy`, with the methods
//! `parents`, `zone_centre`, `extra_wheel_centres` and `wheel_centres`.
//! Uses `bytes` (little-endian reads), `chnk` (the container walk) and `mdpc` (the model bounding box for the wheel radius).
//! Used by the `zartex-app` crate: the vehicle preview (`vehicle3d.rs`) and the asset list (`assets.rs`).

use crate::chnk::Chunk;

/// Context code of an UPVH entry: the bytes `UPVH` read as a little-endian `u32`.
pub const CTX_UPVH: u32 = 0x4856_5055;

/// One node of the part tree, in pre-order. The index fields refer to the tables of the enclosing `Hierarchy`; -1 means
/// none (0xFF for `model_id`).
#[derive(Debug, Clone)]
pub struct Part {
    /// 1 generic, 2 wheel, 3 marker, 4 root, 8/9 damage variants.
    pub ptype: u16,
    /// Slot id: 0x1B..0x1E = wheels FL, FR, RL, RR; doors, bumpers, body...
    pub slot: u16,
    /// Subtree size in pre-order, including the part itself.
    pub n_sub: u16,
    /// Package-global model index; 0xFF = no model.
    pub model_id: u8,
    pub offset_id: i16,
    pub axis_id: i16,
    /// Index into `Hierarchy::damaging` (the part is a damage zone), -1 = none (`part+0x1A`, `transformId`).
    pub damaging_id: i16,
    /// Index into `Hierarchy::markers` (`part+0x16`, `positionId`), -1 = none: marker parts (type 3) use it for their
    /// position (cameras 0x10..0x17, seats, exhaust smoke, lights).
    pub position_id: i16,
    /// Index into the PDL collision models (`part+0x14`, `physicsId`), -1 = none.
    pub physics_id: i16,
    pub flags: u32,
}

/// A vehicle hierarchy: its parts, moving points, placement matrices, damage zones, markers and the half extents of its
/// collision models.
#[derive(Debug, Clone)]
pub struct Hierarchy {
    pub uid: u32,
    pub parts: Vec<Part>,
    /// 8 floats each: position (x,y,z,w) and a rotation-ish vector.
    pub moving: Vec<[f32; 8]>,
    /// Row-vector 4x4 placement matrices.
    pub instances: Vec<[f32; 16]>,
    /// Damage zone records (0x50 bytes): a 4x4 world-to-zone matrix (row-vector convention) and a vec4.
    pub damaging: Vec<[f32; 20]>,
    /// Marker positions (vec4, vehicle space): cameras, seats, smoke and light points.
    pub markers: Vec<[f32; 4]>,
    /// Half extents of the first collision primitive (an oriented box) of every PDL model; `[0; 3]` for a model without
    /// primitives. The damage code reads `|half|` of the part's model as the reach of its zone (`FUN_00535860`).
    pub pdl_half0: Vec<[f32; 3]>,
}

/// Little-endian `u32` at `o`, or `None` when it does not fit.
fn rd32(b: &[u8], o: usize) -> Option<u32> {
    crate::bytes::u32_le(b, o)
}
/// Little-endian `u16` at `o`, or `None` when it does not fit.
fn rd16(b: &[u8], o: usize) -> Option<u16> {
    crate::bytes::u16_le(b, o)
}
/// `f32` at `o` (the bits of the `u32` there), or `None`.
fn rdf(b: &[u8], o: usize) -> Option<f32> {
    rd32(b, o).map(f32::from_bits)
}

/// `UPVH` +0x0C: the hierarchy's type (6 = vehicle, 1 = prop), not a version. [comunidade: driver-tools/DSCript/Models/Hierarchies/HierarchyInfo.cs]
const HIERARCHY_TYPE_VEHICLE: u32 = 6;

impl Hierarchy {
    /// Parses one `UPVH` payload. Returns `None` unless the signature is `AWHF` and the type field at +0x0C is 6 (vehicle),
    /// or when a table runs past the end of the data. Counts: parts at +0x10, uid at +0x14, and markers, moving points,
    /// damage zones and instances as 16-bit counts at +0x20, +0x22, +0x24 and +0x26. The tables follow the 0x30-byte header
    /// in this order: parts (0x20 bytes each), moving points (0x20 each), damage zones (0x50 each), markers (0x10 each),
    /// instances (0x40 each), then an optional PDL block.
    pub fn parse(b: &[u8]) -> Option<Hierarchy> {
        if b.get(0..4)? != b"AWHF" || rd32(b, 0x0C)? != HIERARCHY_TYPE_VEHICLE {
            return None;
        }
        let n_parts = rd32(b, 0x10)? as usize;
        let uid = rd32(b, 0x14)?;
        let n_markers = rd16(b, 0x20)? as usize;
        let n_moving = rd16(b, 0x22)? as usize;
        let n_damaging = rd16(b, 0x24)? as usize;
        let n_inst = rd16(b, 0x26)? as usize;
        let mut parts = Vec::with_capacity(n_parts);
        for i in 0..n_parts {
            let o = 0x30 + 0x20 * i;
            parts.push(Part {
                ptype: rd16(b, o)?,
                slot: rd16(b, o + 2)?,
                n_sub: rd16(b, o + 12)?,
                model_id: *b.get(o + 18)?,
                offset_id: rd16(b, o + 24)? as i16,
                axis_id: rd16(b, o + 28)? as i16,
                damaging_id: rd16(b, o + 26)? as i16,
                position_id: rd16(b, o + 22)? as i16,
                physics_id: rd16(b, o + 20)? as i16,
                flags: rd32(b, o + 4)?,
            });
        }
        // the running offset of the tables that follow the part table
        let mut o = 0x30 + 0x20 * n_parts;
        let mut moving = Vec::with_capacity(n_moving);
        for i in 0..n_moving {
            let mut m = [0f32; 8];
            for (k, v) in m.iter_mut().enumerate() {
                *v = rdf(b, o + 0x20 * i + 4 * k)?;
            }
            moving.push(m);
        }
        o += 0x20 * n_moving;
        let mut damaging = Vec::with_capacity(n_damaging);
        for i in 0..n_damaging {
            let mut m = [0f32; 20];
            for (k, v) in m.iter_mut().enumerate() {
                *v = rdf(b, o + 0x50 * i + 4 * k)?;
            }
            damaging.push(m);
        }
        o += 0x50 * n_damaging;
        let mut markers = Vec::with_capacity(n_markers);
        for i in 0..n_markers {
            markers.push([rdf(b, o + 0x10 * i)?, rdf(b, o + 0x10 * i + 4)?, rdf(b, o + 0x10 * i + 8)?, rdf(b, o + 0x10 * i + 12)?]);
        }
        o += 0x10 * n_markers;
        let mut instances = Vec::with_capacity(n_inst);
        for i in 0..n_inst {
            let mut m = [0f32; 16];
            for (k, v) in m.iter_mut().enumerate() {
                *v = rdf(b, o + 0x40 * i + 4 * k)?;
            }
            instances.push(m);
        }
        o += 0x40 * n_inst;
        // PDL block (`PDL001.002.003a`): n models of 16 bytes { n_prim, offset from the entry, radius, 0 }, then 96-byte boxes
        let mut pdl_half0 = Vec::new();
        if b.get(o + 0x10..o + 0x1B).is_some_and(|t| t == b"PDL001.002.") {
            let n = rd32(b, o)? as usize;
            for k in 0..n {
                let e = o + 0x20 + 16 * k;
                let (n_prim, off) = (rd32(b, e)? as usize, rd32(b, e + 4)? as usize);
                let half = if n_prim > 0 { [rdf(b, e + off)?, rdf(b, e + off + 4)?, rdf(b, e + off + 8)?] } else { [0.0; 3] };
                pdl_half0.push(half);
            }
        }
        Some(Hierarchy { uid, parts, moving, instances, damaging, markers, pdl_half0 })
    }

    /// Parent of every part (pre-order tree: a part's subtree is the next
    /// `n_sub - 1` parts).
    pub fn parents(&self) -> Vec<Option<usize>> {
        let mut out = vec![None; self.parts.len()];
        let mut stack: Vec<(usize, usize)> = Vec::new(); // (index, end of subtree)
        for (i, p) in self.parts.iter().enumerate() {
            while let Some(&(_, end)) = stack.last() {
                if i >= end {
                    stack.pop();
                } else {
                    break;
                }
            }
            out[i] = stack.last().map(|&(j, _)| j);
            stack.push((i, i + (p.n_sub as usize).max(1)));
        }
        out
    }

    /// Centre of a damage zone in model space. The record's matrix is world-to-zone (`local = p * R + t`), so the
    /// centre is `-t * R^T`.
    pub fn zone_centre(&self, id: usize) -> Option<[f32; 3]> {
        let m = self.damaging.get(id)?;
        let t = [m[12], m[13], m[14]];
        let row = |r: usize| [m[4 * r], m[4 * r + 1], m[4 * r + 2]];
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        Some([-dot(t, row(0)), -dot(t, row(1)), -dot(t, row(2))])
    }

    /// Centres of the extra rear wheels of 6-wheelers (slots 0x2A, 0x2B), in vehicle space; empty on cars.
    pub fn extra_wheel_centres(&self) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        for slot in [0x2A, 0x2B] {
            for p in &self.parts {
                if p.ptype == 2 && p.slot == slot && p.offset_id >= 0 {
                    if let Some(m) = self.moving.get(p.offset_id as usize) {
                        out.push([m[0], m[1], m[2]]);
                    }
                }
            }
        }
        out
    }

    /// Wheel centre positions in vehicle space, ordered FL, FR, RL, RR
    /// (slots 0x1B..0x1E). `None` for a missing wheel.
    pub fn wheel_centres(&self) -> [Option<[f32; 3]>; 4] {
        let mut out = [None; 4];
        for p in &self.parts {
            if p.ptype == 2 && (0x1B..=0x1E).contains(&p.slot) && p.offset_id >= 0 {
                if let Some(m) = self.moving.get(p.offset_id as usize) {
                    out[(p.slot - 0x1B) as usize] = Some([m[0], m[1], m[2]]);
                }
            }
        }
        out
    }
}

/// Hierarchies found anywhere inside a container, in file order. Entries that do not parse are skipped.
pub fn load_hierarchies(root: &Chunk) -> Vec<Hierarchy> {
    let mut out = Vec::new();
    fn walk(c: &Chunk, out: &mut Vec<Hierarchy>) {
        for e in &c.entries {
            if e.ctx == CTX_UPVH {
                if let Some(h) = Hierarchy::parse(e.data) {
                    out.push(h);
                }
            } else if e.is_chnk() {
                if let Ok(ch) = e.children() {
                    walk(&ch, out);
                }
            }
        }
    }
    walk(root, &mut out);
    out
}

/// Wheel radius from the bounding box of the front-left wheel model: half of the Y extent of the first wheel part (type 2)
/// in slot 0x1B that has a model. `None` when the value is not finite or not between 0.05 and 2.0.
pub fn wheel_radius(h: &Hierarchy, pkg: &crate::mdpc::Package) -> Option<f32> {
    let part = h.parts.iter().find(|p| p.ptype == 2 && p.slot == 0x1B && p.model_id != 0xFF)?;
    let m = pkg.models.get(part.model_id as usize)?;
    let ext = |axis: usize| {
        let lo = m.bbox.iter().map(|c| c[axis]).fold(f32::MAX, f32::min);
        let hi = m.bbox.iter().map(|c| c[axis]).fold(f32::MIN, f32::max);
        hi - lo
    };
    // the wheel axis is along x, so the vertical extent is the tyre diameter
    let r = ext(1) * 0.5;
    (r.is_finite() && r > 0.05 && r < 2.0).then_some(r)
}
