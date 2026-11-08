//! Driver: Parallel Lines Lua environment: PACKAGE / INITMISSION, the DPL-only creators and the
//! `.sp` package pipeline (port of LuaMissionPackage + LuaMissionScriptDPL).
//! Exposes `MissionEntry`, `PackageEnv`, the DPL creators (`dpl_create_character`, `dpl_create_vehicle`, `dpl_create_mission_actor`),
//! `register_dpl_methods`, `setup_dpl`, `run_dpl_script` and `compile_dpl_file`.
//! Uses `api_gen::register_generated`, `api_manual` (`nre`, `reg`, `register_manual`), `api_consts::register_constants`, `dpl` (the package model),
//! `compile` (`new_lua`, `CompileError`), `binio`, `logic`, `luaapi`, `objects` and `script`.
//! Used by the `zartex-luac` command (src/bin/zartex-luac.rs), by `zartex-app` (src/main.rs, `compile_dpl_file`) and by the integration tests in tests/.
//!
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class and its compiler).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file

use crate::api_gen::register_generated;
use crate::api_manual::{nre, reg};
use crate::binio::*;
use crate::compile::*;
use crate::dpl::*;
use crate::logic::{ActorSetTable, PropValue};
use crate::luaapi::*;
use crate::objects::MissionObject;
use crate::script::*;
use mlua::{Lua, MultiValue, Result as LuaResult, Table, Value};
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------- package environment

/// One mission of a package: its script state, name, id, position, flags and whether it defines the root.
pub struct MissionEntry {
    pub st: State,
    pub name: Option<String>,
    pub id: i16,
    pub x: f32,
    pub y: f32,
    pub flags: i32,
    pub define_root: bool,
}

/// The package a script builds: the INITMISSION entry, every mission made by PACKAGE.CreateMission, and the AddMission calls.
pub struct PackageEnv {
    pub init: MissionEntry,
    /// every mission created with PACKAGE.CreateMission, by creation order
    pub created: Vec<MissionEntry>,
    /// (sub mission id, index into `created`) in AddMission order
    pub added: Vec<(i16, usize)>,
}

type Env = Rc<RefCell<PackageEnv>>;

/// Copies one entry's script state into a new DplMission: objects, strings, sound banks, actors, nodes, actor sets, wire collections and counters.
fn entry_to_mission(e: &MissionEntry) -> DplMission {
    let s = e.st.borrow();
    let mut m = DplMission::new(e.name.as_deref(), e.id, e.x, e.y, e.flags, e.define_root);
    m.objects.objects = s.objects.clone();
    m.logic.strings = s.logic.strings.clone();
    m.logic.sound_banks = s.logic.sound_banks.clone();
    m.logic.actors = s.logic.actors.clone();
    m.logic.nodes = s.logic.nodes.clone();
    m.logic.actor_sets = ActorSetTable { table: s.actor_setup.clone() };
    m.logic.wires.collections = s.wires.iter().map(|w| crate::logic::WireCollection { wires: w.clone() }).collect();
    m.logic.counters = s.logic.counters.clone();
    m
}

impl PackageEnv {
    /// Builds the package: the init mission first, then each AddMission call in its call order, with its sub mission id.
    pub fn to_package(&self) -> DplPackage {
        let mut pkg = DplPackage { init: entry_to_mission(&self.init), missions: Vec::new(), lookup: SpoolSystemLookup::default() };
        for &(sub, i) in &self.added {
            pkg.add_mission(entry_to_mission(&self.created[i]), sub);
        }
        pkg
    }
}

// ---------------------------------------------------------------- DPL-only API

/// Reads a Lua table of (asset, density) rows into two vectors: element 1 of a row is the asset id, element 2 the density.
/// Raises an error when a row has a nil asset or density.
fn asset_densities(t: &Table) -> LuaResult<(Vec<i32>, Vec<f32>)> {
    let len = t.raw_len() as i64;
    let mut assets = Vec::new();
    let mut densities = Vec::new();
    // indices 0 to len are visited; missing (nil) entries are skipped
    for i in 0..=len {
        let v: Value = t.raw_get(i)?;
        if matches!(v, Value::Nil) {
            continue;
        }
        let row = match v {
            Value::Table(r) => r,
            _ => return Err(rt_err("Bad argument #3 - The table must be a table containing tables in (asset, density) format")),
        };
        let a: Value = row.raw_get(1)?;
        let d: Value = row.raw_get(2)?;
        if matches!(a, Value::Nil) || matches!(d, Value::Nil) {
            return Err(rt_err(format!(
                "Bad argument #3 - Asset Densities table index {i} has some invalid asset densities values (nil)"
            )));
        }
        let num = |v: &Value| match v {
            Value::Integer(i) => *i as f64,
            Value::Number(n) => *n,
            _ => 0.0,
        };
        assets.push(cast_int(num(&a)));
        densities.push(num(&d) as f32);
    }
    Ok((assets, densities))
}

/// DensityControl / PedestrianDensityControl: the AssetDensities property takes its name string id
/// BEFORE the node initializer runs (right after the wire collection is created).
fn density_node(
    st: &mut ScriptState,
    type_id: u8,
    note: Option<&str>,
    color: [u8; 4],
    ad: (Vec<i32>, Vec<f32>),
    flags: i32,
    before_ad: Vec<(&str, f32)>,
) -> (usize, usize) {
    let string_id = st.note_id(note);
    let wire = st.new_wire_collection();
    let ad_prop = st.prop("Asset Densities", 32, PropValue::Assets { assets: ad.0, densities: ad.1 });
    let mut props = vec![st.prop("pWireCollection", 19, PropValue::Int(wire as i32))];
    for (name, v) in before_ad {
        props.push(st.prop(name, 2, PropValue::Float(v)));
    }
    props.push(ad_prop);
    props.push(st.prop("Flags", 9, PropValue::Int(flags)));
    let mut def = crate::logic::NodeDef::new(type_id, string_id);
    def.color = color;
    def.properties = props;
    st.logic.nodes.definitions.push(def);
    (st.logic.nodes.definitions.len() - 1, wire)
}

/// Orientation vectors shared by the DPL actor creators (float math, as in the C#).
fn orientation(angle: f32) -> (Vec4, Vec4, Vec4) {
    // the radian factor and the angle sums are f32; the sin and cos calls take f64 values
    let rad = (std::f64::consts::PI / 180.0) as f32;
    let a = rad * angle;
    let a2 = rad * (angle + 90.0);
    let a3 = rad * -90.0f32;
    let (a, a2, a3) = (a as f64, a2 as f64, a3 as f64);
    let fwd = Vec4 { x: (1.0 * a.xcos()) as f32, y: 0.0, z: (1.0 * a.xsin()) as f32, w: 0.0 };
    let l = Vec4 { x: (1.0 * a2.xcos()) as f32, y: 0.0, z: (1.0 * a2.xsin()) as f32, w: 0.0 };
    let up = Vec4 { x: (a3.xcos() * a.xcos()) as f32, y: (-a3.xsin()) as f32, z: (a3.xcos() * a.xsin()) as f32, w: 0.0 };
    (fwd, l, up)
}

/// Builds a matrix property from a position (w = 1) and the right (`l`), up and forward vectors.
fn matrix(x: f32, y: f32, z: f32, l: Vec4, up: Vec4, fwd: Vec4) -> PropValue {
    // new MatrixProperty(position, right = l, up, forward)
    PropValue::Matrix { right: l, up, forward: fwd, pos: Vec4 { x, y, z, w: 1.0 } }
}

/// Creates a DPL character actor and returns its actor index. `angle` is in degrees.
/// A player character always gets flags 65537 and sets the start position to (x, y).
#[allow(clippy::too_many_arguments)]
pub fn dpl_create_character(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    skin: i32,
    health: f32,
    felony: f32,
    vehicle: Option<usize>,
    vehicle_seat: i32,
    weapon: i32,
    player: bool,
    mut flags: i32,
    note: Option<String>,
    r: u8,
    g: u8,
    b: i32,
) -> usize {
    // the player character always uses these flags and sets the start position
    if player {
        flags = 65537;
    }
    let (fwd, l, up) = orientation(angle);
    if player {
        st.summary.start_position = Vec2 { x, y };
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let idx = st.create_actor(2, note.as_deref(), color, 0x14B9, -1, |st| {
        vec![
            st.prop("Matrix", 23, matrix(x, y, z, l, up, fwd)),
            st.prop("Character Skin Type", 28, PropValue::Int(skin)),
            st.prop("Initial Health", 2, PropValue::Float(health)),
            st.prop("Initial Felony", 2, PropValue::Float(felony)),
            st.prop("Weapon", 6, PropValue::Int(weapon)),
            st.prop("Initial Vehicle", 7, PropValue::Int(vehicle.map(|v| v as i32).unwrap_or(-1))),
            st.prop("Initial Vehicle Seat", 6, PropValue::Int(vehicle_seat)),
            st.prop("Flags", 9, PropValue::Int(flags)),
        ]
    });
    // the reserved field of a DPL character actor is set to 2974
    st.logic.actors.definitions[idx].reserved = 2974;
    idx
}

/// Creates a vehicle actor and returns its actor index. Angle is in degrees.
#[allow(clippy::too_many_arguments)]
pub fn dpl_create_vehicle(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    angle: f32,
    vehicle_id: i32,
    color_id: i32,
    initial_speed: f32,
    initial_felony: f32,
    bullet_softness: f32,
    explosion_softness: f32,
    impact_softness: f32,
    impact_fragility: f32,
    attached: Option<usize>,
    note: Option<String>,
    flags: i32,
    r: i32,
    g: i32,
    b: i32,
) -> usize {
    let _ = bullet_softness; // the C# never uses it ("Bullet Softness" gets explosionSoftness)
    let (fwd, l, up) = orientation(angle);
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    st.create_actor(3, note.as_deref(), color, 0, -1, |st| {
        vec![
            st.prop("Matrix", 23, matrix(x, y, z, l, up, fwd)),
            st.prop("Vehicle Type", 28, PropValue::Int(vehicle_id)),
            st.prop("Initial Speed", 2, PropValue::Float(initial_speed)),
            st.prop("Initial Felony", 2, PropValue::Float(initial_felony)),
            st.prop("Impact Softness", 2, PropValue::Float(impact_softness)),
            st.prop("Explosion Softness", 2, PropValue::Float(explosion_softness)),
            st.prop("Bullet Softness", 2, PropValue::Float(explosion_softness)),
            st.prop("Impact Fragility", 2, PropValue::Float(impact_fragility)),
            st.prop("Vehicle Tint", 29, PropValue::Int(color_id)),
            st.prop("Tint Value", 1, PropValue::Int(color_id)),
            st.prop("Attached Vehicle", 7, PropValue::Int(attached.map(|v| v as i32).unwrap_or(-1))),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    })
}

/// Creates a mission actor (type 106) and returns its actor index. The temp name, temp description, file name and FMV file
/// are required: a nil value raises the null reference error.
#[allow(clippy::too_many_arguments)]
pub fn dpl_create_mission_actor(
    st: &mut ScriptState,
    x: f32,
    y: f32,
    z: f32,
    sub_mission_id: i32,
    event_type: i32,
    pot_number: i32,
    icon_type: i32,
    temp_name: Option<String>,
    temp_desc: Option<String>,
    file_name: Option<String>,
    fmv_file: Option<String>,
    flags: i32,
    note: Option<String>,
    r: u8,
    g: u8,
    b: i32,
) -> LuaResult<usize> {
    // a = a2 = a3 = 0 (ints): forward = left = (1,0,0,0); up = (1,-0,0,0)
    let fwd = Vec4 { x: 1.0, y: 0.0, z: 0.0, w: 0.0 };
    let l = fwd;
    let up = Vec4 { x: 1.0, y: (-(0.0f64.xsin())) as f32, z: 0.0, w: 0.0 };
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    // the four strings are required; each nil raises the null reference error
    let temp_name = temp_name.ok_or_else(nre)?;
    let temp_desc = temp_desc.ok_or_else(nre)?;
    let file_name = file_name.ok_or_else(nre)?;
    let fmv_file = fmv_file.ok_or_else(nre)?;
    let idx = st.create_actor(106, note.as_deref(), color, 0x14B9, -1, |st| {
        let mut v = vec![st.prop("Matrix", 23, matrix(x, y, z, l, up, fwd))];
        v.push(st.prop("Sub Mission ID", 6, PropValue::Int(sub_mission_id)));
        for (val, name) in [(&temp_name, "Temp Mission Name"), (&temp_desc, "Temp Mission Description"), (&file_name, "FileName")] {
            let sid = st.string_id(val);
            v.push(st.prop(name, 3, PropValue::Str(sid)));
        }
        v.push(st.prop("Event Type", 6, PropValue::Int(event_type)));
        v.push(st.prop("Pot Number", 1, PropValue::Int(pot_number)));
        let sid = st.string_id(&fmv_file);
        v.push(st.prop("FMV FileName", 3, PropValue::Str(sid)));
        v.push(st.prop("Icon Type", 6, PropValue::Int(icon_type)));
        v.push(st.prop("Flags", 9, PropValue::Int(flags)));
        v
    });
    st.logic.actors.definitions[idx].reserved = 0;
    Ok(idx)
}

/// Registers the DPL-only methods on a mission table: CountActor, DensityControl, PedestrianDensityControl,
/// CreateCharacter, CreateVehicle and CreateMissionActor.
pub fn register_dpl_methods(lua: &Lua, t: &Table, st: &State) -> LuaResult<()> {
    reg(lua, t, st, "CountActor", |_, st, a| {
        // the actor index is appended to the counter list
        let actor = a.actor(0)?.ok_or_else(nre)?;
        st.borrow_mut().logic.counters.items.push(actor as i32);
        Ok(Value::Nil)
    })?;
    reg(lua, t, st, "DensityControl", |lua, st, a| {
        let active = a.float(0, None)?;
        let parked = a.float(1, None)?;
        let tbl = a.table(2)?;
        let litter = a.float(3, Some(0.8))?;
        let flags = a.int(4, Some(2))?;
        let note = a.string(5, Some(Some("")))?;
        let (r, g, b) = (a.int(6, Some(0))?, a.int(7, Some(200))?, a.int(8, Some(122))?);
        let ad = asset_densities(&tbl)?;
        let color = node_color(r as f64, g as f64, b as f64, 255.0);
        let (idx, wire) = density_node(
            &mut st.borrow_mut(),
            100,
            note.as_deref(),
            color,
            ad,
            flags,
            vec![("Active Traffic Density", active), ("Parked Traffic Density", parked), ("Litter Density", litter)],
        );
        ret_node(lua, st, idx, wire)
    })?;
    reg(lua, t, st, "PedestrianDensityControl", |lua, st, a| {
        let ped = a.float(0, None)?;
        let tbl = a.table(1)?;
        let flags = a.int(2, Some(2))?;
        let note = a.string(3, Some(Some("")))?;
        let (r, g, b) = (a.int(4, Some(0))?, a.int(5, Some(200))?, a.int(6, Some(122))?);
        let ad = asset_densities(&tbl)?;
        let color = node_color(r as f64, g as f64, b as f64, 255.0);
        let (idx, wire) =
            density_node(&mut st.borrow_mut(), 105, note.as_deref(), color, ad, flags, vec![("Pedestrian Density", ped)]);
        ret_node(lua, st, idx, wire)
    })?;
    reg(lua, t, st, "CreateCharacter", |lua, st, a| {
        let idx = dpl_create_character(
            &mut st.borrow_mut(),
            a.float(0, None)?,
            a.float(1, None)?,
            a.float(2, None)?,
            a.float(3, None)?,
            a.int(4, None)?,
            a.float(5, None)?,
            a.float(6, None)?,
            a.actor(7)?,
            a.int(8, Some(0))?,
            a.int(9, Some(0))?,
            a.boolean(10, Some(false))?,
            a.int(11, Some(1))?,
            a.string(12, Some(Some("")))?,
            a.byte(13, Some(0))?,
            a.byte(14, Some(155))?,
            a.int(15, Some(200))?,
        );
        ret_actor(lua, st, idx)
    })?;
    reg(lua, t, st, "CreateVehicle", |lua, st, a| {
        let idx = dpl_create_vehicle(
            &mut st.borrow_mut(),
            a.float(0, None)?,
            a.float(1, None)?,
            a.float(2, None)?,
            a.float(3, None)?,
            a.int(4, None)?,
            a.int(5, None)?,
            a.float(6, Some(0.0))?,
            a.float(7, Some(0.0))?,
            a.float(8, Some(0.3333))?,
            a.float(9, Some(0.5))?,
            a.float(10, Some(0.6667))?,
            a.float(11, Some(0.6667))?,
            a.actor(12)?,
            a.string(13, Some(Some("")))?,
            a.int(14, Some(302186497))?,
            a.int(19, Some(0))?,
            a.int(20, Some(200))?,
            a.int(21, Some(122))?,
        );
        ret_actor(lua, st, idx)
    })?;
    reg(lua, t, st, "CreateMissionActor", |lua, st, a| {
        let idx = dpl_create_mission_actor(
            &mut st.borrow_mut(),
            a.float(0, None)?,
            a.float(1, None)?,
            a.float(2, None)?,
            a.int(3, None)?,
            a.int(4, None)?,
            a.int(5, Some(0))?,
            a.int(6, Some(12))?,
            a.string(7, Some(Some("")))?,
            a.string(8, Some(Some("")))?,
            a.string(9, Some(Some("")))?,
            a.string(10, Some(Some("")))?,
            a.int(11, Some(1))?,
            a.string(12, Some(Some("")))?,
            a.byte(13, Some(0))?,
            a.byte(14, Some(155))?,
            a.int(15, Some(200))?,
        )?;
        ret_actor(lua, st, idx)
    })?;
    Ok(())
}

// ---------------------------------------------------------------- Lua environment

/// A mission object as scripts see it (`INITMISSION`, the result of `PACKAGE.CreateMission`).
fn mission_table(lua: &Lua, st: &State) -> LuaResult<Table> {
    let t = lua.create_table()?;
    register_generated(lua, &t, st, true)?;
    crate::api_manual::register_manual(lua, &t, st, true)?;
    register_dpl_methods(lua, &t, st)?;
    add_aliases(&t)?;
    t.set("missionSummary", lua.create_userdata(SummaryH { st: st.clone() })?)?;
    Ok(t)
}

/// Creates a mission entry with an empty script state.
fn new_entry(name: Option<String>, id: i16, x: f32, y: f32, flags: i32, define_root: bool) -> MissionEntry {
    MissionEntry {
        st: Rc::new(RefCell::new(ScriptState::new(""))),
        name,
        id,
        x,
        y,
        flags,
        define_root,
    }
}

/// Installs INITMISSION, the PACKAGE table (CreateMission, AddMission) and the constants in a Lua state.
pub fn setup_dpl(lua: &Lua, env: &Env) -> LuaResult<()> {
    let g = lua.globals();
    // INITMISSION = package.InitMission (created with CreateMission(null, 5, 0, 0, 0, false))
    let init_t = mission_table(lua, &env.borrow().init.st)?;
    g.set("INITMISSION", init_t)?;

    let package = lua.create_table()?;
    {
        let env = env.clone();
        package.set(
            "CreateMission",
            lua.create_function(move |lua, args: MultiValue| {
                let a = Args::new(args, |v| matches!(v, Value::Table(_)));
                let name = a.string(0, Some(None))?;
                let id = a.short(1, Some(5))?;
                let x = a.float(2, Some(0.0))?;
                let y = a.float(3, Some(0.0))?;
                let flags = a.int(4, Some(0x01010000))?;
                let define_root = a.boolean(5, Some(true))?;
                let entry = new_entry(name, id, x, y, flags, define_root);
                let st = entry.st.clone();
                let index = {
                    let mut e = env.borrow_mut();
                    e.created.push(entry);
                    e.created.len() - 1
                };
                let t = mission_table(lua, &st)?;
                t.set("__mission_index", index as i64)?;
                Ok(t)
            })?,
        )?;
    }
    {
        let env = env.clone();
        package.set(
            "AddMission",
            lua.create_function(move |_, args: MultiValue| {
                let a = Args::new(args, |v| matches!(v, Value::Table(t) if t.contains_key("CreateMission").unwrap_or(false)));
                let mission = a.table(0)?;
                let sub = a.short(1, None)?;
                let index: i64 = mission
                    .get("__mission_index")
                    .map_err(|_| rt_err("Object reference not set to an instance of an object."))?;
                env.borrow_mut().added.push((sub, index as usize));
                Ok(())
            })?,
        )?;
    }
    add_aliases(&package)?;
    g.set("PACKAGE", package)?;
    crate::api_consts::register_constants(lua)?;
    Ok(())
}

/// Runs a package script file and returns the package environment it built. A UTF-8 byte order mark is skipped.
pub fn run_dpl_script(path: &str) -> Result<PackageEnv, CompileError> {
    let src = std::fs::read(path).map_err(|e| CompileError::Lua(format!("cannot read {path}: {e}")))?;
    let env: Env = Rc::new(RefCell::new(PackageEnv {
        init: new_entry(None, 5, 0.0, 0.0, 0, false),
        created: Vec::new(),
        added: Vec::new(),
    }));
    let lua = new_lua()?;
    setup_dpl(&lua, &env)?;
    let src = if src.starts_with(&[0xEF, 0xBB, 0xBF]) { &src[3..] } else { &src[..] };
    lua.load(src).set_name(format!("@{path}")).exec()?;
    drop(lua);
    Rc::try_unwrap(env).map(|c| c.into_inner()).map_err(|_| CompileError::Lua("package still referenced".into()))
}

/// Runs a package script file and serialises the package it builds to bytes.
pub fn compile_dpl_file(path: &str) -> Result<Vec<u8>, CompileError> {
    let env = run_dpl_script(path)?;
    let mut pkg = env.to_package();
    Ok(pkg.to_bytes()?)
}

// keeps the MissionObject import used
#[allow(dead_code)]
fn _unused(_: MissionObject) {}
