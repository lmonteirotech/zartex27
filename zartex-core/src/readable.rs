//! Readable Lua export: a mission written as calls of the mission API (`MISSION.CreateCharacter(...)`,
//! `MISSION.ActorCreation(...)`, `node.wireCollection.Add(other)`), the way a person would write it.
//!
//! The API goes from arguments to a node; this module learns the way back. For every creator it runs the function in a
//! sandbox, changes one argument at a time and watches which field of the produced actor, node or object changes: that
//! is where the argument lives. To export an item it rebuilds the arguments from the item, runs the call again in the
//! sandbox and keeps the call only if its result matches the original. The match is exact, with two exceptions that
//! `Stats` counts: `relaxed` ignores flags, colour alpha and property type ids, and a loose match for actors compares
//! only the object fields that the arguments control. Whatever no creator reproduces (and whatever the API cannot
//! express) is written with the raw `MISSION.Raw*` calls, item by item.
//! `export_readable` is the entry point. It uses `binio` (the vector types), `compile` (the Lua state of the sandbox),
//! `creators` (the table of creators it learns from), `logic`, `luaapi` (`State`), `luaraw` (the raw-call text and the
//! fallback to `export_lua`), `mission`, `nodenames` (type names for the comments) and `objects` (`MissionObject`).
//! It is used by the `zartex-app` crate (`main.rs` writes the readable export) and by `tests/readable.rs`.

use crate::binio::{Vec3, Vec4};
use crate::compile::{install_getters, new_lua, seed_state, setup_driv3r};
use crate::creators::{Creator, Param, CREATORS};
use crate::logic::*;
use crate::luaapi::State;
use crate::luaraw::{export_lua, ff, lstr, object_lua, prop_lua};
use crate::mission::*;
use crate::nodenames;
use crate::objects::MissionObject;
use mlua::Lua;
use std::collections::{BTreeSet, HashMap, HashSet};

/// How much of the mission came out as readable calls.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// Number of actors in the mission.
    pub actors: usize,
    /// Actors written as API calls.
    pub actors_readable: usize,
    /// Number of logic nodes in the mission.
    pub nodes: usize,
    /// Logic nodes written as API calls.
    pub nodes_readable: usize,
    /// Readable items whose flags, alpha or property type ids differ from what the API writes (see `relaxed`)
    pub normalized: usize,
    /// Readable actors whose object differs from the original in data the API cannot set (angle rounding, internal bytes)
    pub approximated: usize,
    /// How many items of each kind had to be written raw ("actor Character", "node CameraSelect", ...)
    pub raw_kinds: HashMap<String, usize>,
}

// ---------------------------------------------------------------- resolved definitions (strings by text)

/// A property value with string ids replaced by their text. A wire collection index (type 19) is stored as 0,
/// because the index differs between missions.
#[derive(Clone, PartialEq, Debug)]
enum RVal {
    Int(i32),
    Float(u32),
    Str(String),
    Bool(bool),
    Tf(String, i16),
    Other(PropValue),
}

/// One property of a resolved definition: its type id, its name and its value.
#[derive(Clone, PartialEq, Debug)]
struct RProp {
    ty: u8,
    name: String,
    val: RVal,
}

/// An actor or node with every string id replaced by its text, so two definitions from different string tables compare.
#[derive(Clone, PartialEq, Debug)]
struct RDef {
    ty: u8,
    note: String,
    color: [u8; 4],
    reserved: i16,
    flags: i16,
    props: Vec<RProp>,
}

/// What the API cannot choose: the game's own missions carry other flags, alpha and property type ids than the API
/// writes (it writes the values its own tool used). A readable line stands for the definition without them.
fn relaxed(d: &RDef) -> RDef {
    let mut r = d.clone();
    r.color[3] = 0;
    r.flags = 0;
    r.reserved = 0;
    for p in &mut r.props {
        p.ty = 0;
        p.name = p.name.to_lowercase();
    }
    r
}

/// The text of a string id in the logic string table. A negative id reads entry 0, and an id past the end gives an empty string.
fn text(l: &LogicData, id: i16) -> String {
    l.strings.strings.get(id.max(0) as usize).cloned().unwrap_or_default()
}

/// Resolves a node definition: string ids become text, and the wire collection index (type 19) becomes 0.
fn resolve(l: &LogicData, d: &NodeDef) -> RDef {
    RDef {
        ty: d.type_id,
        note: text(l, d.string_id),
        color: d.color,
        reserved: d.reserved,
        flags: d.flags,
        props: d
            .properties
            .iter()
            .map(|p| RProp {
                ty: p.type_id,
                name: text(l, p.string_id),
                val: match &p.value {
                    PropValue::Int(_) if p.type_id == 19 => RVal::Int(0),
                    PropValue::Int(i) => RVal::Int(*i),
                    PropValue::Float(f) => RVal::Float(f.to_bits()),
                    PropValue::Str(s) => RVal::Str(text(l, *s)),
                    PropValue::Bool(b) => RVal::Bool(*b),
                    PropValue::TextFile { value, index } => RVal::Tf(text(l, *value), *index),
                    other => RVal::Other(other.clone()),
                },
            })
            .collect(),
    }
}

// ---------------------------------------------------------------- object fields

/// Adds the x, y and z fields of a vector, named `n.x`, `n.y` and `n.z`.
fn push3(v: &mut Vec<(String, f64)>, n: &str, a: &Vec3) {
    v.push((format!("{n}.x"), a.x as f64));
    v.push((format!("{n}.y"), a.y as f64));
    v.push((format!("{n}.z"), a.z as f64));
}
/// Adds the x, y, z and w fields of a vector, named `n.x` to `n.w`.
fn push4(v: &mut Vec<(String, f64)>, n: &str, a: &Vec4) {
    push3(v, n, &Vec3 { x: a.x, y: a.y, z: a.z });
    v.push((format!("{n}.w"), a.w as f64));
}
/// Adds creation data twice: each byte as `bN`, and each little-endian float that starts at byte N (N in steps of 4) as `fN`.
fn push_bytes(v: &mut Vec<(String, f64)>, b: &[u8]) {
    for (i, x) in b.iter().enumerate() {
        v.push((format!("b{i}"), *x as f64));
    }
    let mut off = 0;
    while off + 4 <= b.len() {
        v.push((format!("f{off}"), f32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]) as f64));
        off += 4;
    }
}

/// Every number of an object under a name, so a creator's argument can be traced to the field that holds it.
fn views(o: &MissionObject) -> Vec<(String, f64)> {
    use MissionObject::*;
    let mut v = Vec::new();
    match o {
        Vehicle { creation_data, position, uid } => {
            push3(&mut v, "pos", position);
            v.push(("uid".into(), *uid as f64));
            push_bytes(&mut v, creation_data);
        }
        Character { creation_data, position, skin_id, uid } => {
            push3(&mut v, "pos", position);
            v.push(("skin".into(), *skin_id as f64));
            v.push(("uid".into(), *uid as f64));
            push_bytes(&mut v, creation_data);
        }
        Volume { reserved, position } => {
            push3(&mut v, "pos", position);
            v.push(("reserved".into(), *reserved as f64));
        }
        Area { data, scale, position, data5, v1 } => {
            push4(&mut v, "pos", position);
            push4(&mut v, "scale", scale);
            for (i, d) in data.iter().enumerate() {
                v.push((format!("d{i}"), *d as f64));
            }
            v.push(("d5".into(), *data5 as f64));
            push3(&mut v, "v1", v1);
        }
        Path { creation_data, count, .. } => {
            v.push(("count".into(), *count as f64));
            push_bytes(&mut v, creation_data);
        }
        ObjectiveIcon { position, rotation, kind } => {
            push3(&mut v, "pos", position);
            v.push(("rot.x".into(), rotation.x as f64));
            v.push(("rot.y".into(), rotation.y as f64));
            v.push(("kind".into(), *kind as f64));
        }
        Camera { right, up, forward, position, reserved } => {
            push3(&mut v, "pos", position);
            push3(&mut v, "right", right);
            push3(&mut v, "up", up);
            push3(&mut v, "fwd", forward);
            v.push(("reserved".into(), *reserved as f64));
        }
        Prop { id, unk } => {
            v.push(("id".into(), *id as f64));
            for (i, u) in unk.iter().enumerate() {
                v.push((format!("u{i}"), *u as f64));
            }
        }
        Switch { position, direction, uid } => {
            push3(&mut v, "pos", position);
            push4(&mut v, "dir", direction);
            v.push(("uid".into(), *uid as f64));
        }
        Collectable { kind, rotation, position } => {
            push3(&mut v, "pos", position);
            v.push(("kind".into(), *kind as f64));
            v.push(("rot".into(), *rotation as f64));
        }
        AnimProp { position } => push3(&mut v, "pos", position),
        Marker { v1, position } => {
            push3(&mut v, "pos", position);
            push3(&mut v, "v1", v1);
        }
    }
    v
}

// ---------------------------------------------------------------- sandbox

/// The result of one trial: the definition of the item the call added, and its object when the item is an actor with one.
struct Tr {
    def: RDef,
    obj: Option<MissionObject>,
}

/// The mission loaded in a Lua state; each trial runs one creator call and rolls the state back.
struct Sandbox {
    lua: Lua,
    st: State,
    /// The type name of each actor, in the order of the actor list.
    kinds: Vec<String>,
}

/// The Lua text of a creator call, `MISSION.name(args)`, with the arguments separated by commas.
fn call(name: &str, args: &[String]) -> String {
    format!("MISSION.{name}({})", args.join(", "))
}

impl Sandbox {
    /// Loads the mission into a new Lua state with the Driv3r environment. Returns None when the state cannot be set up.
    fn new(m: &ExportedMission, summary: Option<&MissionSummary>) -> Option<Sandbox> {
        let st = seed_state(m, &summary.cloned());
        let lua = new_lua().ok()?;
        setup_driv3r(&lua, &st).ok()?;
        install_getters(&lua, &st).ok()?;
        let kinds = m.logic.actors.definitions.iter().map(|a| nodenames::lookup(nodenames::ACTOR_NODE_TYPES, a.type_id).unwrap_or("?").to_string()).collect();
        Some(Sandbox { lua, st, kinds })
    }

    /// Runs `call`; returns what it added when it added exactly one actor (or node), then restores the state.
    fn trial(&self, code: &str, node: bool) -> Option<Tr> {
        let (na, nn, nw, no, ns, nv, nsetup, sum) = {
            let s = self.st.borrow();
            (s.logic.actors.definitions.len(), s.logic.nodes.definitions.len(), s.wires.len(), s.objects.len(), s.logic.strings.strings.len(), s.spool_vehicles.len(), s.actor_setup.len(), s.summary.clone())
        };
        let ran = self.lua.load(format!("return {code}").as_str()).eval::<mlua::Value>().is_ok();
        let out = if ran {
            let s = self.st.borrow();
            let l = &s.logic;
            if node {
                (l.nodes.definitions.len() == nn + 1 && l.actors.definitions.len() == na && s.objects.len() == no).then(|| Tr { def: resolve(l, &l.nodes.definitions[nn]), obj: None })
            } else if l.actors.definitions.len() == na + 1 && l.nodes.definitions.len() == nn && s.objects.len() <= no + 1 {
                let d = &l.actors.definitions[na];
                let obj = if s.objects.len() == no + 1 && d.object_id == no as i32 { Some(s.objects[no].clone()) } else { None };
                if obj.is_none() && d.object_id != -1 {
                    None
                } else {
                    Some(Tr { def: resolve(l, d), obj })
                }
            } else {
                None
            }
        } else {
            None
        };
        let mut s = self.st.borrow_mut();
        s.logic.actors.definitions.truncate(na);
        s.logic.nodes.definitions.truncate(nn);
        s.wires.truncate(nw);
        s.objects.truncate(no);
        s.logic.strings.strings.truncate(ns);
        s.spool_vehicles.truncate(nv);
        s.actor_setup.truncate(nsetup);
        s.summary = sum;
        out
    }
}

// ---------------------------------------------------------------- learning where each argument lives

/// How a colour argument is stored in the colour byte.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ColorFn {
    /// The byte is the argument itself (the argument & 255).
    Identity,
    /// `node_color(x, ...)` stores `(x * 255) & 0xFF`
    Mul255,
}

/// Which part of a property an argument sets.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// The whole value of the property.
    Whole,
    /// The text of a text-file property.
    TfText,
    /// The index of a text-file property.
    TfIndex,
    /// The bank of an audio property.
    AudioBank,
    /// The sample of an audio property.
    AudioSample,
}

/// Where an argument lives in the item it makes.
#[derive(Clone, Debug)]
enum Loc {
    /// The argument changes nothing we could see: written as its default
    None,
    /// The note text of the item.
    Note,
    /// A colour channel of the item (index 0 to 3), stored as given by `ColorFn`.
    Color(usize, ColorFn),
    /// A property of the item: its index in the property list, and the part of the property.
    Prop(usize, Part),
    /// A field of the object, named as in `views`.
    Obj(String),
    /// An angle in degrees, stored as cos and sin in two object fields
    Angle(String, String),
}

/// An argument of a call: Lua text, an actor by index (`MISSION.GetActor(i)`), nil, or an angle in degrees.
#[derive(Clone, Debug, PartialEq)]
enum A {
    /// Lua text, written as it is.
    Lit(String),
    /// The actor with this index in the mission.
    Actor(usize),
    Nil,
    /// An angle in degrees, written with the rounding of `fmt_angle`.
    Angle(f64),
}

/// A value that a probe passes to a creator while learning, with its Lua type.
enum Pv {
    I(i64),
    F(f64),
    S(String),
    B(bool),
    Act(usize),
}

/// A creator that was learned: the creator, the type id of the item its base call makes, where each argument lives,
/// how many arguments it needs, and the argument values of its base call.
struct Learned {
    /// The creator.
    c: &'static Creator,
    /// The type id of the item that the base call makes.
    ty: u8,
    /// Where each argument lives, in parameter order.
    locs: Vec<Loc>,
    /// Arguments the creator needs; the rest may be left out when they equal the defaults
    min_args: usize,
    /// The arguments of the base call, read back from the item.
    base: Vec<A>,
}

/// Formats a float as Lua text with the fewest digits: whole numbers without a decimal point, `-0.0` for negative zero,
/// and NaN and infinities as in `ff`.
pub(crate) fn fmt_f32(x: f32) -> String {
    if x.is_nan() || x.is_infinite() {
        return ff(x);
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0".into() } else { "0".into() };
    }
    if x.fract() == 0.0 && x.abs() < 1e9 {
        return format!("{}", x as i64);
    }
    format!("{x:?}")
}

/// Formats an angle in degrees, rounded to `variant` decimal places (0 to 5). Variant 6 or more writes the full float.
fn fmt_angle(d: f64, variant: usize) -> String {
    if variant >= 6 {
        return fmt_f32(d as f32);
    }
    let k = 10f64.powi(variant as i32);
    let r = (d * k).round() / k;
    if r == 0.0 {
        "0".into()
    } else if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// Chooses an actor for an argument from its parameter name (for example `veh`, `car` or `truck` select a vehicle).
/// Returns the first actor of the chosen kind, or else the first actor.
fn pick_actor(kinds: &[String], name: &str) -> Option<usize> {
    let n = name.to_lowercase();
    let want: &[&str] = if n.contains("char") || n.contains("player") || n.contains("ped") || n.contains("driver") || n.contains("target") || n.contains("chaser") {
        &["character", "cop"]
    } else if n.contains("veh") || n.contains("car") || n.contains("truck") || n.contains("bike") || n.contains("boat") {
        &["vehicle"]
    } else if n.contains("area") {
        &["area"]
    } else if n.contains("volume") || n.contains("zone") {
        &["volume"]
    } else if n.contains("cam") {
        &["camera"]
    } else if n.contains("path") {
        &["path"]
    } else if n.contains("prop") || n.contains("anim") {
        &["anim", "prop"]
    } else {
        &[]
    };
    kinds.iter().position(|k| want.iter().any(|w| k.to_lowercase().contains(w))).or(if kinds.is_empty() { None } else { Some(0) })
}

/// Finds where a probed argument lives, by comparing the result of the base call with the result of the probe.
/// It checks the note, then the colour bytes, then the properties, then the object fields. For a float argument, two
/// object fields that change together give an angle.
fn locate(pv: &Pv, base: &Tr, pr: &Tr) -> Loc {
    if let Pv::S(s) = pv {
        if base.def.note != pr.def.note && pr.def.note == *s {
            return Loc::Note;
        }
    }
    if let Pv::I(v) = pv {
        for c in 0..4 {
            if base.def.color[c] != pr.def.color[c] {
                let b = pr.def.color[c] as i64;
                if b == (v & 255) {
                    return Loc::Color(c, ColorFn::Identity);
                }
                if b == (v * 255) & 255 {
                    return Loc::Color(c, ColorFn::Mul255);
                }
            }
        }
    }
    if base.def.props.len() == pr.def.props.len() {
        for (k, (a, b)) in base.def.props.iter().zip(&pr.def.props).enumerate() {
            if a.val == b.val {
                continue;
            }
            let hit = match (&b.val, pv) {
                (RVal::Int(x), Pv::I(v)) => (*x as i64 == *v).then_some(Part::Whole),
                (RVal::Int(x), Pv::Act(k2)) => (*x as i64 == *k2 as i64).then_some(Part::Whole),
                (RVal::Float(bits), Pv::F(v)) => (f32::from_bits(*bits) == *v as f32).then_some(Part::Whole),
                (RVal::Str(s), Pv::S(t)) => (s == t).then_some(Part::Whole),
                (RVal::Bool(x), Pv::B(y)) => (x == y).then_some(Part::Whole),
                (RVal::Tf(t, _), Pv::S(s)) => (t == s).then_some(Part::TfText),
                (RVal::Tf(_, i), Pv::I(v)) => (*i as i64 == *v).then_some(Part::TfIndex),
                (RVal::Other(PropValue::Audio { bank, sample }), Pv::I(v)) => {
                    if let RVal::Other(PropValue::Audio { bank: ob, sample: os }) = &a.val {
                        if *bank as i64 == *v && ob != bank {
                            Some(Part::AudioBank)
                        } else if *sample as i64 == *v && os != sample {
                            Some(Part::AudioSample)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(part) = hit {
                return Loc::Prop(k, part);
            }
        }
    }
    if let (Some(bo), Some(po)) = (&base.obj, &pr.obj) {
        let (bv, pvw) = (views(bo), views(po));
        if bv.len() == pvw.len() {
            let mut changed: Vec<&str> = Vec::new();
            for ((k, a), (_, b)) in bv.iter().zip(&pvw) {
                if a == b || (a.is_nan() && b.is_nan()) {
                    continue;
                }
                let hit = match pv {
                    Pv::I(v) => *b == *v as f64,
                    Pv::F(v) => (*b as f32) == (*v as f32),
                    _ => false,
                };
                if hit {
                    return Loc::Obj(k.clone());
                }
                if !k.starts_with('b') {
                    changed.push(k);
                }
            }
            if matches!(pv, Pv::F(_)) && changed.len() >= 2 {
                return Loc::Angle(changed[0].to_string(), changed[1].to_string());
            }
        }
    }
    Loc::None
}

/// First actor of each kind, then the rest: the actors a creator might accept for an actor argument.
fn actor_candidates(kinds: &[String], name: &str) -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    if let Some(k) = pick_actor(kinds, name) {
        v.push(k);
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for (i, k) in kinds.iter().enumerate() {
        if seen.insert(k.as_str()) && !v.contains(&i) {
            v.push(i);
        }
    }
    v
}

/// Learns where the arguments of one creator live. It runs the creator with default arguments (trying the actors of
/// each kind until one works), probes each argument with another value, and finds the smallest number of arguments
/// that still gives the same item. Creators with a table parameter are skipped.
fn learn(sb: &Sandbox, c: &'static Creator) -> Option<Learned> {
    if c.params.iter().any(|p| p.ty == "Table") {
        return None;
    }
    let n = c.params.len();
    // actor arguments: try the likeliest actor of each kind until the creator accepts the call
    let actor_params: Vec<usize> = (0..n).filter(|&i| c.params[i].ty == "Actor" && c.params[i].default != "nil").collect();
    let cands: Vec<Vec<usize>> = actor_params.iter().map(|&i| actor_candidates(&sb.kinds, c.params[i].name)).collect();
    if cands.iter().any(|v| v.is_empty()) {
        return None;
    }
    let mut counter = vec![0usize; actor_params.len()];
    let mut found: Option<(Vec<String>, Vec<Option<usize>>, Tr)> = None;
    for _ in 0..300 {
        let mut base_args: Vec<String> = Vec::new();
        let mut base_pick: Vec<Option<usize>> = Vec::new();
        for (i, p) in c.params.iter().enumerate() {
            if p.ty == "Actor" && p.default == "nil" {
                base_args.push("nil".into());
                base_pick.push(None);
            } else if p.ty == "Actor" {
                let slot = actor_params.iter().position(|&x| x == i).unwrap();
                let k = cands[slot][counter[slot]];
                base_args.push(format!("MISSION.GetActor({k})"));
                base_pick.push(Some(k));
            } else {
                base_args.push(p.default.to_string());
                base_pick.push(None);
            }
        }
        if let Some(tr) = sb.trial(&call(c.name, &base_args), c.makes_node) {
            found = Some((base_args, base_pick, tr));
            break;
        }
        // next combination
        let mut d = 0;
        loop {
            if d == counter.len() {
                break;
            }
            counter[d] += 1;
            if counter[d] < cands[d].len() {
                break;
            }
            counter[d] = 0;
            d += 1;
        }
        if d == counter.len() {
            break;
        }
    }
    let Some((base_args, base_pick, base)) = found else {
        return None;
    };
    let mut locs = Vec::new();
    for (i, p) in c.params.iter().enumerate() {
        if p.ty == "Actor" {
            let same_kind = |k: &usize| base_pick[i].map(|b| sb.kinds[b] == sb.kinds[*k]).unwrap_or(false);
            let mut others: Vec<usize> = (0..sb.kinds.len()).filter(|k| Some(*k) != base_pick[i] && same_kind(k)).take(3).collect();
            others.extend((0..sb.kinds.len()).filter(|k| Some(*k) != base_pick[i] && !same_kind(k)).take(6));
            let mut loc = Loc::None;
            for k in others {
                let mut a = base_args.clone();
                a[i] = format!("MISSION.GetActor({k})");
                if let Some(tr) = sb.trial(&call(c.name, &a), c.makes_node) {
                    let l = locate(&Pv::Act(k), &base, &tr);
                    if !matches!(l, Loc::None) {
                        loc = l;
                        break;
                    }
                }
            }
            locs.push(loc);
            continue;
        }
        let probe: Option<(String, Pv)> = match p.ty {
            "Actor" => None,
            "int" | "short" | "uint" | "byte" => {
                let d = p.default.parse::<i64>().unwrap_or(0);
                let v = if p.ty == "byte" { (d + 7) % 250 } else { d + 7 };
                Some((v.to_string(), Pv::I(v)))
            }
            "float" | "double" => {
                let v = p.default.parse::<f64>().unwrap_or(0.0) + 1.5;
                Some((format!("{v}"), Pv::F(v)))
            }
            "bool" => {
                let d = p.default == "true";
                Some(((!d).to_string(), Pv::B(!d)))
            }
            "string" => {
                let s = format!("probe{i}");
                Some((lstr(&s), Pv::S(s)))
            }
            _ => None,
        };
        let loc = match probe {
            Some((txt, pv)) => {
                let mut a = base_args.clone();
                a[i] = txt;
                match sb.trial(&call(c.name, &a), c.makes_node) {
                    Some(tr) => locate(&pv, &base, &tr),
                    None => Loc::None,
                }
            }
            None => Loc::None,
        };
        locs.push(loc);
    }
    // an actor argument that could not be probed (only one actor of its kind): it is the property that holds the
    // index of the actor it was given
    for i in 0..n {
        if c.params[i].ty == "Actor" && matches!(locs[i], Loc::None) {
            if let Some(k) = base_pick[i] {
                let taken: Vec<usize> = locs.iter().filter_map(|l| if let Loc::Prop(j, Part::Whole) = l { Some(*j) } else { None }).collect();
                if let Some(j) = base.def.props.iter().position(|pr| pr.ty == 7 && pr.val == RVal::Int(k as i32)).filter(|j| !taken.contains(j)).or_else(|| base.def.props.iter().enumerate().position(|(j, pr)| pr.ty == 7 && pr.val == RVal::Int(k as i32) && !taken.contains(&j))) {
                    locs[i] = Loc::Prop(j, Part::Whole);
                }
            }
        }
    }
    let min_args = (0..=n).find(|&k| sb.trial(&call(c.name, &base_args[..k]), c.makes_node).map(|t| t.def == base.def && t.obj == base.obj).unwrap_or(false)).unwrap_or(n);
    let base_a: Vec<A> = c.params.iter().zip(&locs).map(|(p, l)| read(l, p, &base.def, base.obj.as_ref()).unwrap_or_else(|| A::Lit(p.default.to_string()))).collect();
    Some(Learned { c, ty: base.def.ty, locs, min_args, base: base_a })
}

// ---------------------------------------------------------------- from an item back to arguments

/// Reads an argument back from an item, given where it lives. For an actor parameter, a property value is an actor
/// index (a negative value means nil). Returns None when the location cannot be read.
fn read(loc: &Loc, p: &Param, def: &RDef, obj: Option<&MissionObject>) -> Option<A> {
    let float = matches!(p.ty, "float" | "double");
    let num = |x: f64| A::Lit(if float { fmt_f32(x as f32) } else { format!("{}", x as i64) });
    match loc {
        Loc::None => Some(A::Lit(p.default.to_string())),
        Loc::Note => Some(A::Lit(lstr(if def.note == "Unknown" { "" } else { &def.note }))),
        Loc::Color(c, f) => {
            let b = def.color[*c] as i64;
            Some(A::Lit(match f {
                ColorFn::Identity => b,
                ColorFn::Mul255 => (256 - b) % 256,
            }
            .to_string()))
        }
        Loc::Prop(k, part) => {
            let pr = def.props.get(*k)?;
            match (&pr.val, part) {
                (RVal::Int(i), Part::Whole) => {
                    if p.ty == "Actor" {
                        Some(if *i < 0 { A::Nil } else { A::Actor(*i as usize) })
                    } else {
                        Some(A::Lit(i.to_string()))
                    }
                }
                (RVal::Float(b), Part::Whole) => Some(A::Lit(fmt_f32(f32::from_bits(*b)))),
                (RVal::Str(s), Part::Whole) => Some(A::Lit(lstr(s))),
                (RVal::Bool(b), Part::Whole) => Some(A::Lit(b.to_string())),
                (RVal::Tf(t, _), Part::TfText) => Some(A::Lit(lstr(t))),
                (RVal::Tf(_, i), Part::TfIndex) => Some(A::Lit(i.to_string())),
                (RVal::Other(PropValue::Audio { bank, .. }), Part::AudioBank) => Some(A::Lit(bank.to_string())),
                (RVal::Other(PropValue::Audio { sample, .. }), Part::AudioSample) => Some(A::Lit(sample.to_string())),
                _ => None,
            }
        }
        Loc::Obj(key) => {
            let v = views(obj?).into_iter().find(|(k, _)| k == key)?.1;
            Some(num(v))
        }
        Loc::Angle(k1, k2) => {
            let vs = views(obj?);
            let c = vs.iter().find(|(k, _)| k == k1)?.1;
            let s = vs.iter().find(|(k, _)| k == k2)?.1;
            Some(A::Angle(s.atan2(c).to_degrees()))
        }
    }
}

/// Turns an argument into Lua text. `actor` gives the text for an actor index, and `variant` is the rounding of angles.
fn render(a: &A, variant: usize, actor: &dyn Fn(usize) -> String) -> String {
    match a {
        A::Lit(s) => s.clone(),
        A::Actor(i) => actor(*i),
        A::Nil => "nil".into(),
        A::Angle(d) => fmt_angle(*d, variant),
    }
}

/// A readable call found for one item: the learned creator, its arguments (only the first `shown` are written), the
/// angle rounding, and whether the call gives the item exactly or only approximately.
struct Chosen {
    /// The call gives the item exactly, in every field.
    exact: bool,
    /// The call was found by the loose match for actors.
    approx: bool,
    learned: usize,
    args: Vec<A>,
    /// How many arguments are written. The others equal the defaults and are left out.
    shown: usize,
    /// The angle rounding, see `fmt_angle`.
    variant: usize,
}

/// True when two numbers agree within 1e-3, plus 1e-6 times the larger value.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * (1.0 + a.abs().max(b.abs()) * 1e-3)
}

/// Same kind of object, and every field a creator argument controls (positions, ids, sizes) equal; the angle is
/// judged by the angle itself, and bytes no argument reaches are not compared.
fn loose_obj_eq(l: &Learned, a: Option<&MissionObject>, b: Option<&MissionObject>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            if x.type_id() != y.type_id() {
                return false;
            }
            let (vx, vy) = (views(x), views(y));
            l.locs.iter().all(|loc| match loc {
                Loc::Obj(k) => match (vx.iter().find(|(n, _)| n == k), vy.iter().find(|(n, _)| n == k)) {
                    (Some((_, p)), Some((_, q))) => close(*p, *q),
                    _ => false,
                },
                _ => true,
            })
        }
        _ => false,
    }
}

/// Finds a readable call for an item. An actor is first matched exactly; if that fails, it is matched loosely.
/// `idx` is the item's own index: an actor may only refer to earlier actors.
fn plan_item(sb: &Sandbox, learned: &[Learned], is_actor: bool, idx: usize, def: &RDef, orig_obj: Option<&MissionObject>, n_actors: usize) -> Option<Chosen> {
    plan_pass(sb, learned, is_actor, idx, def, orig_obj, n_actors, false).or_else(|| if is_actor { plan_pass(sb, learned, is_actor, idx, def, orig_obj, n_actors, true) } else { None })
}

/// One pass of `plan_item`. Each learned creator that makes the same kind of item is tried: its arguments are read from
/// the item, the call is run, and the first result that matches the item (with `relaxed` values) is kept. In `loose`
/// mode the object is compared with `loose_obj_eq`, and angles use one rounding.
fn plan_pass(sb: &Sandbox, learned: &[Learned], is_actor: bool, idx: usize, def: &RDef, orig_obj: Option<&MissionObject>, n_actors: usize, loose: bool) -> Option<Chosen> {
    'cand: for (li, l) in learned.iter().enumerate() {
        if l.c.makes_node == is_actor || l.ty != def.ty {
            continue;
        }
        let mut args = Vec::new();
        for (p, loc) in l.c.params.iter().zip(&l.locs) {
            match read(loc, p, def, orig_obj) {
                Some(a) => args.push(a),
                None => {
                    continue 'cand;
                }
            }
        }
        for a in &args {
            if matches!(a, A::Lit(t) if t.contains("GetActor")) {
                continue 'cand;
            }
            if let A::Actor(i) = a {
                if *i >= n_actors || (is_actor && *i >= idx) {
                    continue 'cand;
                }
            }
        }
        let n = args.len();
        let mut drop = n;
        while drop > l.min_args && args[drop - 1] == l.base[drop - 1] {
            drop -= 1;
        }
        let has_angle = args.iter().any(|a| matches!(a, A::Angle(_)));
        let variants: Vec<usize> = if !has_angle { vec![0] } else if loose { vec![3] } else { (0..7).collect() };
        for variant in variants {
            for shown in if drop == n { vec![n] } else { vec![drop, n] } {
                let txt: Vec<String> = args[..shown].iter().map(|a| render(a, variant, &|i| format!("MISSION.GetActor({i})"))).collect();
                if let Some(tr) = sb.trial(&call(l.c.name, &txt), !is_actor) {
                    let obj_ok = if loose { loose_obj_eq(l, tr.obj.as_ref(), orig_obj) } else { tr.obj.as_ref() == orig_obj };
                    if obj_ok && relaxed(&tr.def) == relaxed(def) {
                        return Some(Chosen { exact: tr.def == *def && !loose, approx: loose, learned: li, args, shown, variant });
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------- names

/// Lua keywords. A variable name of the script must not be one of them.
const LUA_WORDS: &[&str] = &["and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while"];

/// Turns a note into a Lua variable name: letters are made lower case, other characters become single underscores,
/// the ends are trimmed, and a leading digit gets an underscore. Returns None for an empty name, for a name longer than
/// 28 characters, and for `unknown`. The caller checks the keyword list.
fn ident(note: &str) -> Option<String> {
    let mut s = String::new();
    for ch in note.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() || s.len() > 28 || s == "unknown" {
        return None;
    }
    Some(if s.as_bytes()[0].is_ascii_digit() { format!("_{s}") } else { s })
}

/// How many items may get a local variable. The others are kept in the `RA` and `RN` tables, because Lua limits the
/// number of locals of a function.
const MAX_LOCALS: usize = 170;

/// The Lua names of the items: a local variable for each referenced item that got one, and a table entry for the others.
struct Names {
    local: HashMap<(bool, usize), String>,
    table_a: bool,
    table_n: bool,
}

impl Names {
    /// The Lua expression of an item: its local name, or `RA[i]` for an actor and `RN[i]` for a node.
    fn get(&self, actor: bool, idx: usize) -> String {
        match self.local.get(&(actor, idx)) {
            Some(n) => n.clone(),
            None => format!("{}[{idx}]", if actor { "RA" } else { "RN" }),
        }
    }
}

// ---------------------------------------------------------------- the exporter

/// The mission as a readable Lua script, and how much of it could be written as API calls.
/// Items no creator reproduces exactly are written with the raw calls, so the script always rebuilds the mission.
pub fn export_readable(m: &ExportedMission, summary: Option<&MissionSummary>, title: &str, game_dpl: bool) -> (String, Stats) {
    let l = &m.logic;
    let mut stats = Stats { actors: l.actors.definitions.len(), nodes: l.nodes.definitions.len(), ..Default::default() };
    // Without a sandbox the whole mission is written with the raw calls (export_lua).
    let Some(sb) = Sandbox::new(m, summary) else { return (export_lua(m, summary, title, game_dpl), stats) };
    let n_actors = l.actors.definitions.len();

    // learn the creators (only those that can produce something)
    let learned: Vec<Learned> = CREATORS.iter().filter_map(|c| learn(&sb, c)).collect();

    // decide, item by item, readable or raw
    let obj_of = |a: &NodeDef| -> Option<&MissionObject> { if a.object_id >= 0 { m.objects.objects.get(a.object_id as usize) } else { None } };
    let mut actor_plan: Vec<Option<Chosen>> = Vec::new();
    for (i, a) in l.actors.definitions.iter().enumerate() {
        // An object index below -1, or one past the object list, cannot be written as a readable call.
        let bad_obj = a.object_id < -1 || (a.object_id >= 0 && obj_of(a).is_none());
        actor_plan.push(if bad_obj { None } else { plan_item(&sb, &learned, true, i, &resolve(l, a), obj_of(a), n_actors) });
    }
    let node_plan: Vec<Option<Chosen>> = l.nodes.definitions.iter().enumerate().map(|(i, n)| plan_item(&sb, &learned, false, i, &resolve(l, n), None, n_actors)).collect();
    stats.actors_readable = actor_plan.iter().flatten().count();
    stats.nodes_readable = node_plan.iter().flatten().count();
    for (i, a) in l.actors.definitions.iter().enumerate() {
        if actor_plan[i].is_none() {
            *stats.raw_kinds.entry(format!("actor {}", nodenames::lookup(nodenames::ACTOR_NODE_TYPES, a.type_id).unwrap_or("?"))).or_default() += 1;
        }
    }
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        if node_plan[i].is_none() {
            *stats.raw_kinds.entry(format!("node {}", nodenames::lookup(if game_dpl { nodenames::LOGIC_NODE_TYPES_DPL } else { nodenames::LOGIC_NODE_TYPES }, n.type_id).unwrap_or("?"))).or_default() += 1;
        }
    }
    stats.approximated = actor_plan.iter().flatten().filter(|c| c.approx).count();
    stats.normalized = actor_plan.iter().chain(node_plan.iter()).flatten().filter(|c| !c.exact).count();

    // what the other lines refer to
    let mut ref_a: BTreeSet<usize> = BTreeSet::new();
    let mut ref_n: BTreeSet<usize> = BTreeSet::new();
    for c in actor_plan.iter().chain(node_plan.iter()).flatten() {
        for a in &c.args[..c.shown] {
            if let A::Actor(i) = a {
                ref_a.insert(*i);
            }
        }
    }
    // The wires of a node: those of its wire collection, leaving out wires to nodes that do not exist.
    let wires_of = |i: usize| -> Vec<&WireNode> {
        match l.nodes.definitions[i].properties.first() {
            Some(Property { type_id: 19, value: PropValue::Int(w), .. }) => l.wires.collections.get(*w as usize).map(|c| c.wires.iter().filter(|x| x.node_id >= 0 && (x.node_id as usize) < l.nodes.definitions.len()).collect()).unwrap_or_default(),
            _ => Vec::new(),
        }
    };
    for i in 0..l.nodes.definitions.len() {
        let w = wires_of(i);
        if !w.is_empty() {
            ref_n.insert(i);
            for x in w {
                ref_n.insert(x.node_id as usize);
            }
        }
    }

    // names: readable ones for what is referenced, a table beyond Lua's limit of local variables
    let node_table = if game_dpl { nodenames::LOGIC_NODE_TYPES_DPL } else { nodenames::LOGIC_NODE_TYPES };
    let mut names = Names { local: HashMap::new(), table_a: false, table_n: false };
    let mut used: HashSet<String> = ["MISSION", "RA", "RN", "O"].iter().map(|s| s.to_string()).chain(LUA_WORDS.iter().map(|s| s.to_string())).collect();
    let mut budget = MAX_LOCALS;
    for (actor, set) in [(true, &ref_a), (false, &ref_n)] {
        for &i in set {
            if actor && i >= n_actors {
                continue;
            }
            if budget == 0 {
                if actor { names.table_a = true } else { names.table_n = true }
                continue;
            }
            let (def, kind) = if actor {
                let d = &l.actors.definitions[i];
                (d, nodenames::lookup(nodenames::ACTOR_NODE_TYPES, d.type_id).unwrap_or("actor"))
            } else {
                let d = &l.nodes.definitions[i];
                (d, nodenames::lookup(node_table, d.type_id).unwrap_or("node"))
            };
            let base = ident(&text(l, def.string_id)).unwrap_or_else(|| ident(kind).unwrap_or_else(|| "item".into()));
            let mut name = base.clone();
            if used.contains(&name) || (actor && ident(&text(l, def.string_id)).is_none()) || (!actor && ident(&text(l, def.string_id)).is_none()) {
                name = format!("{base}_{i}");
            }
            while used.contains(&name) {
                name.push('_');
            }
            used.insert(name.clone());
            names.local.insert((actor, i), name);
            budget -= 1;
        }
    }
    let actor_name = |i: usize| names.get(true, i);

    // ---- text
    let mut o = String::new();
    o.push_str(&format!("-- Mission exported by Zartex'27 from {title}\n"));
    o.push_str("-- Written with the mission API: each line creates one actor or logic node (the number in the comment is its\n");
    o.push_str("-- index in the mission), and the wires at the end connect the nodes. Edit the values, add your own lines, or\n");
    o.push_str("-- build more of the mission in code. Lines written MISSION.Raw* hold something the API cannot express.\n");
    o.push_str(&format!("-- {} of {} actors and {} of {} logic nodes are written as API calls.\n", stats.actors_readable, stats.actors, stats.nodes_readable, stats.nodes));
    if stats.approximated > 0 {
        o.push_str(&format!("-- {} actors keep their position and parameters but not the game's internal object bytes or exact angle
-- (the raw export keeps them).
", stats.approximated));
    }
    if stats.normalized > 0 {
        o.push_str("-- Flags, colour alpha and property type ids are the ones the API writes (the game's own files carry others);\n-- use the raw export to keep them.\n");
    }
    o.push('\n');
    o.push_str(&format!("MISSION.RawSoundBanks({{{}}})\n\n", l.sound_banks.items.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")));
    if names.table_a {
        o.push_str("local RA = {}\n");
    }
    if names.table_n {
        o.push_str("local RN = {}\n");
    }

    // objects used by more than one raw actor are created once, in a table
    let mut raw_obj_use: HashMap<i32, usize> = HashMap::new();
    for (i, a) in l.actors.definitions.iter().enumerate() {
        if actor_plan[i].is_none() && obj_of(a).is_some() {
            *raw_obj_use.entry(a.object_id).or_default() += 1;
        }
    }
    if raw_obj_use.values().any(|&n| n > 1) {
        o.push_str("local O = {}\n");
    }
    let mut obj_made: HashSet<i32> = HashSet::new();

    // The start of the line that defines an item: `local name = ` when it has a local name, `RA[i] = ` or `RN[i] = ` when
    // it is referenced, and nothing when no other line refers to it.
    let assign = |actor: bool, i: usize, set: &BTreeSet<usize>| -> String {
        if !set.contains(&i) {
            return String::new();
        }
        match names.local.get(&(actor, i)) {
            Some(n) => format!("local {n} = "),
            None => format!("{} = ", names.get(actor, i)),
        }
    };

    o.push_str("\n-- actors\n");
    for (i, a) in l.actors.definitions.iter().enumerate() {
        let kind = nodenames::lookup(nodenames::ACTOR_NODE_TYPES, a.type_id).unwrap_or("?");
        let note = text(l, a.string_id);
        let asg = assign(true, i, &ref_a);
        match &actor_plan[i] {
            Some(c) => {
                let args: Vec<String> = c.args[..c.shown].iter().map(|x| render(x, c.variant, &actor_name)).collect();
                o.push_str(&format!("{asg}{} -- actor {i}\n", call(learned[c.learned].c.name, &args)));
            }
            None => {
                let objarg = match obj_of(a) {
                    Some(ob) => {
                        if raw_obj_use.get(&a.object_id).copied().unwrap_or(0) > 1 {
                            if obj_made.insert(a.object_id) {
                                o.push_str(&format!("O[{}] = MISSION.RawObject({}) -- object {}\n", a.object_id, object_lua(ob), a.object_id));
                            }
                            format!("O[{}]", a.object_id)
                        } else {
                            format!("MISSION.RawObject({})", object_lua(ob))
                        }
                    }
                    None => a.object_id.to_string(),
                };
                o.push_str(&format!("{asg}MISSION.RawActor({}, {}, {objarg}, {},{},{},{}, {}, {}, {{ -- actor {i}: {kind} (raw)\n", a.type_id, lstr(&note), a.color[0], a.color[1], a.color[2], a.color[3], a.reserved, a.flags));
                for p in &a.properties {
                    o.push_str(&format!("  {},\n", prop_lua(l, p)));
                }
                o.push_str("})\n");
            }
        }
    }
    // objects no actor uses
    let used_objs: HashSet<i32> = l.actors.definitions.iter().filter(|a| obj_of(a).is_some()).map(|a| a.object_id).collect();
    let orphans: Vec<usize> = (0..m.objects.objects.len()).filter(|i| !used_objs.contains(&(*i as i32))).collect();
    if !orphans.is_empty() {
        o.push_str("\n-- objects that no actor uses\n");
        for i in orphans {
            o.push_str(&format!("MISSION.RawObject({}) -- object {i}\n", object_lua(&m.objects.objects[i])));
        }
    }

    o.push_str("\n-- logic nodes\n");
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        let note = text(l, n.string_id);
        let asg = assign(false, i, &ref_n);
        match &node_plan[i] {
            Some(c) => {
                let args: Vec<String> = c.args[..c.shown].iter().map(|x| render(x, c.variant, &actor_name)).collect();
                o.push_str(&format!("{asg}{} -- node {i}\n", call(learned[c.learned].c.name, &args)));
            }
            None => {
                // A node with a wire collection has type 19 as its first property. RawNode creates that property from the flag.
                let has_wire = matches!(n.properties.first(), Some(Property { type_id: 19, value: PropValue::Int(_), .. }));
                o.push_str(&format!("{asg}MISSION.RawNode({}, {}, {},{},{},{}, {}, {}, {has_wire}, {{ -- node {i}: {} (raw)\n", n.type_id, lstr(&note), n.color[0], n.color[1], n.color[2], n.color[3], n.reserved, n.flags, nodenames::lookup(node_table, n.type_id).unwrap_or("?")));
                for p in n.properties.iter().skip(if has_wire { 1 } else { 0 }) {
                    o.push_str(&format!("  {},\n", prop_lua(l, p)));
                }
                o.push_str("})\n");
            }
        }
    }

    o.push_str("\n-- wires\n");
    for i in 0..l.nodes.definitions.len() {
        for x in wires_of(i) {
            let from = names.get(false, i);
            let to = names.get(false, x.node_id as usize);
            if x.wire_type == 1 {
                o.push_str(&format!("{from}.wireCollection.Add({to})\n"));
            } else {
                o.push_str(&format!("{from}.wireCollection.Add({to}, {})\n", x.wire_type));
            }
        }
    }

    if let Some(s) = summary {
        o.push_str("\n-- mission summary (last, so that nothing above changes it)\n");
        // City types 1 to 6 are written as a level name. Other values are written as CityType.
        if (1..=6).contains(&s.city_type) {
            let level = ["", "Miami_Day", "Miami_Night", "Nice_Day", "Nice_Night", "Istanbul_Day", "Istanbul_Night"][s.city_type as usize];
            o.push_str(&format!("MISSION.missionSummary.Level = \"{level}\"\n"));
        } else {
            o.push_str(&format!("MISSION.missionSummary.CityType = {}\n", s.city_type));
        }
        o.push_str(&format!("MISSION.missionSummary.MoodId = {}\n", s.mission_id));
        o.push_str(&format!("MISSION.missionSummary.X = {}\n", ff(s.start_position.x)));
        o.push_str(&format!("MISSION.missionSummary.Y = {}\n", ff(s.start_position.y)));
    }
    (o, stats)
}
