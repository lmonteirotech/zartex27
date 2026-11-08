//! Mission-level resources: MissionSummaryData, MissionInstanceData, ExportedMission, LogicExportData
//! and the game's `MissionScriptFile` (load / save through the chunk container).
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the file container reader/writer and the mission model).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! This file reads and writes a mission. `MissionFile::from_bytes` parses the chunk container, finds the
//! exported mission package (EM__) and the summary buffer (EMMS), and keeps their positions in the tree.
//! `MissionFile::to_bytes` writes the mission and the summary back into the tree and serialises it again.
//! Uses `crate::binio` (the reader, the writer and the vector types), `crate::chunk` (the container),
//! `crate::logic` (the logic tables) and `crate::objects` (the mission objects).
//! Used by: `crate::script`, `crate::dpl`, `crate::compile`, `crate::luaraw`, `crate::readable`, `crate::api_part_b`,
//! and by `zartex-app` (`model`, `creator_tools` and `citysnap`), and by the integration tests of `zartex-core`.

use crate::binio::*;
use crate::chunk::*;
use crate::logic::*;
use crate::objects::*;

/// SpoolerContext from a 4-char tag: (c4<<24)|(c3<<16)|(c2<<8)|c1. Panics when `tag` is not 4 bytes long.
pub fn ctx(tag: &str) -> i32 {
    let b = tag.as_bytes();
    assert_eq!(b.len(), 4);
    ((b[3] as i32) << 24) | ((b[2] as i32) << 16) | ((b[1] as i32) << 8) | (b[0] as i32)
}

/// Marker value that must come first in the density block of the summary buffer.
pub const DENSITY_DATA_MAGIC: i32 = 0x5526_4524;

// ---------------------------------------------------------------- summary

/// The mission summary (EMMS buffer): start position, city and mission ids, the optional density block,
/// and the ping radii. With `dpl` set, the buffer is kept as raw bytes in `dpl_buffer`.
#[derive(Debug, Clone, PartialEq)]
pub struct MissionSummary {
    pub dpl: bool,
    pub has_density_data: bool,
    pub start_position: Vec2,
    pub mission_locale_id: i16,
    pub city_type: i32,
    /// "Mood" id (stored as MissionId)
    pub mission_id: i16,
    pub density_override: bool,
    pub parked_car_density: i32,
    pub attractor_parked_car_density: i32,
    pub ping_in_radius: f64,
    pub ping_out_radius: f64,
    pub dpl_buffer: Vec<u8>,
}

/// Default values: not a DPL summary, no density block, and a 0x1C-byte zero buffer for the DPL form.
impl Default for MissionSummary {
    fn default() -> Self {
        MissionSummary {
            dpl: false,
            has_density_data: false,
            start_position: Vec2::default(),
            mission_locale_id: 0,
            city_type: 0,
            mission_id: 0,
            density_override: false,
            parked_car_density: 0,
            attractor_parked_car_density: 0,
            ping_in_radius: 0.0,
            ping_out_radius: 0.0,
            dpl_buffer: vec![0; 0x1C],
        }
    }
}

/// Math.Round(x, 4) with .NET's default MidpointRounding.ToEven
fn round4(x: f64) -> f64 {
    let s = x * 10000.0;
    let r = s.round();
    let r = if (s - s.trunc()).abs() == 0.5 { 2.0 * (s / 2.0).round() } else { r };
    r / 10000.0
}

impl MissionSummary {
    /// Parses the EMMS buffer `d`. With `dpl` true the bytes are kept as `dpl_buffer` and nothing else is read.
    /// Otherwise the start position, the city, the mission and the locale ids are read. The density block
    /// is read when it is present and its marker matches. Without it the defaults 7, 1, 130 and 135 are used.
    pub fn load(d: &[u8], dpl: bool) -> Res<Self> {
        let mut m = MissionSummary { dpl, ..Default::default() };
        if dpl {
            m.dpl_buffer = d.to_vec();
            return Ok(m);
        }
        let mut r = Reader::new(d);
        m.start_position = r.vec2()?;
        m.city_type = r.i32()?;
        m.mission_id = r.i16()?;
        m.mission_locale_id = r.i16()?;
        // A buffer longer than 0x10 bytes may carry the density block. The marker decides whether it does.
        m.has_density_data = d.len() > 0x10;
        if m.has_density_data && r.i32()? == DENSITY_DATA_MAGIC {
            m.density_override = (r.i32()? & 0xFF) == 1;
            m.parked_car_density = r.i32()?;
            m.attractor_parked_car_density = r.i32()?;
            m.ping_in_radius = round4(r.f32()? as f64);
            m.ping_out_radius = round4(r.f32()? as f64);
        } else {
            m.parked_car_density = 7;
            m.attractor_parked_car_density = 1;
            m.ping_in_radius = 130.0;
            m.ping_out_radius = 135.0;
        }
        Ok(m)
    }

    /// Serialises the summary. The DPL form is always 0x1C bytes: `dpl_buffer` cut or zero-padded to that length.
    pub fn save(&self) -> Vec<u8> {
        if self.dpl {
            let mut b = vec![0u8; 0x1C];
            let n = self.dpl_buffer.len().min(0x1C);
            b[..n].copy_from_slice(&self.dpl_buffer[..n]);
            return b;
        }
        let mut w = Writer::new();
        w.f32(self.start_position.x);
        w.f32(self.start_position.y);
        w.i32(self.city_type);
        w.i16(self.mission_id);
        w.i16(self.mission_locale_id);
        // The density block is written only when `has_density_data` is set.
        if self.has_density_data {
            w.i32(DENSITY_DATA_MAGIC);
            w.u32((0xCCCCCCu32 << 8) | (self.density_override as u32));
            w.i32(self.parked_car_density);
            w.i32(self.attractor_parked_car_density);
            w.f32(self.ping_in_radius as f32);
            w.f32(self.ping_out_radius as f32);
        }
        w.b
    }

    /// MissionSummary.GetCityTypeByName ("Miami_Day" ...); resolved later against the C# table.
    /// Returns 1 to 6 for the six known names, and 0 for any other name.
    pub fn city_by_name(name: &str) -> i32 {
        match name {
            "Miami_Day" => 1,
            "Miami_Night" => 2,
            "Nice_Day" => 3,
            "Nice_Night" => 4,
            "Istanbul_Day" => 5,
            "Istanbul_Night" => 6,
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------- instances (GEBI)

/// One placed instance (a record of the GEBI buffer): position, instance id, the id it is attached to,
/// and a bounding box.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MissionInstance {
    pub position: Vec3,
    pub instance_id: i16,
    pub attached_to: i16,
    pub bounding_box: Vec4,
}

/// The GEBI buffer: five unknown i32 values, a load position, a start position, and the instance records.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MissionInstanceData {
    pub unk: [i32; 5],
    pub load_position: Vec3,
    pub start_position: Vec4,
    pub instances: Vec<MissionInstance>,
}

impl MissionInstanceData {
    /// Parses the GEBI buffer `d`. The first i32 is the number of instance records.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let count = r.i32()?;
        let mut m = MissionInstanceData::default();
        for u in m.unk.iter_mut() {
            *u = r.i32()?;
        }
        // Three more i32 values are read and discarded. `save` writes the instance count in these places.
        let _instance_count = r.i32()?;
        let _unk6 = r.i32()?;
        let _unk7 = r.i32()?;
        m.load_position = r.vec3()?;
        m.start_position = r.vec4()?;
        for _ in 0..count.max(0) {
            m.instances.push(MissionInstance {
                position: r.vec3()?,
                instance_id: r.i16()?,
                attached_to: r.i16()?,
                bounding_box: r.vec4()?,
            });
        }
        Ok(m)
    }
    /// Serialises the GEBI buffer in the layout that `load` reads. The instance count is written four times:
    /// first as the header, then after `unk` three times.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let count = self.instances.len() as i32;
        w.i32(count);
        for u in self.unk {
            w.i32(u);
        }
        w.i32(count);
        w.i32(count);
        w.i32(count);
        w.vec3(self.load_position);
        w.vec4(self.start_position);
        for i in &self.instances {
            w.vec3(i.position);
            w.i16(i.instance_id);
            w.i16(i.attached_to);
            w.vec4(i.bounding_box);
        }
        w.b
    }
}

// ---------------------------------------------------------------- logic + mission

/// The logic data of a mission (LELD package): strings, sound banks, actor and node definitions,
/// actor sets, wire collections and counters.
#[derive(Debug, Clone, PartialEq)]
pub struct LogicData {
    pub strings: StringCollection,
    pub sound_banks: IntTable,
    pub actors: LogicCollection,
    pub nodes: LogicCollection,
    pub actor_sets: ActorSetTable,
    pub wires: WireCollections,
    pub counters: IntTable,
}

/// Empty logic data. The actor collection is created as an actor list and the node collection as a node list.
impl Default for LogicData {
    fn default() -> Self {
        LogicData {
            strings: StringCollection::default(),
            sound_banks: IntTable::default(),
            actors: LogicCollection::new(true),
            nodes: LogicCollection::new(false),
            actor_sets: ActorSetTable::default(),
            wires: WireCollections::default(),
            counters: IntTable::default(),
        }
    }
}

/// Returns the first child of `p` whose context is `context`.
pub fn first_child<'a>(p: &'a Package, context: i32) -> Option<&'a Node> {
    p.children.iter().find(|n| n.c().context == context)
}
/// Returns the first child of `p` whose context is `context`, mutably.
pub fn first_child_mut<'a>(p: &'a mut Package, context: i32) -> Option<&'a mut Node> {
    p.children.iter_mut().find(|n| n.c().context == context)
}

/// Returns the bytes of the buffer child of `p` with tag `tag`. Fails when `p` has no such buffer.
fn buf_of<'a>(p: &'a Package, tag: &str) -> Res<&'a [u8]> {
    match first_child(p, ctx(tag)).and_then(|n| n.as_buffer()) {
        Some(b) => Ok(b.data()),
        None => invalid(format!("missing buffer chunk '{tag}'")),
    }
}

/// Replaces the data of the buffer child of `p` with tag `tag`. The buffer is marked dirty and modified.
/// Fails when `p` has no such buffer.
fn set_buf(p: &mut Package, tag: &str, data: Vec<u8>) -> Res<()> {
    match first_child_mut(p, ctx(tag)).and_then(|n| n.as_buffer_mut()) {
        Some(b) => {
            b.set_data(data);
            Ok(())
        }
        None => invalid(format!("missing buffer chunk '{tag}'")),
    }
}

/// Returns the package child of `p` with tag `tag`. Fails when `p` has no such package.
fn pkg_of<'a>(p: &'a Package, tag: &str) -> Res<&'a Package> {
    match first_child(p, ctx(tag)).and_then(|n| n.as_package()) {
        Some(b) => Ok(b),
        None => invalid(format!("missing package chunk '{tag}'")),
    }
}

/// Returns the package child of `p` with tag `tag`, mutably. Fails when `p` has no such package.
fn pkg_of_mut<'a>(p: &'a mut Package, tag: &str) -> Res<&'a mut Package> {
    match first_child_mut(p, ctx(tag)).and_then(|n| n.as_package_mut()) {
        Some(b) => Ok(b),
        None => invalid(format!("missing package chunk '{tag}'")),
    }
}

impl LogicData {
    /// LogicExportData.Load; `p` = the LELD package
    pub fn load(p: &Package) -> Res<Self> {
        let actors = pkg_of(p, "LEAC")?;
        let nodes = pkg_of(p, "LENC")?;
        Ok(LogicData {
            strings: StringCollection::load(buf_of(p, "LESC")?)?,
            sound_banks: IntTable::load(buf_of(p, "LESB")?)?,
            actors: LogicCollection::load(true, buf_of(actors, "LEAD")?, buf_of(actors, "LEPR")?)?,
            nodes: LogicCollection::load(false, buf_of(nodes, "LEND")?, buf_of(nodes, "LEPR")?)?,
            actor_sets: ActorSetTable::load(buf_of(p, "LEAS")?)?,
            wires: WireCollections::load(buf_of(p, "LEWC")?)?,
            counters: IntTable::load(buf_of(p, "LECO")?)?,
        })
    }

    /// LogicExportData.Save: SetBuffer on every sub-buffer, in the C# order.
    pub fn save(&self, p: &mut Package) -> Res<()> {
        set_buf(p, "LESC", self.strings.save())?;
        set_buf(p, "LESB", self.sound_banks.save())?;
        let (d, pr) = self.actors.save()?;
        {
            let a = pkg_of_mut(p, "LEAC")?;
            set_buf(a, "LEAD", d)?;
            set_buf(a, "LEPR", pr)?;
        }
        let (d, pr) = self.nodes.save()?;
        {
            let n = pkg_of_mut(p, "LENC")?;
            set_buf(n, "LEND", d)?;
            set_buf(n, "LEPR", pr)?;
        }
        set_buf(p, "LEAS", self.actor_sets.save())?;
        set_buf(p, "LEWC", self.wires.save())?;
        set_buf(p, "LECO", self.counters.save())?;
        Ok(())
    }
}

/// One exported mission (EM__ package): the objects (EMOB), the optional instances (GEBI) and the logic (LELD).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportedMission {
    pub objects: ExportedMissionObjects,
    pub instances: Option<MissionInstanceData>,
    pub logic: LogicData,
}

impl ExportedMission {
    /// Reads the EM__ package `p`. EMOB and LELD are required. GEBI is optional.
    pub fn load(p: &Package) -> Res<Self> {
        let objects = ExportedMissionObjects::load(buf_of(p, "EMOB")?)?;
        let instances = match first_child(p, ctx("GEBI")).and_then(|n| n.as_buffer()) {
            Some(b) => Some(MissionInstanceData::load(b.data())?),
            None => None,
        };
        let logic = LogicData::load(pkg_of(p, "LELD")?)?;
        Ok(ExportedMission { objects, instances, logic })
    }

    /// Writes the objects into EMOB, the instances into GEBI (only when `p` already has a GEBI buffer),
    /// and the logic into LELD. `p` is the EM__ package.
    pub fn save(&mut self, p: &mut Package) -> Res<()> {
        set_buf(p, "EMOB", self.objects.save()?)?;
        if let Some(i) = &self.instances {
            if first_child(p, ctx("GEBI")).is_some() {
                set_buf(p, "GEBI", i.save())?;
            }
        }
        self.logic.save(pkg_of_mut(p, "LELD")?)?;
        Ok(())
    }
}

// ---------------------------------------------------------------- game mission file

/// Searches `p` and its packages for the child with context `context`. On success, `path` holds the
/// child indexes that lead from `p` to that node.
fn find_dfs(p: &Package, context: i32, path: &mut Vec<usize>) -> bool {
    // MissionScriptFile reacts to OnSpoolerLoaded, which fires children-first (post-order).
    for (i, n) in p.children.iter().enumerate() {
        path.push(i);
        if let Node::Package(q) = n {
            if find_dfs(q, context, path) {
                return true;
            }
        }
        if n.c().context == context {
            return true;
        }
        path.pop();
    }
    false
}

/// Returns the node at the index path `path` below `root`. `path` must not be empty, and every index
/// before the last must lead to a package. Panics otherwise.
fn node_at<'a>(root: &'a mut Package, path: &[usize]) -> &'a mut Node {
    let mut p = root;
    for &i in &path[..path.len() - 1] {
        p = p.children[i].as_package_mut().unwrap();
    }
    &mut p.children[path[path.len() - 1]]
}

/// A loaded mission file: the chunk tree, the parsed mission and summary, and the tree paths of the
/// EM__ package and the EMMS buffer.
#[derive(Debug, Clone)]
pub struct MissionFile {
    pub chunks: ChunkFile,
    pub mission: ExportedMission,
    pub summary: Option<MissionSummary>,
    mission_path: Vec<usize>,
    summary_path: Option<Vec<usize>>,
}

impl MissionFile {
    /// Builds a MissionFile from a parsed chunk file. Fails when there is no EM__ package.
    /// The EMMS summary is optional.
    pub fn from_chunks(mut chunks: ChunkFile) -> Res<Self> {
        let mut mp = vec![];
        if !find_dfs(&chunks.content, ctx("EM__"), &mut mp) {
            return invalid("no exported mission (EM__) chunk");
        }
        let mission = ExportedMission::load(node_at(&mut chunks.content, &mp).as_package().unwrap())?;

        let mut sp = vec![];
        let (summary, summary_path) = if find_dfs(&chunks.content, ctx("EMMS"), &mut sp) {
            let b = node_at(&mut chunks.content, &sp).as_buffer().map(|b| b.data().to_vec());
            match b {
                Some(d) => (Some(MissionSummary::load(&d, false)?), Some(sp)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        Ok(MissionFile { chunks, mission, summary, mission_path: mp, summary_path })
    }

    /// Parses `d` as a chunk file and builds a MissionFile from it with `from_chunks`.
    pub fn from_bytes(d: &[u8]) -> Res<Self> {
        Self::from_chunks(ChunkFile::from_bytes(d)?)
    }

    /// OnFileSaveBegin (no build info): save MissionData, then MissionSummary, then lay out the file.
    pub fn to_bytes(&mut self) -> Res<Vec<u8>> {
        let mp = self.mission_path.clone();
        let em = node_at(&mut self.chunks.content, &mp).as_package_mut().unwrap();
        self.mission.save(em)?;
        if let (Some(s), Some(sp)) = (&self.summary, &self.summary_path) {
            let data = s.save();
            node_at(&mut self.chunks.content, sp).as_buffer_mut().unwrap().set_data(data);
        }
        Ok(self.chunks.to_bytes()?)
    }
}
