//! Lua API port, part a: actor creators of LuaMissionScript.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class and its compiler).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! Exposes: `register`, which adds the actor creators to the LuaMissionScript table, and the Rust functions behind
//! them: create_ai_path, test_volume, marker, create_area, get_animated_object_at, create_switch, ai_target,
//! special_effect, create_objective_icon, create_objective_icon_attached_to, create_vehicle, create_collectable,
//! create_character_in_vehicle, create_camera and create_camera_looking_at_position.
//! Each creator calls ScriptState::create_actor (note, colour, flags and the actor properties), then pushes its
//! mission object onto the object list, then returns the index of the new actor. The Lua wrapper turns that index
//! into the returned actor value with ret_actor.
//! Uses: api_manual (reg, nre, create_character), binio (Vec3, Vec4), logic (PropValue), luaapi (argument
//! reader Args, rt_err, ret_actor), objects (MissionObject), script (ScriptState, node_color, forward_from_radians).
//! Used by: api_manual::register_manual calls `register`. The integration test tests/part_a.rs runs these creators from Lua.

use crate::api_manual::{create_character, nre, reg};
use crate::binio::*;
use crate::logic::PropValue;
use crate::luaapi::*;
use crate::objects::MissionObject;
use crate::script::*;
use mlua::{Lua, Result as LuaResult, Table, Value};

/// ActorDefinition.Flags used by most creators (0x72D5)
const FLAGS_STD: i16 = 0x72D5;
/// ActorDefinition.Flags used by the creators that set 0x2FD7
const FLAGS_ALT: i16 = 0x2FD7;

/// Node colour from the R, G and B arguments, with alpha 255.
fn color(r: i32, g: i32, b: i32) -> [u8; 4] {
    // new NodeColor(r, g, b, 255) with an int `b` binds the double overload
    node_color(r as f64, g as f64, b as f64, 255.0)
}

/// `(float)x` on a Lua number. Non-numbers: C# `Number` is null and the cast throws.
fn lua_number(v: &Value) -> LuaResult<f64> {
    match v {
        Value::Integer(i) => Ok(*i as f64),
        Value::Number(n) => Ok(*n),
        _ => Err(rt_err("Nullable object must have a value.")),
    }
}

/// `attachedTo == null ? -1 : attachedTo.index`
fn actor_or_neg(a: Option<usize>) -> i32 {
    a.map(|x| x as i32).unwrap_or(-1)
}

/// Vec3 from three components.
fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

// ---------------------------------------------------------------- CreateAIPath

/// CreateAIPath: creates a path actor (type 6) for a Character actor (type 2) and a Path object.
/// `path_table` is a Lua array of {x, y, z} tables. Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn create_ai_path(
    st: &mut ScriptState,
    character: Option<usize>,
    path_table: Option<&Table>,
    speed: f32,
    desired_transport: i32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> LuaResult<usize> {
    let character = character.ok_or_else(|| rt_err("Bad argument #1 - Attempt to create AI path for a nil value"))?;
    if st.logic.actors.definitions[character].type_id != 2 {
        return Err(rt_err("Bad argument #1 - The inputed actor is not a Character actor type"));
    }
    let table = path_table.ok_or_else(nre)?;
    let len = table.raw_len() as usize;
    if len == 0 {
        return Err(rt_err("Bad argument #2 - Path Vector3 table can't be empty!"));
    }
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(6, note, color(r, g, b), FLAGS_STD, object_id, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(character as i32)),
            st.prop("pSpeed", 2, PropValue::Float(speed)),
            st.prop("pDesiredTransport", 1, PropValue::Int(desired_transport)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    // Vector4[] path = new Vector4[Length]; for pathId in 0..=Length (inclusive, like the C# loop)
    let mut path = vec![Vec4::default(); len];
    for path_id in 0..=len {
        let v: Value = table.raw_get(path_id as i64)?;
        match v {
            Value::Nil => {}
            Value::Table(vt) => {
                let x: Value = vt.raw_get(1i64)?;
                let y: Value = vt.raw_get(2i64)?;
                let z: Value = vt.raw_get(3i64)?;
                if matches!(x, Value::Nil) || matches!(y, Value::Nil) || matches!(z, Value::Nil) {
                    return Err(rt_err(format!(
                        "Bad argument #2 - Path table index {path_id} has some invalid Vector3 values (nil)"
                    )));
                }
                let vec = Vec4 {
                    x: lua_number(&x)? as f32,
                    y: lua_number(&y)? as f32,
                    z: lua_number(&z)? as f32,
                    w: 1.0,
                };
                if path_id == 0 {
                    // path[pathId - 1] with pathId 0 is an IndexOutOfRangeException in C#
                    return Err(rt_err("Index was outside the bounds of the array."));
                }
                path[path_id - 1] = vec;
            }
            _ => {
                return Err(rt_err(
                    "Bad argument #2 - Path table must be a table containing tables in a Vector3 format",
                ))
            }
        }
    }
    // CreationData = { 1, 0 x15, 0 x12, 0, 0, 0x3F, 0x80 }
    let mut cd = vec![1u8];
    cd.extend_from_slice(&[0u8; 15]);
    cd.extend_from_slice(&[0u8; 12]);
    cd.extend_from_slice(&[0x00, 0x00, 0x3F, 0x80]);
    st.objects.push(MissionObject::Path { creation_data: cd, count: 0, path });
    Ok(idx)
}

// ---------------------------------------------------------------- TestVolume

/// TestVolume: creates a volume actor (type 4), optionally attached to an actor (-1 when none), and a
/// Volume object at (x, y, z). Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn test_volume(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    attached_to: Option<usize>,
    radius: f32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(4, note, color(r, g, b), FLAGS_STD, object_id, |st| {
        vec![
            st.prop("pAttachTo", 7, PropValue::Int(actor_or_neg(attached_to))),
            st.prop("pRadius", 2, PropValue::Float(radius)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    st.objects.push(MissionObject::Volume { reserved: 0, position: v3(x, y, z) });
    idx
}

// ---------------------------------------------------------------- Marker

/// Marker: creates a marker actor (type 105) and a Marker object running from (x1, y1, z1) to (x2, y2, z2).
/// Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn marker(
    st: &mut ScriptState,
    x1: f32,
    y1: f32,
    z1: f32,
    x2: f32,
    y2: f32,
    z2: f32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(105, note, color(r, g, b), FLAGS_STD, object_id, |_| Vec::new());
    st.objects.push(MissionObject::Marker { v1: v3(x2, y2, z2), position: v3(x1, y1, z1) });
    idx
}

// ---------------------------------------------------------------- CreateArea

/// CreateArea: creates a volume actor (type 4) and an Area object at (x, y, z) with the given scale.
/// Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn create_area(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    scale_x: f32,
    scale_y: f32,
    scale_z: f32,
    attached_to: Option<usize>,
    radius: f32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(4, note, color(r, g, b), FLAGS_STD, object_id, |st| {
        vec![
            st.prop("pAttachTo", 7, PropValue::Int(actor_or_neg(attached_to))),
            st.prop("pRadius", 2, PropValue::Float(radius)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    st.objects.push(MissionObject::Area {
        data: [1, 1, 0, 0],
        scale: Vec4 { x: scale_x, y: scale_y, z: scale_z, w: 0.0 },
        position: Vec4 { x, y, z, w: 1.0 },
        data5: 1,
        v1: v3(0.0, 0.0, 6.7860),
    });
    idx
}

// ---------------------------------------------------------------- GetAnimatedObjectAt

/// GetAnimatedObjectAt: creates an animated-object actor (type 104) with property pType and an AnimProp
/// object at (x, y, z). Returns the index of the new actor.
pub fn get_animated_object_at(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    ty: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(104, note, color(r, g, b), FLAGS_STD, object_id, |st| {
        vec![st.prop("pType", 6, PropValue::Int(ty))]
    });
    st.objects.push(MissionObject::AnimProp { position: v3(x, y, z) });
    idx
}

// ---------------------------------------------------------------- CreateSwitch

/// CreateSwitch: creates a switch actor (type 101) and a Switch object at (x, y, z). `angle` is in degrees
/// and gives the switch direction on the horizontal plane. Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn create_switch(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    ty: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let a = (std::f64::consts::PI / 180.0) * angle as f64;
    let f = forward_from_radians(a);
    // new Vector4(fwd x, 0, fwd z, 0.0333f)
    let fwd = Vec4 { x: f[0], y: f[1], z: f[2], w: 0.0333 };
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(101, note, color(r, g, b), FLAGS_STD, object_id, |st| {
        vec![st.prop("pType", 6, PropValue::Int(ty))]
    });
    st.objects.push(MissionObject::Switch { position: v3(x, y, z), direction: fwd, uid: 0 });
    idx
}

// ---------------------------------------------------------------- AITarget

/// AITarget: creates an AI target actor (type 7) for a Character actor (type 2) and a Volume object at
/// (x, y, z). Returns an error when the actor is nil or is not a Character. Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn ai_target(
    st: &mut ScriptState,
    character: Option<usize>,
    x: f32,
    y: f32,
    z: f32,
    walk_speed: f32,
    drive_speed: f32,
    desired_transport: i32,
    attach_to: Option<usize>,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> LuaResult<usize> {
    let character = character.ok_or_else(nre)?;
    if st.logic.actors.definitions[character].type_id != 2 {
        return Err(rt_err("Inputed actor is not a Character type actor"));
    }
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(7, note, color(r, g, b), 0, object_id, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(character as i32)),
            st.prop("pAttachTo", 7, PropValue::Int(actor_or_neg(attach_to))),
            st.prop("pWalkSpeed", 2, PropValue::Float(walk_speed)),
            st.prop("pVehicleSpeed", 2, PropValue::Float(drive_speed)),
            st.prop("pDesiredTransport", 6, PropValue::Int(desired_transport)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    st.objects.push(MissionObject::Volume { reserved: 0, position: v3(x, y, z) });
    Ok(idx)
}

// ---------------------------------------------------------------- SpecialEffect

/// SpecialEffect: creates a special effect actor (type 8) and an Area object at (x, y, z) whose scale is
/// `range`. Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn special_effect(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    ty: i32,
    force: f32,
    range: f32,
    time: i32,
    attach_to: Option<usize>,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(8, note, color(r, g, b), 0, object_id, |st| {
        vec![
            st.prop("pType", 6, PropValue::Int(ty)),
            st.prop("pAttachTo", 7, PropValue::Int(actor_or_neg(attach_to))),
            st.prop("pForce", 2, PropValue::Float(force)),
            st.prop("pRange", 2, PropValue::Float(range)),
            st.prop("pTime", 1, PropValue::Int(time)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    st.objects.push(MissionObject::Area {
        data: [1, 2, 0, 0],
        scale: Vec4 { x: range, y: range, z: range, w: 0.0 },
        position: Vec4 { x, y, z, w: 1.0 },
        data5: 0,
        v1: v3(0.0, 0.0, 1.0),
    });
    idx
}

// ---------------------------------------------------------------- CreateObjectiveIcon

/// CreateObjectiveIcon: creates an objective icon actor (type 5, not attached) and an ObjectiveIcon object at
/// (x, y, z). `can_go_up` sets flag 131072 instead of 1. Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn create_objective_icon(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    height: i32,
    max_adaptive_height: i32,
    ty: i32,
    can_go_up: bool,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(5, note, color(r, g, b), 0, object_id, |st| {
        vec![
            st.prop("pAttachTo", 7, PropValue::Int(-1)),
            st.prop("pHeight", 1, PropValue::Int(height)),
            st.prop("pMaxAdpativeHeight", 1, PropValue::Int(max_adaptive_height)),
            st.prop("pFlags", 9, PropValue::Int(if can_go_up { 131072 } else { 1 })),
        ]
    });
    st.objects.push(MissionObject::ObjectiveIcon { position: v3(x, y, z), rotation: Vec2::default(), kind: ty });
    idx
}

/// CreateObjectiveIconAttachedTo: the icon is created first, then pAttachTo (Properties[0]) is set
/// from actor.index (a null actor throws after the icon was created, as in C#).
#[allow(clippy::too_many_arguments)]
pub fn create_objective_icon_attached_to(
    st: &mut ScriptState,
    actor: Option<usize>,
    x: f32,
    y: f32,
    z: f32,
    height: i32,
    max_adaptive_height: i32,
    ty: i32,
    can_go_up: bool,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> LuaResult<usize> {
    let idx = create_objective_icon(st, x, y, z, height, max_adaptive_height, ty, can_go_up, note, r, g, b);
    // the error is returned after the icon exists, so a failed call still leaves the icon in the list
    let actor = actor.ok_or_else(nre)?;
    st.logic.actors.definitions[idx].properties[0].value = PropValue::Int(actor as i32);
    Ok(idx)
}

/// Overload picker for CreateObjectiveIconAttachedTo.
/// Overload 1: (Actor, int height, int maxAdaptiveHeight, int type, bool canGoUp, ...)
/// Overload 2: (Actor, float x, int height, float z, int maxAdaptiveHeight, int type, bool canGoUp, ...)
/// They differ at argument 5 (index 4: bool vs number) and argument 7 (index 6: number vs bool).
/// When nothing discriminates (3 or fewer numeric values), overload 1 is chosen.
fn attached_icon_is_overload1(a: &Args) -> bool {
    let is_bool = |i: usize| matches!(a.v.get(i), Some(Value::Boolean(_)));
    let is_num = |i: usize| matches!(a.v.get(i), Some(Value::Integer(_)) | Some(Value::Number(_)));
    if is_bool(4) {
        return true;
    }
    if is_num(4) {
        return false;
    }
    if is_bool(6) {
        return false;
    }
    true
}

// ---------------------------------------------------------------- CreateVehicle

/// CreateVehicle: creates a vehicle actor (type 3) and a Vehicle object at (x, y, z) with a 108-byte creation
/// data block. The vehicle id is added to the spool list when it is not there yet. Returns the actor index.
#[allow(clippy::too_many_arguments)]
pub fn create_vehicle(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    vehicle_id: i32,
    color_id: i32,
    note: Option<&str>,
    flags: i32,
    damage: f32,
    softness: f32,
    weight: f32,
    fragility: f32,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    // angle -= 25.5f; a = (PI/180) * angle; a = a/2 + 45
    let angle = angle - 25.5f32;
    let a = (std::f64::consts::PI / 180.0) * angle as f64;
    let a = (a / 2.0) + 45.0;
    let fwd = forward_from_radians(a);

    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(3, note, color(r, g, b), 0, object_id, |st| {
        vec![
            st.prop("pWeight", 2, PropValue::Float(weight)),
            st.prop("pSoftness", 2, PropValue::Float(softness)),
            st.prop("pIFragility", 2, PropValue::Float(fragility)),
            st.prop("pDemoOnlySoftness", 2, PropValue::Float(1.0)),
            st.prop("pTintValue", 1, PropValue::Int(color_id)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });

    let bx = x.to_le_bytes();
    let by = y.to_le_bytes();
    let bz = z.to_le_bytes();
    let bfx = fwd[0].to_le_bytes();
    let bfy = fwd[1].to_le_bytes();
    let bfz = fwd[2].to_le_bytes();
    let bdmg = damage.to_le_bytes();

    let mut cd: Vec<u8> = Vec::with_capacity(108);
    cd.extend_from_slice(&[4, 0x0C, 0x28, 0x0]);
    cd.extend_from_slice(&[0x0C, 0x0, 0x4C]);
    cd.extend_from_slice(&[0x0, 0x0, 0x0, 0x0, 0x0]);
    cd.extend_from_slice(&[0x01, 0x1C, 0, 0]);
    cd.extend_from_slice(&[0, 0, 0x0, 0x0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0, 0x0]);
    cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    cd.extend_from_slice(&[1, 0x24, 0, 0]);
    // vehicle position (X, Y, Z, W = 1): offsets 44..59
    cd.extend_from_slice(&bx);
    cd.extend_from_slice(&by);
    cd.extend_from_slice(&bz);
    cd.extend_from_slice(&[0, 0, 0x80, 0x3F]);
    cd.extend_from_slice(&[0, 0, 0, 0]);
    // direction
    cd.extend_from_slice(&bfx);
    cd.extend_from_slice(&bfy);
    cd.extend_from_slice(&bfz);
    cd.extend_from_slice(&[5, 0x20, 0, 0]);
    cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0, 0x0]);
    cd.extend_from_slice(&[0, 0, 0x0, 0x0]);
    cd.extend_from_slice(&bdmg);
    cd.extend_from_slice(&[0, 0, 0, 0]);
    debug_assert_eq!(cd.len(), 108);

    st.objects.push(MissionObject::Vehicle { creation_data: cd, position: v3(x, y, z), uid: vehicle_id });
    if !st.spool_vehicles.contains(&vehicle_id) {
        st.spool_vehicles.push(vehicle_id);
    }
    idx
}

// ---------------------------------------------------------------- CreateCollectable

/// CreateCollectable: creates a collectable actor (type 103) and a Collectable object at (x, y, z).
/// Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn create_collectable(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    ty: i32,
    clips: i32,
    rotation: f32,
    note: Option<&str>,
    flags: i32,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(103, note, color(r, g, b), FLAGS_ALT, object_id, |st| {
        vec![
            st.prop("pClips", 1, PropValue::Int(clips)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    st.objects.push(MissionObject::Collectable { kind: ty, rotation, position: v3(x, y, z) });
    idx
}

// ---------------------------------------------------------------- CreateCharacterInVehicle

/// CreateCharacterInVehicle: creates a character with `create_character` at the position of the vehicle
/// object, then replaces the character's creation data with the seat data (sx, sy, sz, seat_type) and sets
/// its uid to the vehicle's object id. `vehicle` must be a Vehicle actor (type 3). Returns the actor index.
#[allow(clippy::too_many_arguments)]
pub fn create_character_in_vehicle(
    st: &mut ScriptState,
    vehicle: Option<usize>,
    skin: u32,
    role: i32,
    personality: Option<String>,
    personality_id: i32,
    personality_index: i32,
    health: f32,
    felony: f32,
    weapon: i32,
    vulnerability: f32,
    sx: f32,
    sy: f32,
    sz: f32,
    seat_type: u8,
    note: Option<String>,
    flags: i32,
    r: u8,
    g: u8,
    b: i32,
) -> LuaResult<usize> {
    let vehicle = vehicle.ok_or_else(nre)?;
    if st.logic.actors.definitions[vehicle].type_id != 3 {
        return Err(rt_err("The inputed actor is not a Vehicle actor type"));
    }
    let vehicle_object_id = st.logic.actors.definitions[vehicle].object_id;
    // VehicleObject veh = (VehicleObject)Objects[vehicle.ObjectId]; x,y,z = veh.Position
    let (x, y, z) = match usize::try_from(vehicle_object_id).ok().and_then(|i| st.objects.get(i)) {
        Some(MissionObject::Vehicle { position, .. }) => (position.x, position.y, position.z),
        Some(_) => return Err(rt_err("Unable to cast object to type 'Zartex.VehicleObject'.")),
        None => {
            return Err(rt_err(
                "Index was out of range. Must be non-negative and less than the size of the collection.",
            ))
        }
    };
    let bsx = sx.to_le_bytes();
    let bsy = sy.to_le_bytes();
    let bsz = sz.to_le_bytes();

    let achar = create_character(
        st,
        x,
        y,
        z,
        1.0,
        skin,
        role,
        personality,
        personality_id,
        personality_index,
        health,
        felony,
        weapon,
        vulnerability,
        note,
        flags,
        r,
        g,
        b,
    )?;

    let char_object_id = st.logic.actors.definitions[achar].object_id;
    let mut cd: Vec<u8> = vec![1, 0, 1, 13];
    cd.extend_from_slice(&bsx);
    cd.extend_from_slice(&bsy);
    cd.extend_from_slice(&bsz);
    cd.extend_from_slice(&[seat_type, 0, 0, 0]);
    let uid = st.logic.actors.definitions[vehicle].object_id;
    match usize::try_from(char_object_id).ok().and_then(|i| st.objects.get_mut(i)) {
        Some(MissionObject::Character { creation_data, uid: u, .. }) => {
            *creation_data = cd;
            *u = uid;
        }
        _ => return Err(rt_err("Unable to cast object to type 'Zartex.CharacterObject'.")),
    }
    Ok(achar)
}

// ---------------------------------------------------------------- CreateCamera

/// CreateCamera: creates a camera actor (type 9) and a Camera object at (x, y, z). The right, up and forward
/// vectors are built from pitch, yaw and roll (in degrees). Returns the index of the new actor.
#[allow(clippy::too_many_arguments)]
pub fn create_camera(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    pitch: f64,
    yaw: f64,
    roll: f64,
    camera_zoom: f32,
    speed: f32,
    blur: f32,
    look_at: Option<usize>,
    attach_to: Option<usize>,
    note: Option<&str>,
    _flags: i32,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let pi = (std::f64::consts::PI / 180.0) * pitch;
    let ya = (std::f64::consts::PI / 180.0) * (yaw + 90.0);
    // NOTE: the C# builds `ro` from roll but the up vector uses the raw `roll` value (ported literally)
    let forward = v3(
        (-(pi.xcos() * ya.xcos())) as f32,
        (-pi.xsin()) as f32,
        (-(pi.xcos() * ya.xsin())) as f32,
    );
    let right = v3(pi.xcos() as f32, 0.0, ya.xsin() as f32);
    let up = v3(0.0, roll.xcos() as f32, roll.xsin() as f32);

    let object_id = st.objects.len() as i32;
    let idx = st.create_actor(9, note, color(r, g, b), FLAGS_ALT, object_id, |st| {
        vec![
            st.prop("pLookAt", 7, PropValue::Int(actor_or_neg(look_at))),
            st.prop("pAttachTo", 7, PropValue::Int(actor_or_neg(attach_to))),
            st.prop("pCameraZoom", 2, PropValue::Float(camera_zoom)),
            st.prop("pSpeed", 2, PropValue::Float(speed)),
            st.prop("pBlur", 2, PropValue::Float(blur)),
        ]
    });
    st.objects.push(MissionObject::Camera { right, up, forward, position: v3(x, y, z), reserved: 0 });
    idx
}

// ---------------------------------------------------------------- CreateCameraLookingAtPosition

/// CreateCameraLookingAtPosition: creates a "Camera Target" volume at (x2, y2, z2), then a camera at
/// (x1, y1, z1) with zero pitch, yaw and roll that uses that volume as its look-at actor. Returns the camera index.
#[allow(clippy::too_many_arguments)]
pub fn create_camera_looking_at_position(
    st: &mut ScriptState,
    x1: f32,
    y1: f32,
    z1: f32,
    x2: f32,
    y2: f32,
    z2: f32,
    camera_zoom: f32,
    speed: f32,
    blur: f32,
    attach_to: Option<usize>,
    note: Option<&str>,
    flags: i32,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let target = test_volume(st, x2, y2, z2, None, 0.0, 1, Some("Camera Target"), r, g, b);
    create_camera(st, x1, y1, z1, 0.0, 0.0, 0.0, camera_zoom, speed, blur, Some(target), attach_to, note, flags, r, g, b)
}

// ---------------------------------------------------------------- registration

/// Adds the creators of this file to the Lua table `t`, each with its Lua argument reading and default values.
/// Does nothing when `dpl` is true, because the DPL class has its own versions of these methods.
pub fn register(lua: &Lua, t: &Table, st: &State, dpl: bool) -> LuaResult<()> {
    if dpl {
        // LuaMissionScriptDPL has its own versions of these methods (not part of this port)
        return Ok(());
    }

    reg(lua, t, st, "CreateAIPath", |lua, st, a| {
        let character = a.actor(0)?;
        let table = match a.v.get(1) {
            Some(Value::Nil) => None,
            _ => Some(a.table(1)?),
        };
        let speed = a.float(2, Some(1.0))?;
        let _burnout_time = a.float(3, Some(0.0))?;
        let desired_transport = a.int(4, Some(0))?;
        let flags = a.int(5, Some(1))?;
        let note = a.string(6, Some(Some("")))?;
        let r = a.int(7, Some(100))?;
        let g = a.int(8, Some(128))?;
        let b = a.int(9, Some(255))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_ai_path(&mut s, character, table.as_ref(), speed, desired_transport, flags, note.as_deref(), r, g, b)?
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "TestVolume", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let attached = a.actor(3)?;
        let radius = a.float(4, Some(0.0))?;
        let flags = a.int(5, Some(1))?;
        let note = a.string(6, Some(Some("")))?;
        let r = a.int(7, Some(100))?;
        let g = a.int(8, Some(128))?;
        let b = a.int(9, Some(255))?;
        let idx = {
            let mut s = st.borrow_mut();
            test_volume(&mut s, x, y, z, attached, radius, flags, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "Marker", |lua, st, a| {
        let x1 = a.float(0, None)?;
        let y1 = a.float(1, None)?;
        let z1 = a.float(2, None)?;
        let x2 = a.float(3, Some(1.0))?;
        let y2 = a.float(4, Some(0.0))?;
        let z2 = a.float(5, Some(0.0))?;
        let note = a.string(6, Some(Some("")))?;
        let r = a.int(7, Some(100))?;
        let g = a.int(8, Some(128))?;
        let b = a.int(9, Some(255))?;
        let idx = {
            let mut s = st.borrow_mut();
            marker(&mut s, x1, y1, z1, x2, y2, z2, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateArea", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let scale_x = a.float(3, None)?;
        let scale_y = a.float(4, None)?;
        let scale_z = a.float(5, None)?;
        let _area_w = a.float(6, Some(1.0))?;
        let _w = a.float(7, Some(1.0))?;
        let attached = a.actor(8)?;
        let radius = a.float(9, Some(0.0))?;
        let flags = a.int(10, Some(1))?;
        let note = a.string(11, Some(Some("")))?;
        let r = a.int(12, Some(100))?;
        let g = a.int(13, Some(128))?;
        let b = a.int(14, Some(255))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_area(&mut s, x, y, z, scale_x, scale_y, scale_z, attached, radius, flags, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "GetAnimatedObjectAt", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let ty = a.int(3, Some(0))?;
        let note = a.string(4, Some(Some("")))?;
        let r = a.int(5, Some(200))?;
        let g = a.int(6, Some(128))?;
        let b = a.int(7, Some(128))?;
        let idx = {
            let mut s = st.borrow_mut();
            get_animated_object_at(&mut s, x, y, z, ty, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateSwitch", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let angle = a.float(3, None)?;
        let ty = a.int(4, Some(10))?;
        let note = a.string(5, Some(Some("")))?;
        let r = a.int(6, Some(200))?;
        let g = a.int(7, Some(128))?;
        let b = a.int(8, Some(128))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_switch(&mut s, x, y, z, angle, ty, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "AITarget", |lua, st, a| {
        let character = a.actor(0)?;
        let x = a.float(1, None)?;
        let y = a.float(2, None)?;
        let z = a.float(3, None)?;
        let walk_speed = a.float(4, None)?;
        let drive_speed = a.float(5, None)?;
        let desired_transport = a.int(6, None)?;
        let attach_to = a.actor(7)?;
        let flags = a.int(8, Some(0))?;
        let note = a.string(9, Some(Some("")))?;
        let r = a.int(10, Some(128))?;
        let g = a.int(11, Some(0))?;
        let b = a.int(12, Some(255))?;
        let idx = {
            let mut s = st.borrow_mut();
            ai_target(
                &mut s,
                character,
                x,
                y,
                z,
                walk_speed,
                drive_speed,
                desired_transport,
                attach_to,
                flags,
                note.as_deref(),
                r,
                g,
                b,
            )?
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "SpecialEffect", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let ty = a.int(3, None)?;
        let force = a.float(4, None)?;
        let range = a.float(5, None)?;
        let time = a.int(6, None)?;
        let attach_to = a.actor(7)?;
        let flags = a.int(8, Some(0))?;
        let note = a.string(9, Some(Some("")))?;
        let r = a.int(10, Some(128))?;
        let g = a.int(11, Some(255))?;
        let b = a.int(12, Some(100))?;
        let idx = {
            let mut s = st.borrow_mut();
            special_effect(&mut s, x, y, z, ty, force, range, time, attach_to, flags, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateObjectiveIcon", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let height = a.int(3, Some(1))?;
        let max_adaptive = a.int(4, Some(0))?;
        let ty = a.int(5, Some(0))?;
        let can_go_up = a.boolean(6, Some(true))?;
        let note = a.string(7, Some(Some("")))?;
        let r = a.int(8, Some(255))?;
        let g = a.int(9, Some(200))?;
        let b = a.int(10, Some(122))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_objective_icon(&mut s, x, y, z, height, max_adaptive, ty, can_go_up, note.as_deref(), r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateObjectiveIconAttachedTo", |lua, st, a| {
        let idx = if attached_icon_is_overload1(a) {
            // (Actor actor, int height = 1, int maxAdaptiveHeight = 0, int type = 0, bool canGoUp = true, string note = "", int r = 255, int g = 200, int b = 122)
            let actor = a.actor(0)?;
            let height = a.int(1, Some(1))?;
            let max_adaptive = a.int(2, Some(0))?;
            let ty = a.int(3, Some(0))?;
            let can_go_up = a.boolean(4, Some(true))?;
            let note = a.string(5, Some(Some("")))?;
            let r = a.int(6, Some(255))?;
            let g = a.int(7, Some(200))?;
            let b = a.int(8, Some(122))?;
            // CreateObjectiveIcon(0, height, 0, height, ...)
            let mut s = st.borrow_mut();
            create_objective_icon_attached_to(
                &mut s,
                actor,
                0.0,
                height as f32,
                0.0,
                height,
                max_adaptive,
                ty,
                can_go_up,
                note.as_deref(),
                r,
                g,
                b,
            )?
        } else {
            // (Actor actor, float x = 0, int height = 1, float z = 0, int maxAdaptiveHeight = 0, int type = 0, bool canGoUp = true, string note = "", int r = 255, int g = 200, int b = 122)
            let actor = a.actor(0)?;
            let x = a.float(1, Some(0.0))?;
            let height = a.int(2, Some(1))?;
            let z = a.float(3, Some(0.0))?;
            let max_adaptive = a.int(4, Some(0))?;
            let ty = a.int(5, Some(0))?;
            let can_go_up = a.boolean(6, Some(true))?;
            let note = a.string(7, Some(Some("")))?;
            let r = a.int(8, Some(255))?;
            let g = a.int(9, Some(200))?;
            let b = a.int(10, Some(122))?;
            // CreateObjectiveIcon(x, height, z, height, ...)
            let mut s = st.borrow_mut();
            create_objective_icon_attached_to(
                &mut s,
                actor,
                x,
                height as f32,
                z,
                height,
                max_adaptive,
                ty,
                can_go_up,
                note.as_deref(),
                r,
                g,
                b,
            )?
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateVehicle", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let angle = a.float(3, None)?;
        let vehicle_id = a.int(4, None)?;
        let color_id = a.int(5, None)?;
        let note = a.string(6, Some(Some("")))?;
        let flags = a.int(7, Some(302186497))?;
        let damage = a.float(8, Some(0.0))?;
        let softness = a.float(9, Some(1.0))?;
        let weight = a.float(10, Some(1.0))?;
        let fragility = a.float(11, Some(1.0))?;
        let r = a.int(12, Some(0))?;
        let g = a.int(13, Some(200))?;
        let b = a.int(14, Some(122))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_vehicle(
                &mut s,
                x,
                y,
                z,
                angle,
                vehicle_id,
                color_id,
                note.as_deref(),
                flags,
                damage,
                softness,
                weight,
                fragility,
                r,
                g,
                b,
            )
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateCollectable", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let ty = a.int(3, None)?;
        let clips = a.int(4, None)?;
        let rotation = a.float(5, Some(0.0))?;
        let note = a.string(6, Some(Some("")))?;
        let flags = a.int(7, Some(1))?;
        let r = a.byte(8, Some(200))? as i32;
        let g = a.byte(9, Some(0))? as i32;
        let b = a.int(10, Some(255))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_collectable(&mut s, x, y, z, ty, clips, rotation, note.as_deref(), flags, r, g, b)
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateCharacterInVehicle", |lua, st, a| {
        let vehicle = a.actor(0)?;
        let skin = a.uint(1, None)?;
        let role = a.int(2, None)?;
        let personality = a.string(3, None)?;
        let personality_id = a.int(4, None)?;
        let personality_index = a.int(5, None)?;
        let health = a.float(6, None)?;
        let felony = a.float(7, None)?;
        let weapon = a.int(8, None)?;
        let vulnerability = a.float(9, None)?;
        let sx = a.float(10, Some(0.0))?;
        let sy = a.float(11, Some(0.0))?;
        let sz = a.float(12, Some(0.0))?;
        let seat_type = a.byte(13, Some(2))?;
        let note = a.string(14, Some(Some("")))?;
        let flags = a.int(15, Some(131073))?;
        let r = a.byte(16, Some(0))?;
        let g = a.byte(17, Some(155))?;
        let b = a.int(18, Some(200))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_character_in_vehicle(
                &mut s,
                vehicle,
                skin,
                role,
                personality,
                personality_id,
                personality_index,
                health,
                felony,
                weapon,
                vulnerability,
                sx,
                sy,
                sz,
                seat_type,
                note,
                flags,
                r,
                g,
                b,
            )?
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateCamera", |lua, st, a| {
        let x = a.float(0, None)?;
        let y = a.float(1, None)?;
        let z = a.float(2, None)?;
        let pitch = a.double(3, None)?;
        let yaw = a.double(4, None)?;
        let roll = a.double(5, None)?;
        let camera_zoom = a.float(6, Some(1.0))?;
        let speed = a.float(7, Some(1.0))?;
        let blur = a.float(8, Some(0.0))?;
        let look_at = a.actor(9)?;
        let attach_to = a.actor(10)?;
        let note = a.string(11, Some(Some("")))?;
        let flags = a.int(12, Some(0))?;
        let r = a.byte(13, Some(0))? as i32;
        let g = a.byte(14, Some(155))? as i32;
        let b = a.int(15, Some(200))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_camera(
                &mut s,
                x,
                y,
                z,
                pitch,
                yaw,
                roll,
                camera_zoom,
                speed,
                blur,
                look_at,
                attach_to,
                note.as_deref(),
                flags,
                r,
                g,
                b,
            )
        };
        ret_actor(lua, st, idx)
    })?;

    reg(lua, t, st, "CreateCameraLookingAtPosition", |lua, st, a| {
        let x1 = a.float(0, None)?;
        let y1 = a.float(1, None)?;
        let z1 = a.float(2, None)?;
        let x2 = a.float(3, None)?;
        let y2 = a.float(4, None)?;
        let z2 = a.float(5, None)?;
        let camera_zoom = a.float(6, Some(1.0))?;
        let speed = a.float(7, Some(1.0))?;
        let blur = a.float(8, Some(0.0))?;
        let attach_to = a.actor(9)?;
        let note = a.string(10, Some(Some("")))?;
        let flags = a.int(11, Some(0))?;
        let r = a.byte(12, Some(0))? as i32;
        let g = a.byte(13, Some(155))? as i32;
        let b = a.int(14, Some(200))?;
        let idx = {
            let mut s = st.borrow_mut();
            create_camera_looking_at_position(
                &mut s,
                x1,
                y1,
                z1,
                x2,
                y2,
                z2,
                camera_zoom,
                speed,
                blur,
                attach_to,
                note.as_deref(),
                flags,
                r,
                g,
                b,
            )
        };
        ret_actor(lua, st, idx)
    })?;

    Ok(())
}
