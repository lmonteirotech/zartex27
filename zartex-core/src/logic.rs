//! Port of Zartex.MissionScript.Logic.* (string collection, sound banks, counters, wires,
//! actor sets, node properties and definitions). Each type has `load(&[u8])` / `save() -> Vec<u8>`
//! mirroring the C# `Load()` / `Save()` byte layout.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the file container reader/writer and the mission model).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! This file holds the logic tables of a mission, one type per table: the string collection (LESC),
//! the sound banks (LESB), the actor and node definitions with their properties (LEAC, LENC and their
//! LEAD, LEND and LEPR buffers), the actor sets (LEAS), the wire collections (LEWC) and the counters (LECO).
//! Each table is parsed with `binio::Reader` and written with `binio::Writer`.
//! Uses `crate::binio` (the reader, the writer, `Res` and the vector types).
//! Used by: `crate::mission`, `crate::script`, `crate::luaraw`, `crate::readable`, `crate::compile`,
//! `crate::luaapi`, several `api_*` modules of this crate, and by `zartex-app` (`model`, `propedit`,
//! `camera`, `main`, `creator_tools` and `edittest`).

use crate::binio::*;

// ---------------------------------------------------------------- strings

/// The string table of a mission. Each entry is one string.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StringCollection {
    pub strings: Vec<String>,
}

impl StringCollection {
    /// Parses the LESC buffer: an i32 count, one i32 offset per string, then the zero-terminated strings.
    /// Each byte becomes one char.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let n = r.i32()?;
        let mut strings = Vec::new();
        for i in 0..n.max(0) as usize {
            r.pos = 4 + i * 4;
            let off = r.i32()? as usize;
            r.pos = off;
            let mut s = String::new();
            loop {
                let c = r.u8()?;
                if c == 0 {
                    break;
                }
                s.push(c as char); // ReadChar: one byte -> one char
            }
            strings.push(s);
        }
        Ok(StringCollection { strings })
    }

    /// Returns the index of `v`. When `v` is not in the table it is appended first.
    pub fn append_string(&mut self, v: &str) -> usize {
        if let Some(i) = self.strings.iter().position(|s| s == v) {
            return i;
        }
        self.strings.push(v.to_string());
        self.strings.len() - 1
    }

    /// Returns the index of `v` in the table, or None when it is not there.
    pub fn find_id(&self, v: &str) -> Option<usize> {
        self.strings.iter().position(|s| s == v)
    }

    /// Serialises the table in the LESC layout read by `load`. The offsets are computed first, then the
    /// strings are written, and the buffer is padded to the computed size.
    pub fn save(&self) -> Vec<u8> {
        let n = self.strings.len();
        let mut buf_size = 4 + n * 4;
        let mut offsets = Vec::with_capacity(n);
        for s in &self.strings {
            offsets.push(buf_size as i32);
            buf_size += s.encode_utf16().count() + 1;
        }
        let mut w = Writer::new();
        w.i32(n as i32);
        for o in offsets {
            w.i32(o);
        }
        for s in &self.strings {
            if !s.is_empty() {
                // Stream.Write(string): UTF8 bytes, but only `value.Length` (UTF-16 units) of them.
                let bytes = s.as_bytes();
                let len = s.encode_utf16().count();
                w.bytes(&bytes[..len.min(bytes.len())]);
            }
            w.u8(0);
        }
        w.b.resize(buf_size, 0);
        w.b
    }
}

// ---------------------------------------------------------------- int tables

/// A list of i32 values. Used for the sound bank list (LESB) and the counter list (LECO).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntTable {
    pub items: Vec<i32>,
}

impl IntTable {
    /// Parses a table: an i32 count, then that many i32 values.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let n = r.i32()?;
        let mut items = Vec::new();
        for _ in 0..n.max(0) {
            items.push(r.i32()?);
        }
        Ok(IntTable { items })
    }
    /// Serialises the table: the count, then the values.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.i32(self.items.len() as i32);
        for &i in &self.items {
            w.i32(i);
        }
        w.b
    }
    /// SoundBankTableData.AppendBank / ScriptCountersData.AppendActor.
    /// Appends `v` when it is not in the table yet, and returns the index of the new entry.
    /// When `v` is already present, the function returns `v` itself, not its index.
    pub fn append_unique(&mut self, v: i32) -> i32 {
        if !self.items.contains(&v) {
            let idx = self.items.len() as i32;
            self.items.push(v);
            idx
        } else {
            v
        }
    }
}

// ---------------------------------------------------------------- wires

/// One wire entry: the wire type, the operation code, and the id of the target node.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WireNode {
    pub wire_type: u8,
    pub op_code: u8,
    pub node_id: i16,
}

/// One list of wire entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WireCollection {
    pub wires: Vec<WireNode>,
}

/// All wire lists of a mission (LEWC buffer).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WireCollections {
    pub collections: Vec<WireCollection>,
}

impl WireCollections {
    /// Parses LEWC: an i32 count of lists. Each list is an i32 count of entries, and each entry is
    /// 4 bytes: wire type, operation code, and an i16 node id.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let n = r.i32()?;
        let mut collections = Vec::new();
        for _ in 0..n.max(0) {
            let nw = r.i32()?;
            let mut wires = Vec::new();
            for _ in 0..nw.max(0) {
                wires.push(WireNode { wire_type: r.u8()?, op_code: r.u8()?, node_id: r.i16()? });
            }
            collections.push(WireCollection { wires });
        }
        Ok(WireCollections { collections })
    }
    /// Serialises the wire lists in the same layout that `load` reads.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.i32(self.collections.len() as i32);
        for c in &self.collections {
            w.i32(c.wires.len() as i32);
            for x in &c.wires {
                w.u8(x.wire_type);
                w.u8(x.op_code);
                w.i16(x.node_id);
            }
        }
        w.b
    }
}

// ---------------------------------------------------------------- actor sets

/// The actor sets of a mission (LEAS buffer): a list of lists of i32 values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActorSetTable {
    pub table: Vec<Vec<i32>>,
}

impl ActorSetTable {
    /// Parses LEAS: an i32 count of sets. Each set is an i32 count, then that many i32 values.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let n = r.i32()?;
        let mut table = Vec::new();
        for _ in 0..n.max(0) {
            let c = r.i32()?;
            let mut set = Vec::new();
            for _ in 0..c.max(0) {
                set.push(r.i32()?);
            }
            table.push(set);
        }
        Ok(ActorSetTable { table })
    }
    /// Serialises the sets in the same layout that `load` reads.
    pub fn save(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.i32(self.table.len() as i32);
        for s in &self.table {
            w.i32(s.len() as i32);
            for &i in s {
                w.i32(i);
            }
        }
        w.b
    }
}

// ---------------------------------------------------------------- properties

/// The value of a node property. The variant is chosen by the property type id (see `Property::load`).
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// Types 1 (Integer), 6 (Enum), 7 (Actor), 9 (Flags), 19 (Wire collection),
    /// 20 (Localised string), 28 (Object type), 29 (Vehicle tint), 30 (Animation type)
    Int(i32),
    Float(f32),
    /// Type 3: index into the string collection
    Str(i16),
    Bool(bool),
    /// Type 8
    TextFile { value: i16, index: i16 },
    /// Type 15
    Uid(u64),
    /// Type 11
    Audio { bank: i32, sample: i32 },
    /// Type 17 (Float3, stored as 16 bytes)
    Vec4(Vec4),
    /// Type 23: right, up, forward, position
    Matrix { right: Vec4, up: Vec4, forward: Vec4, pos: Vec4 },
    /// Type 25
    Path(Vec<Vec4>),
    /// Type 21
    Unicode(Vec<u16>),
    /// Type 22 and every unknown type: size-prefixed raw bytes
    Raw(Vec<u8>),
    /// Type 32
    Assets { assets: Vec<i32>, densities: Vec<f32> },
}

/// A property of a node or actor definition: its type id, the string id of its name, and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub type_id: u8,
    pub string_id: i16,
    pub value: PropValue,
}

/// Reads an i32 size and fails with "Invalid <what> property!" when it is not `want`.
fn expect_size(r: &mut Reader, want: i32, what: &str) -> Res<()> {
    let size = r.i32()?;
    if size != want {
        return invalid(format!("Invalid {what} property!"));
    }
    Ok(())
}

impl Property {
    /// Returns the number of bytes that `write_to` stores for the value, which is the size field of the property header.
    pub fn size(&self) -> i32 {
        match &self.value {
            PropValue::Int(_) | PropValue::Float(_) | PropValue::TextFile { .. } => 4,
            PropValue::Str(_) => 2,
            PropValue::Bool(_) => 1,
            PropValue::Uid(_) | PropValue::Audio { .. } => 8,
            PropValue::Vec4(_) => 16,
            PropValue::Matrix { .. } => 64,
            PropValue::Path(p) => (p.len() * 16 + 4) as i32,
            PropValue::Unicode(u) => (u.len() * 2) as i32,
            PropValue::Raw(b) => b.len() as i32,
            PropValue::Assets { assets, densities } => 4 + ((assets.len() * 4) + (densities.len() * 4)) as i32,
        }
    }

    /// NodeProperty.Create: reads one property (type id, string id, size and value) from `r`.
    pub fn load(r: &mut Reader) -> Res<Property> {
        // The type id is the low byte of the first i16. The second header byte is not kept.
        let type_id = (r.i16()? & 0xFF) as u8;
        let string_id = r.i16()?;
        // Each arm checks the size field that comes before the value, where the type has a fixed size.
        let value = match type_id {
            1 | 6 | 7 | 9 | 19 | 20 | 28 | 29 | 30 => {
                expect_size(r, 4, "integer")?;
                PropValue::Int(r.i32()?)
            }
            2 => {
                expect_size(r, 4, "float")?;
                PropValue::Float(r.f32()?)
            }
            3 => {
                expect_size(r, 2, "string")?;
                PropValue::Str(r.i16()?)
            }
            // Type 4 (Bool): a size of 1 is followed by one byte. Any other size reads nothing,
            // and the value stays false.
            4 => {
                let size = r.i32()?;
                if size == 1 {
                    PropValue::Bool(r.u8()? != 0)
                } else {
                    PropValue::Bool(false) // C# reads nothing in this case
                }
            }
            8 => {
                expect_size(r, 4, "TextFile item")?;
                PropValue::TextFile { value: r.i16()?, index: r.i16()? }
            }
            15 => {
                expect_size(r, 8, "UID")?;
                PropValue::Uid(r.u64()?)
            }
            11 => {
                expect_size(r, 8, "audio")?;
                PropValue::Audio { bank: r.i32()?, sample: r.i32()? }
            }
            17 => {
                expect_size(r, 16, "vector")?;
                PropValue::Vec4(r.vec4()?)
            }
            23 => {
                expect_size(r, 64, "matrix")?;
                PropValue::Matrix { right: r.vec4()?, up: r.vec4()?, forward: r.vec4()?, pos: r.vec4()? }
            }
            // Type 25 (Path): the size field is skipped, then an i32 count and that many Vec4 points.
            25 => {
                r.pos += 4; // skip size
                let count = r.i32()?;
                let mut p = Vec::new();
                for _ in 0..count.max(0) {
                    p.push(r.vec4()?);
                }
                PropValue::Path(p)
            }
            // Type 21 (Unicode): the size in bytes is followed by that many bytes, read as UTF-16 units.
            // A trailing odd byte is dropped.
            21 => {
                let size = r.i32()? as usize;
                let buf = r.bytes(size)?;
                PropValue::Unicode(buf.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
            }
            // Type 32 (Assets): the size is skipped, then all asset ids are read, then all densities.
            32 => {
                let _size = r.i32()?;
                let count = r.i32()?.max(0) as usize;
                let mut assets = Vec::new();
                let mut densities = Vec::new();
                for _ in 0..count {
                    assets.push(r.i32()?);
                }
                for _ in 0..count {
                    densities.push(r.f32()?);
                }
                PropValue::Assets { assets, densities }
            }
            // Unknown types are kept as size-prefixed raw bytes.
            _ => {
                let size = r.i32()? as usize;
                PropValue::Raw(r.bytes(size)?.to_vec())
            }
        };
        Ok(Property { type_id, string_id, value })
    }

    /// NodeProperty.WriteTo: writes the property header (type id, string id, size) and then the value.
    pub fn write_to(&self, w: &mut Writer) -> Res<()> {
        w.u8(self.type_id);
        // The second byte of the header is always 0x3E.
        w.u8(0x3E);
        w.i16(self.string_id);
        w.i32(self.size());
        match &self.value {
            PropValue::Int(v) => w.i32(*v),
            PropValue::Float(v) => w.f32(*v),
            PropValue::Str(v) => w.i16(*v),
            PropValue::Bool(v) => w.u8(if *v { 255 } else { 0 }),
            PropValue::TextFile { value, index } => {
                w.i16(*value);
                w.i16(*index);
            }
            PropValue::Uid(v) => w.u64(*v),
            PropValue::Audio { bank, sample } => {
                w.i32(*bank);
                w.i32(*sample);
            }
            PropValue::Vec4(v) => w.vec4(*v),
            PropValue::Matrix { right, up, forward, pos } => {
                w.vec4(*right);
                w.vec4(*up);
                w.vec4(*forward);
                w.vec4(*pos);
            }
            PropValue::Path(p) => {
                w.i32(p.len() as i32);
                for v in p {
                    w.vec4(*v);
                }
            }
            PropValue::Unicode(u) => {
                for c in u {
                    w.u16(*c);
                }
            }
            PropValue::Raw(b) => w.bytes(b),
            PropValue::Assets { assets, densities } => {
                if assets.len() != densities.len() {
                    return invalid("count of Densities does not match with the count of Assets!");
                }
                w.i32(assets.len() as i32);
                for a in assets {
                    w.i32(*a);
                }
                for d in densities {
                    w.f32(*d);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- definitions

/// A node or actor definition: type id, string id, object id (used by actors only), a 4-byte colour,
/// the reserved and flags fields, and the list of properties.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeDef {
    pub type_id: u8,
    pub string_id: i16,
    /// Only meaningful for actor definitions
    pub object_id: i32,
    pub color: [u8; 4],
    pub reserved: i16,
    pub flags: i16,
    pub properties: Vec<Property>,
}

impl NodeDef {
    /// Creates a definition with no properties, object id 0, colour [127, 127, 127, 255], and zero reserved and flags.
    pub fn new(type_id: u8, string_id: i16) -> Self {
        NodeDef { type_id, string_id, object_id: 0, color: [127, 127, 127, 255], reserved: 0, flags: 0, properties: Vec::new() }
    }
}

/// A list of definitions. `is_actor` selects the actor layout (with the object id) or the node layout.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogicCollection {
    pub is_actor: bool,
    pub definitions: Vec<NodeDef>,
}

/// Rounds `n` up to a multiple of 4.
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

impl LogicCollection {
    /// Creates an empty collection. `is_actor` selects the actor layout.
    pub fn new(is_actor: bool) -> Self {
        LogicCollection { is_actor, definitions: Vec::new() }
    }

    /// `defs` = definitions table buffer, `props` = properties table buffer
    pub fn load(is_actor: bool, defs: &[u8], props: &[u8]) -> Res<Self> {
        let mut fd = Reader::new(defs);
        let mut fp = Reader::new(props);
        // The definition buffer and the property buffer are read in step. Both must hold the same count.
        let count = fp.i32()?;
        if fd.i32()? != count {
            return invalid("Number of definitions/properties mismatch!");
        }
        let mut out = Vec::new();
        for _ in 0..count.max(0) {
            let type_id = (fd.i16()? & 0xFF) as u8;
            let string_id = fd.i16()?;
            let object_id = if is_actor { fd.i32()? } else { 0 };
            let color = [fd.u8()?, fd.u8()?, fd.u8()?, fd.u8()?];
            let reserved = fd.i16()?;
            let flags = fd.i16()?;
            let nprops = fp.i32()?;
            let mut properties = Vec::new();
            for _ in 0..nprops.max(0) {
                properties.push(Property::load(&mut fp)?);
                // Every property is padded to a 4-byte boundary in the property buffer.
                if fp.pos & 3 != 0 {
                    fp.align(4);
                }
            }
            out.push(NodeDef { type_id, string_id, object_id, color, reserved, flags, properties });
        }
        Ok(LogicCollection { is_actor, definitions: out })
    }

    /// Returns (definitions buffer, properties buffer)
    pub fn save(&self) -> Res<(Vec<u8>, Vec<u8>)> {
        let count = self.definitions.len();
        let mut d = Writer::new();
        d.i32(count as i32);
        // The size of the properties buffer is computed first, with 4-byte padding after each property.
        // The buffer is then written and padded to that size.
        let mut p_size = 4usize;
        for def in &self.definitions {
            d.u8(def.type_id);
            d.u8(0x3E);
            d.i16(def.string_id);
            if self.is_actor {
                d.i32(def.object_id);
            }
            d.bytes(&def.color);
            d.i16(def.reserved);
            d.i16(def.flags);
            p_size += 4;
            for prop in &def.properties {
                p_size = align4(p_size);
                p_size += prop.size() as usize + 8;
            }
        }
        let mut p = Writer::new();
        p.i32(count as i32);
        for def in &self.definitions {
            p.i32(def.properties.len() as i32);
            for prop in &def.properties {
                prop.write_to(&mut p)?;
                // Padding bytes use the value 0x3E until the 4-byte boundary is reached.
                if p.pos() < p_size {
                    while p.pos() & 3 != 0 {
                        p.u8(0x3E);
                    }
                }
            }
        }
        p.b.resize(p_size.max(p.b.len()), 0);
        Ok((d.b, p.b))
    }
}
