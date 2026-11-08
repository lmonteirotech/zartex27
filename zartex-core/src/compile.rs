//! Driv3r Lua -> .mpc pipeline (port of LuaC `ProcessCompilationForLuaMissionScript`) and the
//! Lua environment (MISSION global + constants).
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class and its compiler).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - added seed_state, install_getters and apply_snippet: the "Add to mission" mode, which runs a script on an already open mission
//! - setup_driv3r registers the raw calls of luaraw.rs in the MISSION table
//!
//! This file exposes the Lua entry points: new_lua, setup_driv3r (the MISSION table and constants), run_driv3r_script,
//! compile_to_mpc and compile_file (script to .mpc bytes), seed_state, install_getters and apply_snippet (running a
//! script on an existing mission), and CompileError. It uses the generated and hand-written MISSION calls
//! (api_gen, api_manual, luaraw, luaapi), the script state (script), the binary helpers (binio), the logic and
//! mission models (logic, mission), and the Lua engine (mlua). It is used by api_dpl.rs, by readable.rs
//! (seed_state, install_getters, setup_driv3r), by zartex-app (apply_snippet, compile_file), by the bin
//! zartex-luac and by the tests of the crate.

use crate::api_gen::register_generated;
use crate::binio::*;
use crate::luaapi::*;
use crate::mission::*;
use crate::script::*;
use mlua::{Lua, LuaOptions, StdLib, Table};
use std::cell::RefCell;
use std::rc::Rc;

/// The template .mpc file embedded at build time. compile_to_mpc starts from its content.
pub static TEMPLATE_MPC: &[u8] = include_bytes!("../assets/template.mpc");

/// Error of a script compilation.
#[derive(Debug)]
pub enum CompileError {
    /// Error from Lua (reading the file, running the script, or the state still being referenced)
    Lua(String),
    /// Error from the model (reading the template or writing the .mpc bytes)
    Model(ModelError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::Lua(s) => write!(f, "Lua error: {s}"),
            CompileError::Model(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for CompileError {}
impl From<mlua::Error> for CompileError {
    fn from(e: mlua::Error) -> Self {
        CompileError::Lua(e.to_string())
    }
}
impl From<ModelError> for CompileError {
    fn from(e: ModelError) -> Self {
        CompileError::Model(e)
    }
}

/// Creates a Lua state with the table, string, math, bit and coroutine libraries (no io, os or debug).
pub fn new_lua() -> mlua::Result<Lua> {
    // MoonSharp "preset default" exposes the standard Lua 5.2 libraries (no io/os/debug).
    Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT | StdLib::COROUTINE,
        LuaOptions::default(),
    )
}

/// Registers MISSION + constants for a Driv3r script.
/// Builds the MISSION table in this order: generated calls, manual calls, raw calls (luaraw), name aliases,
/// the `missionSummary` member; then sets it as the global MISSION and registers the constants.
pub fn setup_driv3r(lua: &Lua, st: &State) -> mlua::Result<()> {
    let g = lua.globals();
    let mission: Table = lua.create_table()?;
    register_generated(lua, &mission, st, false)?;
    crate::api_manual::register_manual(lua, &mission, st, false)?;
    crate::luaraw::register_raw(lua, &mission, st)?;
    add_aliases(&mission)?;
    mission.set("missionSummary", lua.create_userdata(SummaryH { st: st.clone() })?)?;
    g.set("MISSION", mission)?;
    crate::api_consts::register_constants(lua)?;
    Ok(())
}

/// Runs a Driv3r Lua script file and returns the script state it built.
/// The folder of the file becomes the working directory; a UTF-8 byte order mark at the start of the file is removed.
pub fn run_driv3r_script(path: &str) -> Result<ScriptState, CompileError> {
    let src = std::fs::read(path).map_err(|e| CompileError::Lua(format!("cannot read {path}: {e}")))?;
    let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let st: State = Rc::new(RefCell::new(ScriptState::new(&dir)));
    st.borrow_mut().source_path = path.to_string();
    let lua = new_lua()?;
    setup_driv3r(&lua, &st)?;
    // MoonSharp's DoFile strips a UTF-8 BOM
    let src = if src.starts_with(&[0xEF, 0xBB, 0xBF]) { &src[3..] } else { &src[..] };
    lua.load(src).set_name(format!("@{path}")).exec()?;
    drop(lua);
    Rc::try_unwrap(st).map(|c| c.into_inner()).map_err(|_| CompileError::Lua("script state still referenced".into()))
}

/// Applies a finished script to the embedded template and returns the .mpc bytes
/// (ProcessCompilationForLuaMissionScript with a fresh output file).
pub fn compile_to_mpc(script: &ScriptState) -> Result<Vec<u8>, CompileError> {
    // starts from the embedded template; the script's actors, nodes, wires, objects, strings,
    // sound banks and summary values then replace the template's content
    let mut mf = MissionFile::from_bytes(TEMPLATE_MPC)?;
    let l = &script.logic;
    mf.mission.logic.actors.definitions = l.actors.definitions.clone();
    mf.mission.logic.nodes.definitions = l.nodes.definitions.clone();
    mf.mission.logic.wires.collections =
        script.wires.iter().map(|w| crate::logic::WireCollection { wires: w.clone() }).collect();
    mf.mission.objects.objects = script.objects.clone();
    mf.mission.logic.strings.strings = l.strings.strings.clone();
    mf.mission.logic.sound_banks.items = l.sound_banks.items.clone();

    let summary = mf.summary.get_or_insert_with(Default::default);
    summary.start_position = script.summary.start_position;
    summary.city_type = script.summary.city_type;
    summary.mission_id = script.summary.mood_id;
    Ok(mf.to_bytes()?)
}

/// Runs the script at `path` (run_driv3r_script) and returns the .mpc bytes (compile_to_mpc).
pub fn compile_file(path: &str) -> Result<Vec<u8>, CompileError> {
    let script = run_driv3r_script(path)?;
    compile_to_mpc(&script)
}

/// A script state that starts from the mission (what `apply_snippet` and the readable exporter's sandbox run on).
pub fn seed_state(mission: &ExportedMission, summary: &Option<MissionSummary>) -> State {
    let mut s = ScriptState::new("");
    s.logic.actors.definitions = mission.logic.actors.definitions.clone();
    s.logic.nodes.definitions = mission.logic.nodes.definitions.clone();
    s.logic.strings.strings = mission.logic.strings.strings.clone();
    s.logic.sound_banks.items = mission.logic.sound_banks.items.clone();
    s.wires = mission.logic.wires.collections.iter().map(|c| c.wires.clone()).collect();
    s.objects = mission.objects.objects.clone();
    if let Some(sm) = summary.as_ref() {
        s.summary.start_position = sm.start_position;
        s.summary.city_type = sm.city_type;
        s.summary.mood_id = sm.mission_id;
    }
    Rc::new(RefCell::new(s))
}

/// `MISSION.GetActor(i)` / `MISSION.GetNode(i)`: handles to what the state already holds.
/// GetActor errors when `i` is out of range. GetNode also needs the node's first property to be an
/// integer (its wire collection index); otherwise it errors. Sets both on the MISSION table of the
/// Lua state, then adds the name aliases again.
pub fn install_getters(lua: &Lua, st: &State) -> mlua::Result<()> {
    let mission_t: Table = lua.globals().get("MISSION")?;
    let st2 = st.clone();
    mission_t.set(
        "GetActor",
        lua.create_function(move |lua, i: i64| {
            let n = st2.borrow().logic.actors.definitions.len();
            if i < 0 || i as usize >= n {
                return Err(mlua::Error::runtime(format!("GetActor: no actor {i} (the mission has {n})")));
            }
            lua.create_userdata(ActorH { st: st2.clone(), idx: i as usize })
        })?,
    )?;
    let st3 = st.clone();
    mission_t.set(
        "GetNode",
        lua.create_function(move |lua, i: i64| {
            let (n, wire) = {
                let s = st3.borrow();
                let n = s.logic.nodes.definitions.len();
                let wire = s.logic.nodes.definitions.get(i.max(0) as usize).and_then(|d| match d.properties.first().map(|p| &p.value) {
                    Some(crate::logic::PropValue::Int(w)) => Some(*w as usize),
                    _ => None,
                });
                (n, wire)
            };
            match wire {
                Some(w) if i >= 0 && (i as usize) < n => lua.create_userdata(NodeH { st: st3.clone(), idx: i as usize, wire: w }),
                _ => Err(mlua::Error::runtime(format!("GetNode: no node {i} with a wire collection (the mission has {n} nodes)"))),
            }
        })?,
    )?;
    add_aliases(&mission_t)?;
    Ok(())
}

/// Runs a Lua snippet on an existing mission: the script state starts from the mission's actors, nodes, wires, objects
/// and strings, so everything the snippet creates is appended and `MISSION.GetActor(i)` / `MISSION.GetNode(i)` reach the
/// existing ones (to wire new nodes to them). The result replaces the mission's content.
/// The summary is updated only when `summary` is Some.
pub fn apply_snippet(mission: &mut ExportedMission, summary: &mut Option<MissionSummary>, src: &str) -> Result<(), CompileError> {
    let st = seed_state(mission, summary);
    let lua = new_lua()?;
    setup_driv3r(&lua, &st)?;
    install_getters(&lua, &st)?;
    lua.load(src).set_name("snippet").exec()?;
    drop(lua);
    let out = Rc::try_unwrap(st).map(|c| c.into_inner()).map_err(|_| CompileError::Lua("script state still referenced".into()))?;
    mission.logic.actors.definitions = out.logic.actors.definitions;
    mission.logic.nodes.definitions = out.logic.nodes.definitions;
    mission.logic.wires.collections = out.wires.into_iter().map(|w| crate::logic::WireCollection { wires: w }).collect();
    mission.objects.objects = out.objects;
    mission.logic.strings.strings = out.logic.strings.strings;
    mission.logic.sound_banks.items = out.logic.sound_banks.items;
    if let Some(sm) = summary.as_mut() {
        sm.start_position = out.summary.start_position;
        sm.city_type = out.summary.city_type;
        sm.mission_id = out.summary.mood_id;
    }
    Ok(())
}
