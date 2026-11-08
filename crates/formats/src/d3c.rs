//! `.d3c` city container reader: building and prop instances (`GEBI`), water tiles (`GWTR`) and parking slots (`ATRD`).
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! Reads the root container of a city: the global section (`GESR`), the region sections (`GEIR`, named by their description
//! `Region N`) and the interior sections (`IRCT`).
//! Exposes `City::parse`, `City`, `Instance`, `Water`, `ParkingSlot` and `Scope`.
//! Uses `chnk` to walk the container. Used by `zartex-app`'s city view (`city3d.rs`), which imports `City` and `Scope`.

use crate::chnk::Chunk;

/// Where an instance was found: `Global` (the `GESR` section), `Region(n)` (a `GEIR` section described as `Region n`),
/// or `Interior` (any other `GEIR` section, and the `IRCT` sections).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scope {
    Global,
    Region(u32),
    Interior,
}

/// One placed object (building or prop) of the city, read from a `GEBI` record of 32 bytes.
#[derive(Debug, Clone, Copy)]
pub struct Instance {
    pub scope: Scope,
    pub pos: [f32; 3],
    /// `hi16` = group link (0xFFFF none); `lo16` = `package<<10 | model`.
    pub id: u32,
    pub d: [f32; 3],
    pub s: f32,
    /// Index of the `IRCT` chunk (interior) this instance belongs to, in file order; `u32::MAX` if none.
    /// Interior packages share uids (0x3B external, 0x1E internal), so the interior index picks the package.
    pub interior: u32,
}

impl Instance {
    /// Package uid referenced by the instance (0 = global package): bits 10 to 15 of `id`.
    pub fn package(&self) -> u32 {
        (self.id >> 10) & 0x3F
    }
    /// Model index inside that package: the low 10 bits of `id`.
    pub fn model(&self) -> u32 {
        self.id & 0x3FF
    }
    /// 0x3FF in the model field marks an empty placeholder.
    pub fn is_placeholder(&self) -> bool {
        self.model() == 0x3FF
    }
    /// `s == 101` marks the yaw form `d = (cos t, sin t, 0)`; otherwise
    /// `(d, s)` is a quaternion `(x, y, z, w)`.
    pub fn is_yaw(&self) -> bool {
        self.s == 101.0
    }
}

/// One water surface tile (model of the global package).
#[derive(Debug, Clone, Copy)]
pub struct Water {
    pub pos: [f32; 3],
    pub id: u32,
}

/// A parking space of the city: where the game parks a car, and of which kind. `ATRD` k-d tree 3 holds the categories 2..8
/// (3 = any car, 23 = vans/trucks, 24 = boats, 25 = bikes, 26, 27 = taxis, 31; the exe's table at 0x6ED46C); tree 1 holds
/// category 1 (kind 1), the cars the zone presets' `pParkedCarDensity` asks for.
#[derive(Debug, Clone, Copy)]
pub struct ParkingSlot {
    pub pos: [f32; 3],
    pub kind: u8,
}

/// The contents of one city: instances, water tiles and parking slots.
#[derive(Debug, Default)]
pub struct City {
    pub instances: Vec<Instance>,
    pub water: Vec<Water>,
    /// categories 2..8, `ATRD` tree 3
    pub parking: Vec<ParkingSlot>,
    /// category 1, `ATRD` tree 1: where the original's parked cars of the street are born (1550 in Miami, all of kind 1)
    /// [evidência 3, 4, 5]
    pub parking1: Vec<ParkingSlot>,
}

/// `ATRD` ("Attractor data", v7): `{x0, z0, 480, 480}`, 21 absolute offsets of 20 k-d trees, records of 16 bytes
/// `{i32 w0, i32 w1, u8, u8, u8 kind, i8 axis, u32}` with x = w0 * 2^-19, z = w1 * 2^-19 and y = (w0 & 0x1FFF) / 64 - 10
/// (decoder 0x43C4F0). Tree 3 holds the parking spaces of the categories 2..8 (`0x441730`); tree 1 the category 1: the cars
/// that are born parked in the recordings of the original fall on its points (131 of 136, 81 of 92, 88 of 103 within
/// 0.5 m and the same height) and on no other tree's [evidência 3, 4, 5]. `tree` is the index of the tree.
fn parse_atrd_parking(data: &[u8], tree: usize, out: &mut Vec<ParkingSlot>) {
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let (Some(a), Some(b)) = (u32_at(0x10 + 4 * tree), u32_at(0x10 + 4 * (tree + 1))) else { return };
    let (a, b) = (a as usize, b as usize);
    // both offsets must lie inside the block, and the bytes between them must be whole 16-byte records
    if a < 0x64 || b < a || b > data.len() || (b - a) % 16 != 0 {
        return;
    }
    for r in data[a..b].chunks_exact(16) {
        let w0 = i32::from_le_bytes(r[0..4].try_into().unwrap());
        let w1 = i32::from_le_bytes(r[4..8].try_into().unwrap());
        let x = w0 as f64 * (1.0 / 524288.0);
        let z = w1 as f64 * (1.0 / 524288.0);
        let y = (w0 & 0x1FFF) as f64 / 64.0 - 10.0;
        out.push(ParkingSlot { pos: [x as f32, y as f32, z as f32], kind: r[10] });
    }
}

/// Reads the water tiles of a `GWTR` payload into `out`: a `u32` count at 0, then 32-byte records from offset 16. The
/// position (three floats) is at the start of each record and the `u32` id at +16. Ignored when the payload is shorter
/// than the count needs.
fn parse_gwtr(data: &[u8], out: &mut Vec<Water>) {
    if data.len() < 16 {
        return;
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if data.len() < 16 + 32 * n {
        return;
    }
    let f = |o: usize| f32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    for i in 0..n {
        let o = 16 + 32 * i;
        out.push(Water { pos: [f(o), f(o + 4), f(o + 8)], id: u32::from_le_bytes(data[o + 16..o + 20].try_into().unwrap()) });
    }
}

/// Reads the instances of a `GEBI` payload into `out`, tagged with `scope` and `interior`. Layout: a `u32` count at 0, a
/// 0x40-byte header, then 32-byte records. In each record: the position (three floats at +0), `id` (`u32` at +12), `d`
/// (three floats at +16) and `s` (float at +28); see `Instance::is_yaw`. The payload is ignored unless its length is
/// exactly 0x40 + 32 * count.
fn parse_gebi(data: &[u8], scope: Scope, interior: u32, out: &mut Vec<Instance>) {
    if data.len() < 0x40 {
        return;
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if data.len() != 0x40 + 32 * n {
        return;
    }
    let f = |o: usize| f32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    for i in 0..n {
        let o = 0x40 + 32 * i;
        out.push(Instance {
            scope,
            pos: [f(o), f(o + 4), f(o + 8)],
            id: u32::from_le_bytes(data[o + 12..o + 16].try_into().unwrap()),
            d: [f(o + 16), f(o + 20), f(o + 24)],
            s: f(o + 28),
            interior,
        });
    }
}

/// The region number from a description of the form `Region N`; `None` for any other text.
fn region_index(desc: &str) -> Option<u32> {
    desc.strip_prefix("Region ")?.trim().parse().ok()
}

impl City {
    /// Builds a city from the root container of a `.d3c`. Entries are read in file order. Only the first `GESR` is read:
    /// it gives the global instances and water. Each `GEIR` gives its instances (region or interior) and its parking slots
    /// (`ATRD` trees 3 and 1). Each `IRCT` gives instances tagged with its index. Other entries, and entries that do not
    /// parse, are skipped.
    pub fn parse(root: &Chunk) -> City {
        let mut instances = Vec::new();
        let mut water = Vec::new();
        let mut parking = Vec::new();
        let mut parking1 = Vec::new();
        let mut seen_global = false;
        let mut interior = 0u32;
        for e in &root.entries {
            let tag = e.tag();
            if !e.is_chnk() {
                continue;
            }
            let Ok(kids) = e.children() else { continue };
            match tag.as_str() {
                "GESR" if !seen_global => {
                    seen_global = true;
                    for g in kids.find("GEBI") {
                        parse_gebi(g.data, Scope::Global, u32::MAX, &mut instances);
                    }
                    for g in kids.find("GWTR") {
                        parse_gwtr(g.data, &mut water);
                    }
                }
                "GEIR" => {
                    let scope = region_index(&e.desc).map(Scope::Region).unwrap_or(Scope::Interior);
                    for g in kids.find("GEBI") {
                        parse_gebi(g.data, scope, u32::MAX, &mut instances);
                    }
                    for g in kids.find("ATRD") {
                        parse_atrd_parking(g.data, 3, &mut parking);
                        parse_atrd_parking(g.data, 1, &mut parking1);
                    }
                }
                "IRCT" => {
                    for g in kids.find("GEBI") {
                        parse_gebi(g.data, Scope::Interior, interior, &mut instances);
                    }
                    interior += 1;
                }
                _ => {}
            }
        }
        City { instances, water, parking, parking1 }
    }

    /// Number of instances whose scope satisfies `pred`.
    pub fn count(&self, pred: impl Fn(&Scope) -> bool) -> usize {
        self.instances.iter().filter(|i| pred(&i.scope)).count()
    }
}
