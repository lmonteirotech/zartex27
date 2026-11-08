//! Hand-ported parts of the Lua API (everything tools_gen_api.py cannot translate safely).
//! Each port keeps the C# evaluation order: string ids are created exactly when the C# creates them.
//! Exposes `nre`, `reg` (adds one method to a mission table), `create_character` and `register_manual`.
//! Uses `binio`, `logic::PropValue`, `luaapi`, `objects::MissionObject` and `script`, and calls the register functions of api_fix and api_part_a/b/c.
//! Used by `api_dpl` (the DPL mission table), by `compile` (the normal mission table), and by api_part_a/b/c and api_fix through `reg` and `nre`.
//!
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file

use crate::binio::*;
use crate::logic::PropValue;
use crate::luaapi::*;
use crate::objects::MissionObject;
use crate::script::*;
use mlua::{Lua, MultiValue, Result as LuaResult, Table, Value};

/// The error raised when a required object argument is nil (the C# NullReferenceException message).
pub fn nre() -> mlua::Error {
    rt_err("Object reference not set to an instance of an object.")
}

/// Registers `name` as a function taking `Args` (self dropped) and returning a Lua value.
pub fn reg(
    lua: &Lua,
    t: &Table,
    st: &State,
    name: &str,
    f: impl Fn(&Lua, &State, &Args) -> LuaResult<Value> + 'static,
) -> LuaResult<()> {
    let st = st.clone();
    let this = t.clone();
    t.set(
        name,
        lua.create_function(move |lua, args: MultiValue| {
            // drop `self` only when the call used `:` (first arg is this very table)
            let a = Args::new(args, |v| matches!(v, Value::Table(x) if x.to_pointer() == this.to_pointer()));
            f(lua, &st, &a)
        })?,
    )
}

// ---------------------------------------------------------------- CreateCharacter (Driv3r)

/// Creates a character actor and returns its actor index. `angle` is in degrees.
/// When `role` is 1 the start position is set to (x, z). The character object is pushed onto `st.objects`.
#[allow(clippy::too_many_arguments)]
pub fn create_character(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    skin: u32,
    role: i32,
    personality: Option<String>,
    personality_id: i32,
    personality_index: i32,
    health: f32,
    felony: f32,
    weapon: i32,
    vulnerability: f32,
    note: Option<String>,
    flags: i32,
    r: u8,
    g: u8,
    b: i32,
) -> LuaResult<usize> {
    // angle converted from degrees to radians in double precision
    let a = (std::f64::consts::PI / 180.0) * angle as f64;
    let fwd = forward_from_radians(a);
    if role == 1 {
        st.summary.start_position = Vec2 { x, y: z };
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let object_id = st.objects.len() as i32;
    // a nil personality raises the same error as the C# null reference
    let personality = personality.ok_or_else(nre)?;
    let idx = st.create_actor(2, note.as_deref(), color, 0x2FD7, object_id, |st| {
        vec![
            st.prop("pRole", 1, PropValue::Int(role)),
            {
                let pid = st.string_id(&personality);
                st.prop("pPersonality", 8, PropValue::TextFile { value: pid, index: personality_id as i16 })
            },
            st.prop("pPersonalityIndex", 1, PropValue::Int(personality_index)),
            st.prop("pHealth", 2, PropValue::Float(health)),
            st.prop("pFelony", 2, PropValue::Float(felony)),
            st.prop("pWeapon", 6, PropValue::Int(weapon)),
            st.prop("pVulnerability", 2, PropValue::Float(vulnerability)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    // creation data: a 4-byte header, then the position and the forward vector as little-endian f32 values
    let mut cd = vec![0u8, 0, 1, 0x0D];
    for v in [x, y, z, fwd[0], fwd[1], fwd[2]] {
        cd.extend_from_slice(&v.to_le_bytes());
    }
    st.objects.push(MissionObject::Character {
        creation_data: cd,
        position: Vec3 { x, y, z },
        skin_id: skin,
        uid: -1073623027,
    });
    Ok(idx)
}

/// Registers the hand-ported methods on a mission table, then the api_fix and api_part_a/b/c methods.
/// `dpl` selects the Parallel Lines environment; CreateCharacter is registered here only when `dpl` is false.
pub fn register_manual(lua: &Lua, t: &Table, st: &State, dpl: bool) -> LuaResult<()> {
    // the DPL environment has its own CreateCharacter (api_dpl.rs)
    if !dpl {
        reg(lua, t, st, "CreateCharacter", |lua, st, a| {
            let idx = {
                let mut s = st.borrow_mut();
                create_character(
                    &mut s,
                    a.float(0, None)?,
                    a.float(1, None)?,
                    a.float(2, None)?,
                    a.float(3, None)?,
                    a.uint(4, None)?,
                    a.int(5, None)?,
                    a.string(6, None)?,
                    a.int(7, None)?,
                    a.int(8, None)?,
                    a.float(9, None)?,
                    a.float(10, None)?,
                    a.int(11, None)?,
                    a.float(12, None)?,
                    a.string(13, Some(Some("")))?,
                    a.int(14, Some(131073))?,
                    a.byte(15, Some(0))?,
                    a.byte(16, Some(155))?,
                    a.int(17, Some(200))?,
                )?
            };
            ret_actor(lua, st, idx)
        })?;
    }
    crate::api_fix::register(lua, t, st, dpl)?;
    crate::api_part_a::register(lua, t, st, dpl)?;
    crate::api_part_b::register(lua, t, st, dpl)?;
    crate::api_part_c::register(lua, t, st, dpl)?;
    Ok(())
}
