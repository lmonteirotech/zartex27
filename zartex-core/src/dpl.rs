//! Driver: Parallel Lines mission package (.sp): port of LuaMissionPackage.CreateMission / AddMission /
//! OnFileSaveBegin and SpoolSystemLookup. The package is built from scratch (no template).
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the spool/package model of Zartex 2's Parallel Lines package support).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - reads the high 16 bits of the SSLP count field (kept in count_hi) and writes them back, so that the game's own packages open
//! - skips the trailing BINF buffer of the game's own packages: a top-level child after the init container that is not a package is ignored
//!
//! This file exposes the Parallel Lines package model: DplPackage (the whole .sp file), DplMission (one mission,
//! stored as an EM__ package), SpoolSystemLookup (the SSLP buffer) and the alignment codes ALIGN_MISSIONS and ALIGN_ROOT.
//! It uses the binary reader and writer (binio), the chunk tree (chunk), the mission and logic model (mission) and the
//! exported mission objects (objects). It is used by api_dpl.rs (compile_dpl_file), by zartex-app (model.rs, main.rs)
//! and by the tests of the crate.

use crate::binio::*;
use crate::chunk::*;
use crate::mission::*;
use crate::objects::ExportedMissionObjects;

/// Alignment code of the mission chunks (0xB, written as Align2048).
pub const ALIGN_MISSIONS: u8 = 0xB; // Align2048
/// Alignment code of the mission root packages (0x8, written as Align256).
pub const ALIGN_ROOT: u8 = 0x8; // Align256

// ---------------------------------------------------------------- lookup (SSLP)

/// The SSLP buffer: a lookup table from sub mission ids to mission positions in the package.
#[derive(Debug, Clone, PartialEq)]
pub struct SpoolSystemLookup {
    /// First i16 of the buffer.
    pub short_flags: i16,
    /// High 16 bits of the stored count field (0x4E20 in the game's own packages, 0 in LuaC output)
    pub count_hi: u16,
    /// i32 after the count field. Not interpreted; kept so the buffer is written back unchanged.
    pub flags: i32,
    /// i32 after `flags`. Not interpreted; kept so the buffer is written back unchanged.
    pub spool_flags: i32,
    /// i16 after `spool_flags`. Not interpreted; kept so the buffer is written back unchanged.
    pub unk1: i16,
    /// i64 after `unk1`. Not interpreted; kept so the buffer is written back unchanged.
    pub unk2: i64,
    /// i32 after `unk2`. Not interpreted; kept so the buffer is written back unchanged.
    pub unk3: i32,
    /// (sub mission id, chunk id)
    pub lookups: Vec<(i16, i32)>,
}

impl Default for SpoolSystemLookup {
    fn default() -> Self {
        SpoolSystemLookup {
            short_flags: 1,
            count_hi: 0,
            flags: -2147483643, // 05 00 00 80
            spool_flags: 1,
            unk1: 0,
            unk2: 0,
            unk3: 2,
            lookups: Vec::new(),
        }
    }
}

impl SpoolSystemLookup {
    /// Parses an SSLP buffer. The low 16 bits of the count field give the number of (sub id, chunk id) pairs.
    /// Returns an error when the buffer ends before the fields or the stated number of pairs.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let short_flags = r.i16()?;
        let count_raw = r.i32()?;
        let count = count_raw & 0xFFFF;
        let count_hi = ((count_raw as u32) >> 16) as u16;
        let flags = r.i32()?;
        let spool_flags = r.i32()?;
        let unk1 = r.i16()?;
        let unk2 = r.i64()?;
        let unk3 = r.i32()?;
        let mut lookups = Vec::new();
        for _ in 0..count.max(0) {
            lookups.push((r.i16()?, r.i32()?));
        }
        Ok(SpoolSystemLookup { short_flags, count_hi, flags, spool_flags, unk1, unk2, unk3, lookups })
    }
    /// Serialises the buffer. The count field is the number of lookups in the low 16 bits and `count_hi` in the high 16 bits.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.i16(self.short_flags);
        w.i32((self.lookups.len() as u32 | ((self.count_hi as u32) << 16)) as i32);
        w.i32(self.flags);
        w.i32(self.spool_flags);
        w.i16(self.unk1);
        w.i64(self.unk2);
        w.i32(self.unk3);
        for &(a, b) in &self.lookups {
            w.i16(a);
            w.i32(b);
        }
        w.b
    }
}

// ---------------------------------------------------------------- mission

/// One mission of a package: its id, position, flags, logic, objects and the raw extra buffers.
#[derive(Debug, Clone, PartialEq)]
pub struct DplMission {
    /// Description of the EPMR root (None for the init mission, which has no root)
    pub name: Option<String>,
    /// Mission id (5 is the init mission)
    pub id: i16,
    /// Position written to the mission summary (EMMS)
    pub x: f32,
    /// Position written to the mission summary (EMMS)
    pub y: f32,
    /// Flags written to the mission summary (EMMS)
    pub flags: i32,
    /// Whether the mission has an EPMR root package
    pub define_root: bool,
    /// Objects of the mission (EMOB)
    pub objects: ExportedMissionObjects,
    /// Logic data of the mission (LELD)
    pub logic: LogicData,
    /// Raw 'extras' that CreateMission fills with fixed content
    pub empr: Vec<u8>,
    /// Raw LEPS buffer (4 bytes when created). Not interpreted.
    pub leps: Vec<u8>,
    /// Raw LENE buffer (4 bytes when created). Not interpreted.
    pub lene: Vec<u8>,
    /// Raw FING buffer (0x32 bytes when created). Not interpreted.
    pub fing: Vec<u8>,
    /// Raw LEAT buffer (6 bytes, or 4 for the init mission with id 5, when created). Not interpreted.
    pub leat: Vec<u8>,
}

impl DplMission {
    /// LuaMissionPackage.CreateMission(packageName, id, x, y, flags, defineRoot).
    /// Fills the raw buffers with fixed content: EMPR 24 zero bytes, LEPS and LENE 4, FING 0x32,
    /// LEAT 4 when `id` is 5 and 6 otherwise.
    pub fn new(name: Option<&str>, id: i16, x: f32, y: f32, flags: i32, define_root: bool) -> Self {
        DplMission {
            name: name.map(|s| s.to_string()),
            id,
            x,
            y,
            flags,
            define_root,
            objects: ExportedMissionObjects::default(),
            logic: LogicData::default(),
            empr: vec![0; 24],
            leps: vec![0; 4],
            lene: vec![0; 4],
            fing: vec![0; 0x32],
            leat: vec![0; if id != 5 { 6 } else { 4 }],
        }
    }

    /// The 0x1C 'Mission Summary' buffer (EMMS). Layout: byte 0x01, 7 zero bytes, byte 0x01, 3 zero bytes,
    /// i16 id at offset 12, u16 0xFFFF at 14, f32 x at 16, f32 y at 20, i32 flags at 24.
    pub fn summary_bytes(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(0x01);
        w.bytes(&[0; 7]);
        w.u8(0x01);
        w.bytes(&[0; 3]);
        w.i16(self.id);
        w.u16(0xFFFF);
        w.f32(self.x);
        w.f32(self.y);
        w.i32(self.flags);
        w.b
    }

    /// Makes a buffer chunk with the given tag and description, aligned with ALIGN_MISSIONS.
    fn buf(tag: &str, desc: &str, data: Vec<u8>) -> Node {
        let mut b = Buffer::new(ctx(tag), desc, data);
        b.c.alignment = ALIGN_MISSIONS;
        Node::Buffer(b)
    }

    /// Makes a package chunk with the given tag, description and children, aligned with ALIGN_MISSIONS.
    fn pkg(tag: &str, desc: &str, children: Vec<Node>) -> Node {
        let mut p = Package::new(ctx(tag));
        p.c.description = desc.to_string();
        p.c.alignment = ALIGN_MISSIONS;
        for c in children {
            p.push(c);
        }
        Node::Package(p)
    }

    /// Builds the EM__ package (children order as CreateMission) with all data serialised.
    pub fn build_exported_mission(&mut self) -> Res<Node> {
        // the actor and node tables are saved first, as two buffers each (definitions and properties)
        let (ad, ap) = self.logic.actors.save()?;
        let (nd, np) = self.logic.nodes.save()?;
        // LELD holds the logic data; EM__ holds the summary, the objects, the EMPR buffer and LELD
        let leld = Self::pkg(
            "LELD",
            "Logic Export Data",
            vec![
                Self::buf("LESC", "String Collection", self.logic.strings.save()),
                Self::buf("LESB", "Sound Bank Table", self.logic.sound_banks.save()),
                Self::buf("LEPS", "Unknown (LEPS)", self.leps.clone()),
                Self::pkg(
                    "LEAC",
                    "Logic Export Actors Chunk",
                    vec![
                        Self::buf("LEAD", "Logic Actors Definitions Table", ad),
                        Self::buf("LEPR", "Logic Actors Properties Table", ap),
                    ],
                ),
                Self::pkg(
                    "LENC",
                    "Logic Export Nodes Chunk",
                    vec![
                        Self::buf("LEND", "Logic Nodes Definitions Table", nd),
                        Self::buf("LEPR", "Logic Nodes Properties Table", np),
                    ],
                ),
                Self::buf("LEAS", "Actors Set Table", self.logic.actor_sets.save()),
                Self::buf("LEWC", "Exported Wire Collections", self.logic.wires.save()),
                Self::buf("LECO", "Logic Script Counters Export", self.logic.counters.save()),
                Self::buf("LENE", "Unknown (LENE)", self.lene.clone()),
                Self::buf("FING", "Unknown (FING)", self.fing.clone()),
                Self::buf("LEAT", "Unknown (LEAT)", self.leat.clone()),
            ],
        );
        Ok(Self::pkg(
            "EM__",
            "Exported Mission",
            vec![
                Self::buf("EMMS", "Mission Summary", self.summary_bytes()),
                Self::buf("EMOB", "Exported Mission Objects", self.objects.save()?),
                Self::buf("EMPR", "Exported Mission Prop Handles", self.empr.clone()),
                leld,
            ],
        ))
    }

    /// Loads a mission from its EM__ package (extras kept raw). The id, x, y and flags are read from the
    /// EMMS buffer inside `em` at offsets 12, 16, 20 and 24 (see summary_bytes). Returns an error when a
    /// required child (LELD, EMMS, EMOB, EMPR or a LELD child) is missing.
    pub fn load_exported_mission(em: &Package, name: Option<String>, define_root: bool) -> Res<Self> {
        let leld = match first_child(em, ctx("LELD")).and_then(|n| n.as_package()) {
            Some(p) => p,
            None => return invalid("no LELD"),
        };
        let raw = |p: &Package, t: &str| -> Res<Vec<u8>> {
            match first_child(p, ctx(t)).and_then(|n| n.as_buffer()) {
                Some(b) => Ok(b.data().to_vec()),
                None => invalid(format!("missing {t}")),
            }
        };
        let summary = raw(em, "EMMS")?;
        let mut r = Reader::new(&summary);
        r.pos = 12;
        let id = r.i16()?;
        r.pos = 16;
        let x = r.f32()?;
        let y = r.f32()?;
        let flags = r.i32()?;
        Ok(DplMission {
            name,
            id,
            x,
            y,
            flags,
            define_root,
            objects: ExportedMissionObjects::load(&raw(em, "EMOB")?)?,
            logic: LogicData::load(leld)?,
            empr: raw(em, "EMPR")?,
            leps: raw(leld, "LEPS")?,
            lene: raw(leld, "LENE")?,
            fing: raw(leld, "FING")?,
            leat: raw(leld, "LEAT")?,
        })
    }
}

// ---------------------------------------------------------------- package

/// A whole Parallel Lines mission package (.sp file).
#[derive(Debug, Clone, PartialEq)]
pub struct DplPackage {
    /// The init (auto-exec) mission, created with defineRoot = false
    pub init: DplMission,
    /// Missions in AddMission order, with their sub mission id
    pub missions: Vec<(i16, DplMission)>,
    /// The SSLP lookup table of the package
    pub lookup: SpoolSystemLookup,
}

impl Default for DplPackage {
    fn default() -> Self {
        DplPackage {
            init: DplMission::new(None, 5, 0.0, 0.0, 0, false),
            missions: Vec::new(),
            lookup: SpoolSystemLookup::default(),
        }
    }
}

impl DplPackage {
    /// PACKAGE.AddMission(mission, subId): lookup entry is (subId, number of missions so far + 1)
    pub fn add_mission(&mut self, mission: DplMission, sub_id: i16) {
        self.lookup.lookups.push((sub_id, self.missions.len() as i32 + 1));
        self.missions.push((sub_id, mission));
    }

    /// Builds the chunk tree like the LuaMissionPackage constructor + AddMission. Does not serialise it.
    /// The first top-level chunk is the init container (SSIC); each mission follows as an EPMR root.
    pub fn build_chunks(&mut self) -> Res<ChunkFile> {
        let mut cf = ChunkFile::default();

        // ---- init container (SSIC): SSLP, the placeholder MRRD, EMMS and the init EM__ package
        let mut ssic = Package::new(ctx("SSIC"));
        ssic.c.description = "Auto-exec Mission Container".into();
        ssic.c.alignment = ALIGN_MISSIONS;

        let mut lookup = Buffer::new(ctx("SSLP"), "Missions System Lookup", self.lookup.save());
        lookup.c.alignment = ALIGN_MISSIONS;
        ssic.push(Node::Buffer(lookup));

        let mut mrrd = vec![0u8; 368];
        mrrd[0] = 91;
        mrrd[366] = 1;
        mrrd[367] = 1;
        let mut mrrd = Buffer::new(0x4452524D, "Placeholder MRRD", mrrd);
        mrrd.c.alignment = ALIGN_MISSIONS;
        ssic.push(Node::Buffer(mrrd));

        let mut emms = Buffer::new(ctx("EMMS"), "Mission Summary", self.init.summary_bytes());
        emms.c.alignment = ALIGN_MISSIONS;
        ssic.push(Node::Buffer(emms));
        ssic.push(self.init.build_exported_mission()?);
        cf.content.push(Node::Package(ssic));

        // ---- one EPMR root per mission: its description is the mission name (at most 255 characters)
        for (_, m) in &mut self.missions {
            let mut root = Package::new(ctx("EPMR"));
            root.c.alignment = ALIGN_ROOT;
            if let Some(n) = &m.name {
                root.c.description = n.chars().take(255).collect();
            }
            let mut summary = Buffer::new(ctx("EMMS"), "Mission Summary", m.summary_bytes());
            summary.c.alignment = ALIGN_MISSIONS;
            root.push(Node::Buffer(summary));
            root.push(m.build_exported_mission()?);
            cf.content.push(Node::Package(root));
        }
        Ok(cf)
    }

    /// Builds the chunk tree (build_chunks) and serialises it to the bytes of a .sp file.
    pub fn to_bytes(&mut self) -> Res<Vec<u8>> {
        let mut cf = self.build_chunks()?;
        Ok(cf.to_bytes()?)
    }

    /// Reads a package written by `to_bytes` (used for the round-trip tests), or a game package.
    /// The first top-level chunk must be the SSIC container. The missions are the packages after it,
    /// in the order of the SSLP lookups.
    pub fn from_bytes(d: &[u8]) -> Res<Self> {
        let cf = ChunkFile::from_bytes(d)?;
        let top = &cf.content.children;
        let ssic = match top.first().and_then(|n| n.as_package()) {
            Some(p) if p.c.context == ctx("SSIC") => p,
            _ => return invalid("no SSIC chunk"),
        };
        let lookup = match first_child(ssic, ctx("SSLP")).and_then(|n| n.as_buffer()) {
            Some(b) => SpoolSystemLookup::load(b.data())?,
            None => return invalid("no SSLP chunk"),
        };
        let init_em = match first_child(ssic, ctx("EM__")).and_then(|n| n.as_package()) {
            Some(p) => p,
            None => return invalid("no init EM__"),
        };
        let init = DplMission::load_exported_mission(init_em, None, false)?;
        let mut missions = Vec::new();
        for (i, n) in top.iter().enumerate().skip(1) {
            let root = match n.as_package() {
                Some(p) => p,
                // the game's own packages end with a BINF buffer that LuaC output does not have
                None => continue,
            };
            let em = match first_child(root, ctx("EM__")).and_then(|n| n.as_package()) {
                Some(p) => p,
                None => return invalid("mission root without EM__"),
            };
            let name = if root.c.description.is_empty() { None } else { Some(root.c.description.clone()) };
            // the lookup entry for mission i - 1 gives its sub mission id; 0 when there is no entry
            let sub_id = lookup.lookups.get(i - 1).map(|l| l.0).unwrap_or(0);
            missions.push((sub_id, DplMission::load_exported_mission(em, name, true)?));
        }
        Ok(DplPackage { init, missions, lookup })
    }
}
