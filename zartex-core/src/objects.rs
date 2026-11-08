//! Port of ExportedMissionObjects + MissionObject + the 12 object types.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the file container reader/writer and the mission model).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! This file parses and writes the object list of a mission (EMOB buffer). The list holds 12 kinds of
//! object, and each kind is one variant of `MissionObject`. Vehicles, characters, areas and paths can
//! carry a size-prefixed creation block, and the code keeps or rebuilds it.
//! Uses `crate::binio` (the reader, the writer and the vector types).
//! Used by: `crate::mission`, `crate::dpl`, `crate::script`, `crate::luaraw`, `crate::readable`, several
//! `api_*` modules of this crate, and by `zartex-app` (`camtest`, `citysnap`, `main`, `model`, `view3d`
//! and `camera`).

use crate::binio::*;

/// One placed object of a mission. Each variant is one of the 12 object types. The number in the
/// doc comment of a variant is the type id that is stored in the file.
#[derive(Debug, Clone, PartialEq)]
pub enum MissionObject {
    /// type 1
    Vehicle { creation_data: Vec<u8>, position: Vec3, uid: i32 },
    /// type 2
    Character { creation_data: Vec<u8>, position: Vec3, skin_id: u32, uid: i32 },
    /// type 3
    Volume { reserved: i32, position: Vec3 },
    /// type 4
    Area { data: [i32; 4], scale: Vec4, position: Vec4, data5: i32, v1: Vec3 },
    /// type 5
    Path { creation_data: Vec<u8>, count: i64, path: Vec<Vec4> },
    /// type 6
    ObjectiveIcon { position: Vec3, rotation: Vec2, kind: i32 },
    /// type 7
    Camera { right: Vec3, up: Vec3, forward: Vec3, position: Vec3, reserved: i32 },
    /// type 8
    Prop { id: i32, unk: [i32; 5] },
    /// type 9
    Switch { position: Vec3, direction: Vec4, uid: i32 },
    /// type 10
    Collectable { kind: i32, rotation: f32, position: Vec3 },
    /// type 11
    AnimProp { position: Vec3 },
    /// type 12
    Marker { v1: Vec3, position: Vec3 },
}

impl MissionObject {
    /// Returns the numeric type id (1 to 12) of this object, as stored in the file.
    pub fn type_id(&self) -> i32 {
        use MissionObject::*;
        match self {
            Vehicle { .. } => 1,
            Character { .. } => 2,
            Volume { .. } => 3,
            Area { .. } => 4,
            Path { .. } => 5,
            ObjectiveIcon { .. } => 6,
            Camera { .. } => 7,
            Prop { .. } => 8,
            Switch { .. } => 9,
            Collectable { .. } => 10,
            AnimProp { .. } => 11,
            Marker { .. } => 12,
        }
    }

    /// True for the types that store a size-prefixed creation block: vehicle, character, area and path.
    fn has_creation_data(&self) -> bool {
        matches!(self, MissionObject::Vehicle { .. } | MissionObject::Character { .. } | MissionObject::Area { .. } | MissionObject::Path { .. })
    }

    /// MissionObject.Alignment (default 16, 4 for Vehicle / Character)
    fn alignment(&self) -> usize {
        match self {
            MissionObject::Vehicle { .. } | MissionObject::Character { .. } => 4,
            _ => 16,
        }
    }

    /// Construct a default object for a type id, then `load` fills it in.
    /// Fails for a type id outside 1 to 12.
    fn blank(type_id: i32) -> Res<MissionObject> {
        use MissionObject::*;
        let z3 = Vec3::default();
        let z4 = Vec4::default();
        Ok(match type_id {
            1 => Vehicle { creation_data: vec![], position: z3, uid: 0 },
            2 => Character { creation_data: vec![], position: z3, skin_id: 0, uid: 0 },
            3 => Volume { reserved: 0, position: z3 },
            4 => Area { data: [0; 4], scale: z4, position: z4, data5: 0, v1: z3 },
            5 => Path { creation_data: vec![], count: 0, path: vec![] },
            6 => ObjectiveIcon { position: z3, rotation: Vec2::default(), kind: 0 },
            7 => Camera { right: z3, up: z3, forward: z3, position: z3, reserved: 0 },
            8 => Prop { id: 0, unk: [0; 5] },
            9 => Switch { position: z3, direction: z4, uid: 0 },
            10 => Collectable { kind: 0, rotation: 0.0, position: z3 },
            11 => AnimProp { position: z3 },
            12 => Marker { v1: z3, position: z3 },
            _ => return invalid(format!("Couldn't create mission object -- invalid type '{type_id}'!")),
        })
    }

    /// MissionObject.Load (after the type id was read)
    fn load(&mut self, r: &mut Reader) -> Res<()> {
        let mut creation: Option<Vec<u8>> = None;
        // For the types with creation data: a u16 size, then a u16 whose high byte is the alignment.
        // The creation block is aligned and copied as it is. A size of 0 means there is no block.
        if self.has_creation_data() {
            let size = r.u16()? as usize;
            let alignment = ((r.u16()? >> 8) & 0xFF) as usize;
            if size != 0 {
                r.align(alignment);
                creation = Some(r.bytes(size)?.to_vec());
            }
        }
        use MissionObject::*;
        match self {
            Vehicle { creation_data, position, uid } => {
                if let Some(c) = creation {
                    *creation_data = c;
                }
                *position = r.vec3()?;
                *uid = r.i32()?;
            }
            Character { creation_data, position, skin_id, uid } => {
                if let Some(c) = creation {
                    *creation_data = c;
                }
                *position = r.vec3()?;
                *skin_id = r.u32()?;
                *uid = r.i32()?;
            }
            Volume { reserved, position } => {
                *reserved = r.i32()?;
                *position = r.vec3()?;
            }
            // Area takes all of its fields from the creation block. Its body in the object list is not read.
            Area { data, scale, position, data5, v1 } => {
                if let Some(c) = creation {
                    let mut cr = Reader::new(&c);
                    for d in data.iter_mut() {
                        *d = cr.i32()?;
                    }
                    *scale = cr.vec4()?;
                    *position = cr.vec4()?;
                    *data5 = cr.i32()?;
                    *v1 = cr.vec3()?;
                }
            }
            // Path keeps the raw creation block, then reads the points from it: an i64 count at byte 0,
            // and one Vec4 per point, the first at byte 16.
            Path { creation_data, count, path } => {
                if let Some(c) = creation {
                    *creation_data = c;
                }
                let mut cr = Reader::new(creation_data);
                *count = cr.i64()?;
                path.clear();
                for id in 0..(*count).max(0) as usize {
                    cr.pos = 16 * (id + 1);
                    path.push(cr.vec4()?);
                }
            }
            ObjectiveIcon { position, rotation, kind } => {
                *position = r.vec3()?;
                *rotation = r.vec2()?;
                *kind = r.i32()?;
            }
            Camera { right, up, forward, position, reserved } => {
                *right = r.vec3()?;
                *up = r.vec3()?;
                *forward = r.vec3()?;
                *position = r.vec3()?;
                *reserved = r.i32()?;
            }
            Prop { id, unk } => {
                *id = r.i32()?;
                for u in unk.iter_mut() {
                    *u = r.i32()?;
                }
            }
            Switch { position, direction, uid } => {
                *position = r.vec3()?;
                *direction = r.vec4()?;
                *uid = r.i32()?;
            }
            Collectable { kind, rotation, position } => {
                *kind = r.i32()?;
                *rotation = r.f32()?;
                *position = r.vec3()?;
            }
            AnimProp { position } => *position = r.vec3()?,
            Marker { v1, position } => {
                *v1 = r.vec3()?;
                *position = r.vec3()?;
            }
        }
        Ok(())
    }

    /// SaveCreationData; returns the bytes (and may update cached fields like the C# does).
    fn save_creation_data(&mut self) -> Res<Vec<u8>> {
        use MissionObject::*;
        match self {
            // The position is written back into the creation block at bytes 44 to 56, so that the block and
            // the object agree. A block shorter than 56 bytes is refused.
            Vehicle { creation_data, position, .. } => {
                if creation_data.len() < 56 {
                    return invalid("Vehicle creation data is too short to patch the position");
                }
                creation_data[44..48].copy_from_slice(&position.x.to_le_bytes());
                creation_data[48..52].copy_from_slice(&position.y.to_le_bytes());
                creation_data[52..56].copy_from_slice(&position.z.to_le_bytes());
                Ok(creation_data.clone())
            }
            Character { creation_data, .. } => Ok(creation_data.clone()),
            // The area block is rebuilt from its fields, in the same order that `load` reads them.
            Area { data, scale, position, data5, v1 } => {
                let mut w = Writer::new();
                for d in data.iter() {
                    w.i32(*d);
                }
                w.vec4(*scale);
                w.vec4(*position);
                w.i32(*data5);
                w.vec3(*v1);
                Ok(w.b)
            }
            // The path block is rebuilt: the i64 count at byte 0, then one Vec4 per point at 16 * (index + 1).
            // The count field is updated to the number of points.
            Path { creation_data, count, path } => {
                *count = path.len() as i64;
                let mut b = vec![0u8; 16 * (path.len() + 1)];
                b[0..8].copy_from_slice(&count.to_le_bytes());
                for (id, v) in path.iter().enumerate() {
                    let off = 16 * (id + 1);
                    b[off..off + 4].copy_from_slice(&v.x.to_le_bytes());
                    b[off + 4..off + 8].copy_from_slice(&v.y.to_le_bytes());
                    b[off + 8..off + 12].copy_from_slice(&v.z.to_le_bytes());
                    b[off + 12..off + 16].copy_from_slice(&v.w.to_le_bytes());
                }
                *creation_data = b.clone();
                Ok(b)
            }
            _ => invalid("Object does not implement creation data."),
        }
    }

    /// MissionObject.Save
    fn save(&mut self, w: &mut Writer) -> Res<()> {
        w.i32(self.type_id());
        if self.has_creation_data() {
            let buffer = self.save_creation_data()?;
            let size = buffer.len();
            if size != 0 {
                if size > 65535 {
                    return invalid("Too much creation data!");
                }
                // The header is the size in the low 16 bits, the constant 0xFB in bits 16 to 23,
                // and the alignment in the top byte. Then 0x3E3E3E3E words pad up to the alignment.
                let alignment = self.alignment();
                let header = (size as u32) | (0xFBu32 << 16) | ((alignment as u32) << 24);
                w.u32(header);
                while w.pos() & (alignment - 1) != 0 {
                    w.u32(0x3E3E3E3E);
                }
                w.bytes(&buffer);
            } else {
                w.i32(0);
            }
        }
        use MissionObject::*;
        match self {
            Vehicle { position, uid, .. } => {
                w.vec3(*position);
                w.i32(*uid);
            }
            Character { position, skin_id, uid, .. } => {
                w.vec3(*position);
                w.u32(*skin_id);
                w.i32(*uid);
            }
            Volume { reserved, position } => {
                w.i32(*reserved);
                w.vec3(*position);
            }
            // Area and Path have no body after the type id and the creation block.
            Area { .. } | Path { .. } => {}
            ObjectiveIcon { position, rotation, kind } => {
                w.vec3(*position);
                w.vec2(*rotation);
                w.i32(*kind);
            }
            Camera { right, up, forward, position, reserved } => {
                w.vec3(*right);
                w.vec3(*up);
                w.vec3(*forward);
                w.vec3(*position);
                w.i32(*reserved);
            }
            Prop { id, unk } => {
                w.i32(*id);
                for u in unk {
                    w.i32(*u);
                }
            }
            Switch { position, direction, uid } => {
                w.vec3(*position);
                w.vec4(*direction);
                w.i32(*uid);
            }
            Collectable { kind, rotation, position } => {
                w.i32(*kind);
                w.f32(*rotation);
                w.vec3(*position);
            }
            AnimProp { position } => w.vec3(*position),
            Marker { v1, position } => {
                w.vec3(*v1);
                w.vec3(*position);
            }
        }
        Ok(())
    }
}

/// The object list of a mission (EMOB buffer).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportedMissionObjects {
    pub objects: Vec<MissionObject>,
}

impl ExportedMissionObjects {
    /// Parses EMOB: an i32 count, then for each object its i32 type id followed by the object fields.
    pub fn load(d: &[u8]) -> Res<Self> {
        let mut r = Reader::new(d);
        let count = r.i32()?;
        let mut objects = Vec::new();
        for _ in 0..count.max(0) {
            let type_id = r.i32()?;
            let mut obj = MissionObject::blank(type_id)?;
            obj.load(&mut r)?;
            objects.push(obj);
        }
        Ok(ExportedMissionObjects { objects })
    }

    /// Serialises the object list in the layout that `load` reads. Fails when an object cannot be written.
    pub fn save(&mut self) -> Res<Vec<u8>> {
        let mut w = Writer::new();
        w.i32(self.objects.len() as i32);
        for o in &mut self.objects {
            o.save(&mut w)?;
        }
        Ok(w.b)
    }
}
