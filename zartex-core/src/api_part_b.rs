//! Lua API port, part b: methods of LuaMissionScript for actor setup, node lookup and character moves.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class and its compiler).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! Exposes: `register`, which adds the methods to the LuaMissionScript table; the userdata types ActorSettingH and
//! ActorSetupH; and the functions toflag, get_actor_by_id, get_node_by_id, get_logic_start, find_player_actor,
//! set_actor_uid, set_actor_object_position_xyza, create_actor_setting, set_chase_vehicle_1 and set_chase_vehicle_2.
//! Covers: toflag, CreateActorSetting, CreateWireCollection, AddSoundBankToLoadList, GetActorById, SetActorUID,
//! SetActorObjectPositionXYZA, FindPlayerActor, GetLogicStart, GetNodeById, ImportMissionScriptFromFile, the two
//! overloads of SetChaseVehicle (chosen by the type of Lua argument 4), and SetCharacterPositionTo and
//! SetCharacterForceGetOutVehicleTo (both through character_move, which chooses between two argument layouts).
//! Uses: api_manual (nre, reg), binio (Vec3), logic (PropValue), luaapi (Args, WiresH, canon, ret_actor, ret_node,
//! rt_err), mission (MissionFile, to read a mission file), objects (MissionObject), script (ScriptState, node_color,
//! cast_int, forward_from_radians).
//! Used by: api_manual::register_manual calls `register`. The integration test tests/part_b.rs calls toflag,
//! get_actor_by_id, get_node_by_id, get_logic_start, find_player_actor and the character move methods.
use crate::api_manual::{nre, reg};
use crate::binio::*;
use crate::logic::PropValue;
use crate::luaapi::*;
use crate::mission::MissionFile;
use crate::objects::MissionObject;
use crate::script::*;
use mlua::{Lua, MetaMethod, MultiValue, Result as LuaResult, Table, UserData, UserDataMethods, Value};
use std::rc::Rc;

/// Message of the .NET indexer error for an index outside a collection; used by the lookups in this file.
const INDEX_RANGE: &str = "Index was out of range. Must be non-negative and less than the size of the collection.";

/// True when the value is a userdata of type T.
fn is_ud<T: 'static>(v: &Value) -> bool {
    matches!(v, Value::UserData(u) if u.is::<T>())
}

/// True when Lua argument `i` is a boolean.
fn is_bool(a: &Args, i: usize) -> bool {
    matches!(a.v.get(i), Some(Value::Boolean(_)))
}

// ---------------------------------------------------------------- toflag and lookups by id
/// `int toflag(byte, byte, byte, byte)`: BitConverter.ToInt32 (little endian)
pub fn toflag(a: u8, b: u8, c: u8, d: u8) -> i32 {
    i32::from_le_bytes([a, b, c, d])
}

/// Index check used by GetActorById / GetNodeById: `id` is 1-based, returns the 0-based index.
fn check_id(id: i32, count: usize) -> LuaResult<usize> {
    let max = count as i32 - 1;
    if id.wrapping_sub(1) > max {
        return Err(rt_err("Bad argument #1 - Index ID can't be bigger than the size of the array"));
    }
    if id.wrapping_sub(1) < 0 {
        return Err(rt_err("Bad argument #1 - Index ID can't be negative or zero"));
    }
    Ok((id - 1) as usize)
}

/// GetActorById: checks the 1-based `id` against the actor count and returns the 0-based actor index.
pub fn get_actor_by_id(st: &ScriptState, id: i32) -> LuaResult<usize> {
    check_id(id, st.logic.actors.definitions.len())
}

/// GetNodeById: returns (node index, wire collection index taken from the node's first property).
pub fn get_node_by_id(st: &ScriptState, id: i32) -> LuaResult<(usize, usize)> {
    let idx = check_id(id, st.logic.nodes.definitions.len())?;
    let wire_id = match st.logic.nodes.definitions[idx].properties.first() {
        None => return Err(rt_err(INDEX_RANGE)),
        Some(p) => match p.value {
            PropValue::Int(v) => v,
            _ => return Err(rt_err("Unable to cast object to type 'IntegerProperty'.")),
        },
    };
    if wire_id < 0 || wire_id as usize >= st.wires.len() {
        return Err(rt_err(INDEX_RANGE));
    }
    Ok((idx, wire_id as usize))
}

/// GetLogicStart: first node of type 1 (LogicStart), or None (null).
pub fn get_logic_start(st: &ScriptState) -> LuaResult<Option<(usize, usize)>> {
    match st.logic.nodes.definitions.iter().position(|n| n.type_id == 1) {
        Some(id) => get_node_by_id(st, id as i32 + 1).map(Some),
        None => Ok(None),
    }
}

/// FindPlayerActor: first character whose first property (pRole) is 1.
pub fn find_player_actor(st: &ScriptState) -> LuaResult<Option<usize>> {
    for (id, actor) in st.logic.actors.definitions.iter().enumerate() {
        if actor.type_id != 2 || actor.properties.is_empty() {
            continue;
        }
        let p = &actor.properties[0];
        match p.value {
            PropValue::Int(v) if p.type_id == 1 => {
                if v == 1 {
                    return Ok(Some(id));
                }
            }
            _ => return Err(nre()),
        }
    }
    Ok(None)
}

/// Object index of an actor (`missionData.Objects.Objects[actor.TheActor.ObjectId]`).
fn object_index(st: &ScriptState, actor: usize) -> LuaResult<usize> {
    let object_id = st.logic.actors.definitions[actor].object_id;
    if object_id < 0 || object_id as usize >= st.objects.len() {
        return Err(rt_err(INDEX_RANGE));
    }
    Ok(object_id as usize)
}

/// SetActorUID: Character (actor type 2) and Vehicle (type 3) only.
pub fn set_actor_uid(st: &mut ScriptState, actor: Option<usize>, uid: i32) -> LuaResult<()> {
    let actor = actor.ok_or_else(nre)?;
    let type_id = st.logic.actors.definitions[actor].type_id;
    let oi = object_index(st, actor)?;
    match type_id {
        2 => match &mut st.objects[oi] {
            MissionObject::Character { uid: u, .. } => *u = uid,
            _ => return Err(nre()),
        },
        3 => match &mut st.objects[oi] {
            MissionObject::Vehicle { uid: u, .. } => *u = uid,
            _ => return Err(nre()),
        },
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------- creation data of actors
/// .NET MemoryStream growth: a write past the end grows the buffer to max(needed, 256, 2 * capacity).
fn grow(buf: &mut Vec<u8>, need: usize) {
    if need > buf.len() {
        let mut nc = need.max(256);
        if nc < buf.len() * 2 {
            nc = buf.len() * 2;
        }
        buf.resize(nc, 0);
    }
}

/// Little-endian bytes of three f32 values (12 bytes).
fn vec3_bytes(v: [f32; 3]) -> [u8; 12] {
    let mut out = [0u8; 12];
    for (i, f) in v.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&f.to_le_bytes());
    }
    out
}

/// Creation data after `stream.Write(data); Position = 0x4; Write(pos); Position = 0x10; Write(fwd)`.
fn write_creation_data(old: &[u8], pos: [f32; 3], fwd: [f32; 3]) -> Vec<u8> {
    let mut buf = old.to_vec();
    grow(&mut buf, 0x4 + 12);
    buf[0x4..0x10].copy_from_slice(&vec3_bytes(pos));
    grow(&mut buf, 0x10 + 12);
    buf[0x10..0x1C].copy_from_slice(&vec3_bytes(fwd));
    buf
}

/// SetActorObjectPositionXYZA (switch on the actor type).
pub fn set_actor_object_position_xyza(st: &mut ScriptState, actor: Option<usize>, x: f32, y: f32, z: f32, angle: f32) -> LuaResult<()> {
    let actor = actor.ok_or_else(nre)?;
    let type_id = st.logic.actors.definitions[actor].type_id;
    let oi = object_index(st, actor)?;
    let pos = Vec3 { x, y, z };
    match type_id {
        2 => {
            let a = (std::f64::consts::PI / 180.0) * angle as f64;
            let fwd = forward_from_radians(a);
            match &mut st.objects[oi] {
                MissionObject::Character { creation_data, position, .. } => {
                    *position = pos;
                    let new_data = write_creation_data(creation_data.as_slice(), [x, y, z], fwd);
                    *creation_data = new_data;
                }
                _ => return Err(nre()),
            }
        }
        3 => match &mut st.objects[oi] {
            MissionObject::Vehicle { position, .. } => *position = pos,
            _ => return Err(nre()),
        },
        4 => match &mut st.objects[oi] {
            MissionObject::Volume { position, .. } => *position = pos,
            _ => return Err(nre()),
        },
        5 => match &mut st.objects[oi] {
            MissionObject::ObjectiveIcon { position, .. } => *position = pos,
            _ => return Err(nre()),
        },
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------- actor setting and import
/// CreateActorSetting: each key of the table is a 1-based actor index (the C# reads `key.Number`).
/// Returns the 0-based indexes. Fails when a key is not a number or is outside the actor list.
pub fn create_actor_setting(st: &ScriptState, set: &Table) -> LuaResult<Vec<i32>> {
    let mut sets = Vec::new();
    for pair in set.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let number = match key {
            Value::Integer(i) => Some(i as f64),
            Value::Number(n) => Some(n),
            _ => None,
        };
        let index = match number {
            Some(n) => cast_int(n).wrapping_sub(1),
            None => -1,
        };
        if index < 0 || index as usize >= st.logic.actors.definitions.len() {
            return Err(rt_err(INDEX_RANGE));
        }
        sets.push(index);
    }
    Ok(sets)
}

/// ImportMissionScriptFromFile body (appendSpooler = false). Errors come back as the string the C#
/// returns (the C# suffix "\nLine: N\nFile: F" cannot be reproduced).
fn import_file(st: &mut ScriptState, path: &str, append_spooler: bool) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = MissionFile::from_bytes(&bytes).map_err(|e| e.to_string())?;
    if append_spooler {
        // C# copies the spooler here; the ExportedMission model of this port has no spooler.
        return Ok(());
    }
    let m = &ext.mission;
    st.logic.counters.items = m.logic.counters.items.clone();
    st.logic.nodes.definitions = m.logic.nodes.definitions.clone();
    st.logic.actors.definitions = m.logic.actors.definitions.clone();
    st.objects = m.objects.objects.clone();
    st.logic.strings.strings = m.logic.strings.strings.clone();
    st.logic.sound_banks.items = m.logic.sound_banks.items.clone();
    let s = ext.summary.as_ref().ok_or("Object reference not set to an instance of an object.")?;
    st.summary.city_type = s.city_type;
    st.summary.start_position = s.start_position;
    st.summary.mood_id = s.mission_id;
    st.summary.locale_id = s.mission_locale_id;
    // PingInRadius / PingOutRadius: ScriptSummary has no such fields, so they are not kept.
    st.wires = m.logic.wires.collections.iter().map(|c| c.wires.clone()).collect();
    Ok(())
}

// ---------------------------------------------------------------- node / actor creators

/// Marker actor (type 105) used by the character move helpers, with the C# defaults: note "", r 100, g 128, b 255.
/// Pushes a Marker object running from (x1, y1, z1) to (x2, y2, z2). Returns the actor index.
fn marker(st: &mut ScriptState, x1: f32, y1: f32, z1: f32, x2: f32, y2: f32, z2: f32) -> usize {
    let color = node_color(100.0, 128.0, 255.0, 255.0);
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(105, None, color, 0x72D5, object_id, |_| Vec::new());
    st.objects.push(MissionObject::Marker {
        v1: Vec3 { x: x2, y: y2, z: z2 },
        position: Vec3 { x: x1, y: y1, z: z1 },
    });
    idx
}

/// Forward used for the marker of SetCharacterPositionTo / SetCharacterForceGetOutVehicleTo:
/// the C# builds Vector3(0, cos a, sin a) (note the x component is 0 here).
fn marker_forward(angle: f32) -> [f32; 3] {
    let a = (std::f64::consts::PI / 180.0) * angle as f64;
    [0.0, (1.0f64 * a.xcos()) as f32, (1.0f64 * a.xsin()) as f32]
}

/// CharacterControl node (type 18) for a Character actor, used by character_move. The node has the same
/// properties as api_part_c::character_control; this copy is kept private to this file.
#[allow(clippy::too_many_arguments)]
fn character_control(
    st: &mut ScriptState,
    character: Option<usize>,
    property: i32,
    value: f32,
    weapon: i32,
    personality_index: i32,
    vehicle: Option<usize>,
    target: Option<usize>,
    animation: &str,
    anim_action: i16,
    pad_file_name: &str,
    num_pad_recordings: i32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> LuaResult<(usize, usize)> {
    let character = character.ok_or_else(|| rt_err("Bad argument #1 - Attempt to control a nil value"))?;
    if st.logic.actors.definitions[character].type_id != 2 {
        return Err(rt_err("Bad argument #1 - The inputed actor is not a Character actor type"));
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let character = character as i32;
    let vehicle = vehicle.map_or(-1, |v| v as i32);
    let target = target.map_or(-1, |v| v as i32);
    Ok(st.create_node(18, note, color, 0, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(character)),
            st.prop("pVehicle", 7, PropValue::Int(vehicle)),
            st.prop("pTarget", 7, PropValue::Int(target)),
            st.prop("pPersonalityIndex", 1, PropValue::Int(personality_index)),
            st.prop("pProperty", 6, PropValue::Int(property)),
            st.prop("pWeapon", 6, PropValue::Int(weapon)),
            st.prop("pValue", 2, PropValue::Float(value)),
            {
                let sid = st.string_id(animation);
                st.prop("pAnimation", 8, PropValue::TextFile { value: sid, index: anim_action })
            },
            st.prop("pFlags", 9, PropValue::Int(flags)),
            st.prop("pNumPadRecordings", 1, PropValue::Int(num_pad_recordings)),
            {
                let sid = st.string_id(pad_file_name);
                st.prop("pPadFileName", 3, PropValue::Str(sid))
            },
        ]
    }))
}

/// SetChaseVehicle overload 1: (vehicle, health, textId, minForce, maxForce, bulletDamage, invicibleTyres, note, r, g, b).
/// The vehicle argument is not stored by this overload (C# behaviour), so it is not a parameter here.
/// Returns (node index, wire collection index).
#[allow(clippy::too_many_arguments)]
pub fn set_chase_vehicle_1(
    st: &mut ScriptState,
    health: f32,
    text_id: i32,
    min_force: f32,
    max_force: f32,
    bullet_damage: f32,
    invincible_tyres: bool,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> (usize, usize) {
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    st.create_node(137, note, color, 0, |st| {
        // the C# also adds the wire collection index as an ActorProperty, named "pWireCollection"
        // create_node has just added this node's wire collection, so it is the last one in the list
        let wire = (st.wires.len() - 1) as i32;
        vec![
            st.prop("pWireCollection", 7, PropValue::Int(wire)),
            st.prop("pMinForce", 2, PropValue::Float(min_force)),
            st.prop("pMaxForce", 2, PropValue::Float(max_force)),
            st.prop("pHealth", 2, PropValue::Float(health)),
            st.prop("pBulletDamage", 2, PropValue::Float(bullet_damage)),
            st.prop("pText", 20, PropValue::Int(text_id)),
            st.prop("pInvicibleTyres", 4, PropValue::Bool(invincible_tyres)),
        ]
    })
}

/// SetChaseVehicle overload 2: (vehicle, health, text, invicibleTyres, bulletDamage, minForce, maxForce, note, r, g, b).
#[allow(clippy::too_many_arguments)]
pub fn set_chase_vehicle_2(
    st: &mut ScriptState,
    vehicle: Option<usize>,
    health: f32,
    text_id: i32,
    invincible_tyres: bool,
    _bullet_damage: f32, // not written by this overload in the C#
    min_force: f32,
    max_force: f32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> (usize, usize) {
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let vehicle = vehicle.map_or(-1, |v| v as i32);
    st.create_node(137, note, color, 0, |st| {
        vec![
            st.prop("pVehicle", 7, PropValue::Int(vehicle)),
            st.prop("pMinForce", 2, PropValue::Float(min_force)),
            st.prop("pMaxForce", 2, PropValue::Float(max_force)),
            st.prop("pHealth", 2, PropValue::Float(health)),
            st.prop("pText", 20, PropValue::Int(text_id)),
            // the C# names this BooleanProperty "pText" (copied literally)
            st.prop("pText", 4, PropValue::Bool(invincible_tyres)),
        ]
    })
}

/// SetCharacterPositionTo (kind 18) and SetCharacterForceGetOutVehicleTo (kind 17).
/// Overload A (character, x, y, z, angle, vehicle, flags, note, r, g, b) when argument 2 is a number;
/// overload B (character, actor, vehicle, flags, note, r, g, b) otherwise.
fn character_move(lua: &Lua, st: &State, a: &Args, kind: i32) -> LuaResult<Value> {
    if a.is_number(1) {
        let character = a.actor(0)?;
        let x = a.float(1, None)?;
        let y = a.float(2, None)?;
        let z = a.float(3, None)?;
        let angle = a.float(4, Some(0.0))?;
        let vehicle = a.actor(5)?;
        let flags = a.int(6, Some(0))?;
        let note = a.string(7, Some(Some("Character.SetPositionTo(float,float,float,float)")))?;
        let r = a.int(8, Some(200))?;
        let g = a.int(9, Some(255))?;
        let b = a.int(10, Some(100))?;
        let (idx, wire) = {
            let mut s = st.borrow_mut();
            let fwd = marker_forward(angle);
            let m = marker(&mut s, x, y, z, fwd[0], fwd[1], fwd[2]);
            character_control(&mut s, character, kind, 0.0, 0, -1, vehicle, Some(m), "", -1, "", 0, flags, note.as_deref(), r, g, b)?
        };
        ret_node(lua, st, idx, wire)
    } else {
        let character = a.actor(0)?;
        let actor = a.actor(1)?;
        let vehicle = a.actor(2)?;
        let flags = a.int(3, Some(0))?;
        let note = a.string(4, Some(Some("Character.SetPositionTo(Actor)")))?;
        let r = a.int(5, Some(200))?;
        let g = a.int(6, Some(255))?;
        let b = a.int(7, Some(100))?;
        let (idx, wire) = {
            let mut s = st.borrow_mut();
            character_control(&mut s, character, kind, 0.0, 0, -1, vehicle, actor, "", -1, "", 0, flags, note.as_deref(), r, g, b)?
        };
        ret_node(lua, st, idx, wire)
    }
}

// ---------------------------------------------------------------- userdata: ActorSetting / ActorSetup

// ---------------------------------------------------------------- userdata: ActorSetting / ActorSetup
/// ActorSetting (Lua userdata): 0-based actor indexes plus the `index` field set by ActorSetup.Add.
pub struct ActorSettingH {
    pub sets: Vec<i32>,
    pub index: i32,
}

impl UserData for ActorSettingH {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |_, this, key: String| -> LuaResult<Value> {
            Ok(match canon(&key).as_str() {
                "Index" => Value::Integer(this.index as i64),
                _ => Value::Nil,
            })
        });
    }
}

/// MISSION.actorSetup (ActorSetup): Add(setting) appends the set to the actor setup table.
pub struct ActorSetupH {
    pub st: State,
}

impl UserData for ActorSetupH {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| -> LuaResult<Value> {
            if canon(&key) != "Add" {
                return Ok(Value::Nil);
            }
            let st = this.st.clone();
            Ok(Value::Function(lua.create_function(move |_, args: MultiValue| {
                let a = Args::new(args, is_ud::<ActorSetupH>);
                let setting = match a.v.first() {
                    None | Some(Value::Nil) => return Err(nre()),
                    Some(Value::UserData(u)) => u.clone(),
                    Some(_) => return Err(rt_err("cannot convert value to clr type Zartex.ActorSetting")),
                };
                let mut sd = setting
                    .borrow_mut::<ActorSettingH>()
                    .map_err(|_| rt_err("cannot convert a userdata to a clr type Zartex.ActorSetting"))?;
                let mut s = st.borrow_mut();
                sd.index = s.actor_setup.len() as i32;
                s.actor_setup.push(sd.sets.clone());
                Ok(())
            })?))
        });
    }
}

// ---------------------------------------------------------------- registration

/// Adds the methods of this file to the Lua table `t`. The `dpl` argument is not used: the same methods
/// are registered for both classes.
pub fn register(lua: &Lua, t: &Table, st: &State, _dpl: bool) -> LuaResult<()> {
    reg(lua, t, st, "toflag", |_, _, a| {
        Ok(Value::Integer(toflag(a.byte(0, None)?, a.byte(1, None)?, a.byte(2, None)?, a.byte(3, None)?) as i64))
    })?;

    // Registered by hand: reg() would drop a leading table argument as `self`.
    let mission = t.clone();
    let st_c = Rc::clone(st);
    t.set(
        "CreateActorSetting",
        lua.create_function(move |lua, args: mlua::MultiValue| {
            let a = Args::new(args, |v| matches!(v, Value::Table(x) if *x == mission));
            let set = a.table(0)?;
            let sets = create_actor_setting(&st_c.borrow(), &set)?;
            Ok(Value::UserData(lua.create_userdata(ActorSettingH { sets, index: 0 })?))
        })?,
    )?;
    t.set("actorSetup", lua.create_userdata(ActorSetupH { st: Rc::clone(st) })?)?;

    reg(lua, t, st, "CreateWireCollection", |lua, st, a| {
        let n = a.int(0, Some(0))?;
        if n < 0 {
            return Err(rt_err("Non-negative number required."));
        }
        let wire = st.borrow_mut().new_wire_collection();
        Ok(Value::UserData(lua.create_userdata(WiresH { st: st.clone(), wire })?))
    })?;

    reg(lua, t, st, "AddSoundBankToLoadList", |_, st, a| {
        let bank = a.int(0, None)?;
        st.borrow_mut().logic.sound_banks.items.push(bank);
        Ok(Value::Nil)
    })?;

    reg(lua, t, st, "GetActorById", |lua, st, a| {
        let id = a.int(0, None)?;
        let idx = get_actor_by_id(&st.borrow(), id)?;
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "SetActorUID", |_, st, a| {
        let actor = a.actor(0)?;
        let uid = a.int(1, None)?;
        set_actor_uid(&mut st.borrow_mut(), actor, uid)?;
        Ok(Value::Nil)
    })?;

    reg(lua, t, st, "SetActorObjectPositionXYZA", |_, st, a| {
        let actor = a.actor(0)?;
        let x = a.float(1, None)?;
        let y = a.float(2, None)?;
        let z = a.float(3, None)?;
        let angle = a.float(4, None)?;
        set_actor_object_position_xyza(&mut st.borrow_mut(), actor, x, y, z, angle)?;
        Ok(Value::Nil)
    })?;

    reg(lua, t, st, "FindPlayerActor", |lua, st, _a| match find_player_actor(&st.borrow())? {
        Some(idx) => ret_actor(lua, st, idx),
        None => Ok(Value::Nil),
    })?;

    reg(lua, t, st, "GetLogicStart", |lua, st, _a| match get_logic_start(&st.borrow())? {
        Some((idx, wire)) => ret_node(lua, st, idx, wire),
        None => Ok(Value::Nil),
    })?;

    reg(lua, t, st, "GetNodeById", |lua, st, a| {
        let id = a.int(0, None)?;
        let (idx, wire) = get_node_by_id(&st.borrow(), id)?;
        ret_node(lua, st, idx, wire)
    })?;

    reg(lua, t, st, "ImportMissionScriptFromFile", |lua, st, a| {
        let filename = a.string(0, None)?;
        let append_spooler = a.boolean(1, Some(false))?;
        if filename.as_deref() == Some("") {
            return Err(rt_err("Bad argument #1 - Literally empty string detected"));
        }
        let working_dir = st.borrow().working_dir.clone();
        let path = format!("{}\\{}", working_dir, filename.unwrap_or_default());
        let p = std::path::Path::new(&path);
        if !p.is_file() {
            let full = std::path::absolute(p).map(|f| f.display().to_string()).unwrap_or_else(|_| path.clone());
            return Err(rt_err(format!("Bad argument #1 - The file does not exist - {full}")));
        }
        match import_file(&mut st.borrow_mut(), &path, append_spooler) {
            Ok(()) => Ok(Value::Boolean(true)),
            Err(msg) => Ok(Value::String(lua.create_string(&msg)?)),
        }
    })?;

    reg(lua, t, st, "SetChaseVehicle", |lua, st, a| {
        let vehicle = a.actor(0)?;
        let health = a.float(1, None)?;
        let text_id = a.int(2, None)?;
        let (idx, wire) = if !a.is_number(3) {
            // overload 2 (measured against MoonSharp: it wins whenever argument 4 is not a number, incl. absent)
            let invincible = a.boolean(3, Some(true))?;
            let bullet = a.float(4, Some(0.0875))?;
            let min_force = a.float(5, Some(3000.0))?;
            let max_force = a.float(6, Some(25000.0))?;
            let note = a.string(7, Some(Some("")))?;
            let r = a.int(8, Some(155))?;
            let g = a.int(9, Some(0))?;
            let b = a.int(10, Some(255))?;
            set_chase_vehicle_2(
                &mut st.borrow_mut(),
                vehicle,
                health,
                text_id,
                invincible,
                bullet,
                min_force,
                max_force,
                note.as_deref(),
                r,
                g,
                b,
            )
        } else {
            // overload 1: argument 4 is the minForce number (or absent: first overload in source order)
            let min_force = a.float(3, Some(1700.0))?;
            let max_force = a.float(4, Some(14000.0))?;
            let bullet = a.float(5, Some(0.085))?;
            let invincible = a.boolean(6, Some(true))?;
            let note = a.string(7, Some(Some("")))?;
            let r = a.int(8, Some(100))?;
            let g = a.int(9, Some(0))?;
            let b = a.int(10, Some(222))?;
            set_chase_vehicle_1(
                &mut st.borrow_mut(),
                health,
                text_id,
                min_force,
                max_force,
                bullet,
                invincible,
                note.as_deref(),
                r,
                g,
                b,
            )
        };
        ret_node(lua, st, idx, wire)
    })?;

    reg(lua, t, st, "SetCharacterForceGetOutVehicleTo", |lua, st, a| character_move(lua, st, a, 17))?;
    reg(lua, t, st, "SetCharacterPositionTo", |lua, st, a| character_move(lua, st, a, 18))?;
    Ok(())
}
