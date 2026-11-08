//! Lossless conversion between a mission and a Lua script made of `MISSION.Raw*` calls.
//! `export_lua` writes a mission as Lua text: one `MISSION.RawObject` call per object, one `MISSION.RawActor` call per actor
//! and one `MISSION.RawNode` call per logic node, followed by the wires as `N[i].wireCollection.Add(N[j], type)` lines.
//! `register_raw` adds the `Raw*` functions to the `MISSION` table, so that a script which uses them compiles back to the
//! same mission (compile.rs calls it). Every property keeps its type id and its value; strings are written as Lua literals.
//! It uses `binio` (the `Vec2`, `Vec3` and `Vec4` types), `logic` (node, property and wire types), `luaapi` (`State`, and
//! `ret_actor` / `ret_node`, which return the created objects to Lua), `mission` (`ExportedMission`, `MissionSummary`,
//! `LogicData`), `nodenames` (node type names, written as comments), `objects` (`MissionObject`) and `script` (the state
//! helpers that create string ids, properties and wire collections).
//! It is used by `compile.rs` (which calls `register_raw`), by `readable.rs` (which falls back to `export_lua` when its
//! sandbox cannot be built), by the `zartex-app` crate (`main.rs` calls `export_lua`) and by `tests/lua_export.rs`.

use crate::binio::*;
use crate::logic::*;
use crate::luaapi::*;
use crate::mission::*;
use crate::nodenames;
use crate::objects::MissionObject;
use crate::script::*;
use mlua::{Lua, Table, Value};

// ---------------------------------------------------------------- export

/// Formats a float as Lua source text. NaN and the infinities become the Lua expressions `(0/0)`, `(1/0)` and `(-1/0)`;
/// other values use the Debug format, which is the shortest text that reads back as the same value.
pub(crate) fn ff(x: f32) -> String {
    if x.is_nan() {
        "(0/0)".into()
    } else if x.is_infinite() {
        if x > 0.0 { "(1/0)".into() } else { "(-1/0)".into() }
    } else {
        format!("{x:?}")
    }
}

/// Writes a string as a double-quoted Lua literal. Backslash, double quote, newline and carriage return are escaped;
/// any character outside 32..=126 is written as a three-digit decimal escape of its low byte.
pub(crate) fn lstr(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 32 || (c as u32) > 126 => o.push_str(&format!("\\{:03}", c as u32 & 0xFF)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Writes a byte slice as comma-separated decimal numbers, without brackets.
fn bytes(b: &[u8]) -> String {
    b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
}

/// Writes the four components of a Vec4 as comma-separated Lua numbers, formatted by `ff`.
fn v4(v: &Vec4) -> String {
    format!("{},{},{},{}", ff(v.x), ff(v.y), ff(v.z), ff(v.w))
}
/// Writes the three components of a Vec3 as comma-separated Lua numbers, formatted by `ff`.
fn v3(v: &Vec3) -> String {
    format!("{},{},{}", ff(v.x), ff(v.y), ff(v.z))
}

/// Writes one property as a Lua table `{type_id, name, kind, values...}`. The kind selects the value form that
/// `parse_prop` reads back: "i" int, "f" float, "s" string, "b" bool, "tf" text file (string and index), "u" uid
/// (high and low 32-bit halves), "au" audio (bank, sample), "v4", "m" matrix (four Vec4), "p" path (Vec4 list),
/// "uc" unicode (u16 values), "raw" (bytes) and "as" assets (count, asset ids, then densities).
/// The name is read from the logic string table; it is empty when the id does not exist.
pub(crate) fn prop_lua(l: &LogicData, p: &Property) -> String {
    let text = |id: i16| lstr(l.strings.strings.get(id.max(0) as usize).map(|s| s.as_str()).unwrap_or(""));
    let name = text(p.string_id);
    let body = match &p.value {
        PropValue::Int(x) => format!("\"i\", {x}"),
        PropValue::Float(x) => format!("\"f\", {}", ff(*x)),
        PropValue::Str(s) => format!("\"s\", {}", text(*s)),
        PropValue::Bool(b) => format!("\"b\", {b}"),
        PropValue::TextFile { value, index } => format!("\"tf\", {}, {index}", text(*value)),
        PropValue::Uid(u) => format!("\"u\", {}, {}", u >> 32, u & 0xFFFF_FFFF),
        PropValue::Audio { bank, sample } => format!("\"au\", {bank}, {sample}"),
        PropValue::Vec4(v) => format!("\"v4\", {}", v4(v)),
        PropValue::Matrix { right, up, forward, pos } => format!("\"m\", {}, {}, {}, {}", v4(right), v4(up), v4(forward), v4(pos)),
        PropValue::Path(pts) => format!("\"p\", {}", pts.iter().map(v4).collect::<Vec<_>>().join(", ")),
        PropValue::Unicode(u) => format!("\"uc\", {}", u.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")),
        PropValue::Raw(b) => format!("\"raw\", {}", bytes(b)),
        PropValue::Assets { assets, densities } => format!("\"as\", {}, {}{}{}", assets.len(), assets.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","), if assets.is_empty() { "" } else { ", " }, densities.iter().map(|x| ff(*x)).collect::<Vec<_>>().join(",")),
    };
    format!("{{{}, {}, {}}}", p.type_id, name, body)
}

/// Writes one mission object as a Lua table. The first element is the kind name ("vehicle", "character", ...),
/// which `parse_object` reads to choose the fields that follow.
pub(crate) fn object_lua(o: &MissionObject) -> String {
    use MissionObject::*;
    match o {
        Vehicle { creation_data, position, uid } => format!("{{\"vehicle\", {uid}, {}, {}}}", v3(position), bytes(creation_data)),
        Character { creation_data, position, skin_id, uid } => format!("{{\"character\", {uid}, {skin_id}, {}, {}}}", v3(position), bytes(creation_data)),
        Volume { reserved, position } => format!("{{\"volume\", {reserved}, {}}}", v3(position)),
        Area { data, scale, position, data5, v1 } => format!("{{\"area\", {},{},{},{}, {}, {}, {data5}, {}}}", data[0], data[1], data[2], data[3], v4(scale), v4(position), v3(v1)),
        Path { creation_data, count, path } => format!("{{\"path\", {count}, {}, {{{}}}, {}}}", creation_data.len(), bytes(creation_data), path.iter().map(v4).collect::<Vec<_>>().join(", ")),
        ObjectiveIcon { position, rotation, kind } => format!("{{\"icon\", {}, {},{}, {kind}}}", v3(position), ff(rotation.x), ff(rotation.y)),
        Camera { right, up, forward, position, reserved } => format!("{{\"camera\", {}, {}, {}, {}, {reserved}}}", v3(right), v3(up), v3(forward), v3(position)),
        Prop { id, unk } => format!("{{\"prop\", {id}, {},{},{},{},{}}}", unk[0], unk[1], unk[2], unk[3], unk[4]),
        Switch { position, direction, uid } => format!("{{\"switch\", {}, {}, {uid}}}", v3(position), v4(direction)),
        Collectable { kind, rotation, position } => format!("{{\"collectable\", {kind}, {}, {}}}", ff(*rotation), v3(position)),
        AnimProp { position } => format!("{{\"anim\", {}}}", v3(position)),
        Marker { v1, position } => format!("{{\"marker\", {}, {}}}", v3(v1), v3(position)),
    }
}

/// Writes a whole mission as Lua text (see the module header). `summary`, when present, becomes a `RawSummary` call.
/// `title` is shown in the first comment line. `game_dpl` selects the DPL table of logic node type names for the comments.
pub fn export_lua(m: &ExportedMission, summary: Option<&MissionSummary>, title: &str, game_dpl: bool) -> String {
    let l = &m.logic;
    let table = if game_dpl { nodenames::LOGIC_NODE_TYPES_DPL } else { nodenames::LOGIC_NODE_TYPES };
    let mut o = String::new();
    o.push_str(&format!("-- Mission exported by Zartex'27 from {title}\n"));
    o.push_str("-- Every call below recreates one thing of the mission exactly: edit values, add your own lines, or\n");
    o.push_str("-- build more of the mission in code. Compile it again with Compile Lua (or run it with Add to mission\n");
    o.push_str("-- on an open mission). Not covered: building instances, counters and actor-set tables.\n\n");
    if let Some(s) = summary {
        o.push_str(&format!("MISSION.RawSummary({}, {}, {}, {})   -- start x, start z, city type, mood id\n", ff(s.start_position.x), ff(s.start_position.y), s.city_type, s.mission_id));
    }
    o.push_str(&format!("MISSION.RawSoundBanks({{{}}})\n\n", l.sound_banks.items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")));
    o.push_str("local A, N = {}, {}\n\n-- objects (positions and creation data of vehicles, characters, cameras, areas, paths ...)\n");
    for (i, ob) in m.objects.objects.iter().enumerate() {
        o.push_str(&format!("MISSION.RawObject({}) -- object {i}\n", object_lua(ob)));
    }
    o.push_str("\n-- actors: type, note, object index, color r,g,b,a, reserved, flags, properties\n");
    for (i, a) in l.actors.definitions.iter().enumerate() {
        let note = l.strings.strings.get(a.string_id.max(0) as usize).cloned().unwrap_or_default();
        o.push_str(&format!("A[{i}] = MISSION.RawActor({}, {}, {}, {},{},{},{}, {}, {}, {{ -- actor {i}: {}\n", a.type_id, lstr(&note), a.object_id, a.color[0], a.color[1], a.color[2], a.color[3], a.reserved, a.flags, nodenames::lookup(nodenames::ACTOR_NODE_TYPES, a.type_id).unwrap_or("?")));
        for p in &a.properties {
            o.push_str(&format!("  {},\n", prop_lua(l, p)));
        }
        o.push_str("})\n");
    }
    o.push_str("\n-- logic nodes: type, note, color r,g,b,a, reserved, flags, has wire collection, properties\n");
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        let note = l.strings.strings.get(n.string_id.max(0) as usize).cloned().unwrap_or_default();
        // A node with a wire collection has type 19 (pWireCollection) as its first property. That property is not written
        // again in the property list: `RawNode` creates it from the `has_wire` flag.
        let has_wire = matches!(n.properties.first(), Some(Property { type_id: 19, value: PropValue::Int(_), .. }));
        o.push_str(&format!("N[{i}] = MISSION.RawNode({}, {}, {},{},{},{}, {}, {}, {has_wire}, {{ -- node {i}: {}\n", n.type_id, lstr(&note), n.color[0], n.color[1], n.color[2], n.color[3], n.reserved, n.flags, nodenames::lookup(table, n.type_id).unwrap_or("?")));
        for p in n.properties.iter().skip(if has_wire { 1 } else { 0 }) {
            o.push_str(&format!("  {},\n", prop_lua(l, p)));
        }
        o.push_str("})\n");
    }
    o.push_str("\n-- wires: N[from].wireCollection.Add(N[to], type)\n");
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        if let Some(Property { type_id: 19, value: PropValue::Int(w), .. }) = n.properties.first() {
            if let Some(c) = l.wires.collections.get(*w as usize) {
                for x in &c.wires {
                    if (x.node_id as usize) < l.nodes.definitions.len() && x.node_id >= 0 {
                        o.push_str(&format!("N[{i}].wireCollection.Add(N[{}], {})\n", x.node_id, x.wire_type));
                    }
                }
            }
        }
    }
    o
}

// ---------------------------------------------------------------- the Raw* calls

/// Reads a Lua string as text by mapping each byte to the char with the same value.
fn bstr(s: &mlua::String) -> String {
    s.as_bytes().iter().map(|&b| b as char).collect()
}

/// A cursor over the values of a Lua array table. `i` is the index of the last value read; each read moves to the next
/// index, so the first read returns element `i + 1`. Used for the flat value lists of the Raw calls.
struct Cur<'a> {
    t: &'a Table,
    i: i64,
}
impl<'a> Cur<'a> {
    /// Reads the next value as a float.
    fn f(&mut self) -> mlua::Result<f64> {
        self.i += 1;
        self.t.get::<f64>(self.i)
    }
    /// Reads the next value as an integer (the float is truncated toward zero).
    fn n(&mut self) -> mlua::Result<i64> {
        Ok(self.f()? as i64)
    }
    /// Reads three floats as a Vec3.
    fn v3(&mut self) -> mlua::Result<Vec3> {
        Ok(Vec3 { x: self.f()? as f32, y: self.f()? as f32, z: self.f()? as f32 })
    }
    /// Reads four floats as a Vec4.
    fn v4(&mut self) -> mlua::Result<Vec4> {
        Ok(Vec4 { x: self.f()? as f32, y: self.f()? as f32, z: self.f()? as f32, w: self.f()? as f32 })
    }
    /// The number of values after the cursor that have not been read yet. Variable-length tails use it.
    fn left(&self) -> i64 {
        self.t.raw_len() as i64 - self.i
    }
    /// Reads `n` values as bytes.
    fn bytes(&mut self, n: i64) -> mlua::Result<Vec<u8>> {
        (0..n).map(|_| self.n().map(|x| x as u8)).collect()
    }
}

/// Reads one property table written by `prop_lua`: element 1 is the type id, 2 the name, 3 the kind, and the values
/// follow from element 4. The name is added to the string table of the state.
fn parse_prop(s: &mut ScriptState, t: &Table) -> mlua::Result<Property> {
    let ty: i64 = t.get(1)?;
    let name = bstr(&t.get::<mlua::String>(2)?);
    let kind = bstr(&t.get::<mlua::String>(3)?);
    let mut c = Cur { t, i: 3 };
    let value = match kind.as_str() {
        "i" => PropValue::Int(c.n()? as i32),
        "f" => PropValue::Float(c.f()? as f32),
        "b" => PropValue::Bool(t.get::<bool>(4)?),
        "s" => PropValue::Str(s.string_id(&bstr(&t.get::<mlua::String>(4)?))),
            // A text-file value is a string (element 4) followed by an index (element 5).
        "tf" => {
            let value = s.string_id(&bstr(&t.get::<mlua::String>(4)?));
            PropValue::TextFile { value, index: t.get::<f64>(5)? as i16 }
        }
        "u" => PropValue::Uid(((c.f()? as u64) << 32) | (c.f()? as u64)),
        "au" => PropValue::Audio { bank: c.n()? as i32, sample: c.n()? as i32 },
        "v4" => PropValue::Vec4(c.v4()?),
        "m" => PropValue::Matrix { right: c.v4()?, up: c.v4()?, forward: c.v4()?, pos: c.v4()? },
        "p" => {
            let n = c.left() / 4;
            PropValue::Path((0..n).map(|_| c.v4()).collect::<mlua::Result<Vec<_>>>()?)
        }
        "uc" => {
            let n = c.left();
            PropValue::Unicode((0..n).map(|_| c.n().map(|x| x as u16)).collect::<mlua::Result<Vec<_>>>()?)
        }
        "raw" => {
            let n = c.left();
            PropValue::Raw(c.bytes(n)?)
        }
        "as" => {
            let n = c.n()?;
            let assets = (0..n).map(|_| c.n().map(|x| x as i32)).collect::<mlua::Result<Vec<_>>>()?;
            let densities = (0..n).map(|_| c.f().map(|x| x as f32)).collect::<mlua::Result<Vec<_>>>()?;
            PropValue::Assets { assets, densities }
        }
        other => return Err(mlua::Error::runtime(format!("unknown property kind '{other}'"))),
    };
    let string_id = s.string_id(&name);
    Ok(Property { type_id: ty as u8, string_id, value })
}

/// Reads one object table written by `object_lua`: element 1 is the kind name, and the values follow in the order
/// that `object_lua` wrote them.
fn parse_object(t: &Table) -> mlua::Result<MissionObject> {
    use MissionObject::*;
    let kind = bstr(&t.get::<mlua::String>(1)?);
    let mut c = Cur { t, i: 1 };
    Ok(match kind.as_str() {
        "vehicle" => {
            let uid = c.n()? as i32;
            let position = c.v3()?;
            let n = c.left();
            Vehicle { uid, position, creation_data: c.bytes(n)? }
        }
        "character" => {
            let uid = c.n()? as i32;
            let skin_id = c.n()? as u32;
            let position = c.v3()?;
            let n = c.left();
            Character { uid, skin_id, position, creation_data: c.bytes(n)? }
        }
        "volume" => Volume { reserved: c.n()? as i32, position: c.v3()? },
        "area" => Area { data: [c.n()? as i32, c.n()? as i32, c.n()? as i32, c.n()? as i32], scale: c.v4()?, position: c.v4()?, data5: c.n()? as i32, v1: c.v3()? },
        "path" => {
            let count = c.n()?;
            let nb = c.n()?;
            // The creation bytes of a path are a nested table: it is the element right after the two count values.
            let ct: Table = t.get(c.i + 1)?;
            c.i += 1;
            let mut cc = Cur { t: &ct, i: 0 };
            let creation_data = cc.bytes(nb)?;
            let n = c.left() / 4;
            Path { count, creation_data, path: (0..n).map(|_| c.v4()).collect::<mlua::Result<Vec<_>>>()? }
        }
        "icon" => ObjectiveIcon { position: c.v3()?, rotation: Vec2 { x: c.f()? as f32, y: c.f()? as f32 }, kind: c.n()? as i32 },
        "camera" => Camera { right: c.v3()?, up: c.v3()?, forward: c.v3()?, position: c.v3()?, reserved: c.n()? as i32 },
        "prop" => Prop { id: c.n()? as i32, unk: [c.n()? as i32, c.n()? as i32, c.n()? as i32, c.n()? as i32, c.n()? as i32] },
        "switch" => Switch { position: c.v3()?, direction: c.v4()?, uid: c.n()? as i32 },
        "collectable" => Collectable { kind: c.n()? as i32, rotation: c.f()? as f32, position: c.v3()? },
        "anim" => AnimProp { position: c.v3()? },
        "marker" => Marker { v1: c.v3()?, position: c.v3()? },
        other => return Err(mlua::Error::runtime(format!("unknown object kind '{other}'"))),
    })
}

/// Adds the `Raw*` functions to the `MISSION` table: `RawSummary`, `RawSoundBanks`, `RawObject`, `RawActor` and
/// `RawNode`. Each one writes into the shared state. `RawObject` returns the object index; `RawActor` and `RawNode`
/// return an actor or node handle (`ret_actor`, `ret_node`).
pub fn register_raw(lua: &Lua, mission: &Table, st: &State) -> mlua::Result<()> {
    // Each function keeps a clone of the shared state handle (an Rc), so all of them write into the same state.
    let s1 = st.clone();
    mission.set(
        "RawSummary",
        lua.create_function(move |_, (x, z, city, mood): (f64, f64, i64, i64)| {
            let mut s = s1.borrow_mut();
            s.summary.start_position = Vec2 { x: x as f32, y: z as f32 };
            s.summary.city_type = city as i32;
            s.summary.mood_id = mood as i16;
            Ok(())
        })?,
    )?;
    let s2 = st.clone();
    mission.set(
        "RawSoundBanks",
        lua.create_function(move |_, t: Table| {
            let mut v = Vec::new();
            for i in 1..=t.raw_len() {
                v.push(t.get::<f64>(i as i64)? as i32);
            }
            s2.borrow_mut().logic.sound_banks.items = v;
            Ok(())
        })?,
    )?;
    let s3 = st.clone();
    mission.set(
        "RawObject",
        lua.create_function(move |_, t: Table| {
            let o = parse_object(&t)?;
            let mut s = s3.borrow_mut();
            s.objects.push(o);
            Ok(s.objects.len() as i64 - 1)
        })?,
    )?;
    let s4 = st.clone();
    mission.set(
        "RawActor",
        lua.create_function(move |lua, (ty, note, object, r, g, b, a, reserved, flags, props): (i64, mlua::String, i64, i64, i64, i64, i64, i64, i64, Table)| -> mlua::Result<Value> {
            let idx = {
                let mut s = s4.borrow_mut();
                let string_id = s.string_id(&bstr(&note));
                let mut def = NodeDef::new(ty as u8, string_id);
                def.object_id = object as i32;
                def.color = [r as u8, g as u8, b as u8, a as u8];
                def.reserved = reserved as i16;
                def.flags = flags as i16;
                for i in 1..=props.raw_len() {
                    let p: Table = props.get(i as i64)?;
                    let pr = parse_prop(&mut s, &p)?;
                    def.properties.push(pr);
                }
                s.logic.actors.definitions.push(def);
                s.logic.actors.definitions.len() - 1
            };
            ret_actor(lua, &s4, idx)
        })?,
    )?;
    let s5 = st.clone();
    mission.set(
        "RawNode",
        lua.create_function(move |lua, (ty, note, r, g, b, a, reserved, flags, has_wire, props): (i64, mlua::String, i64, i64, i64, i64, i64, i64, bool, Table)| -> mlua::Result<Value> {
            let (idx, wire) = {
                let mut s = s5.borrow_mut();
                let string_id = s.string_id(&bstr(&note));
                let mut def = NodeDef::new(ty as u8, string_id);
                def.color = [r as u8, g as u8, b as u8, a as u8];
                def.reserved = reserved as i16;
                def.flags = flags as i16;
                let mut wire = 0usize;
                // A node with a wire collection gets a new collection, and its pWireCollection property is added first.
                if has_wire {
                    wire = s.new_wire_collection();
                    let p = s.prop("pWireCollection", 19, PropValue::Int(wire as i32));
                    def.properties.push(p);
                }
                for i in 1..=props.raw_len() {
                    let p: Table = props.get(i as i64)?;
                    let pr = parse_prop(&mut s, &p)?;
                    def.properties.push(pr);
                }
                s.logic.nodes.definitions.push(def);
                (s.logic.nodes.definitions.len() - 1, wire)
            };
            ret_node(lua, &s5, idx, wire)
        })?,
    )?;
    Ok(())
}
