//! Smoke tests for the part b Lua API methods (see ../PORTING_BRIEF.md). Each test runs a short Lua script, or calls a
//! Rust helper, and checks the actors, nodes, wires, objects, sound banks and actor setup that the script creates.
//! The import test reads assets/template.mpc from the crate directory. No game files are needed.
use std::cell::RefCell;
use std::rc::Rc;
use zartex_core::api_part_b::*;
use zartex_core::compile::{new_lua, setup_driv3r};
use zartex_core::luaapi::State;
use zartex_core::mission::MissionFile;
use zartex_core::objects::MissionObject;
use zartex_core::script::ScriptState;

/// Runs Lua source src in a fresh script state with the MISSION API set up. dir is the working directory for relative
/// file names. Returns the state. A Lua error in src panics.
fn run(dir: &str, src: &str) -> State {
    let st: State = Rc::new(RefCell::new(ScriptState::new(dir)));
    let lua = new_lua().unwrap();
    setup_driv3r(&lua, &st).unwrap();
    lua.load(src).exec().unwrap();
    drop(lua);
    st
}

const CHAR_A: &str = r#"MISSION.CreateCharacter(1, 2, 3, 0, 5, 1, "Bob", 0, 0, 100, 0, 0, 0, "Hero", 0, 0, 0, 0)"#;
const CHAR_B: &str = r#"MISSION.CreateCharacter(4, 5, 6, 0, 5, 0, "Ann", 0, 0, 100, 0, 0, 0, "Civ", 0, 0, 0, 0)"#;

/// Returns Lua source that creates the two characters a and b, followed by extra.
fn two_chars(extra: &str) -> String {
    format!("local a = {CHAR_A}\nlocal b = {CHAR_B}\n{extra}")
}

/// MISSION.toflag reads four bytes as a little endian 32-bit integer. The Rust helper toflag is checked the same way.
#[test]
fn toflag_is_little_endian_bitconverter() {
    let st = run("", "assert(MISSION.toflag(1, 0, 0, 0) == 1)\nassert(MISSION.toflag(0, 0, 1, 0) == 65536)\nassert(MISSION.toflag(255, 255, 255, 255) == -1)");
    assert_eq!(st.borrow().logic.actors.definitions.len(), 0);
    assert_eq!(toflag(0x78, 0x56, 0x34, 0x12), 0x12345678);
}

/// CreateActorSetting builds an actor setting from a table of actor indices. The setting is added to actorSetup
/// once with the colon call and once with the dot call.
#[test]
fn create_actor_setting_and_actor_setup_add() {
    let st = run("", &two_chars("local s = MISSION.CreateActorSetting({[1] = true, [2] = true})\nMISSION.actorSetup:Add(s)\nMISSION.actorSetup.Add(MISSION.actorSetup, s)"));
    let s = st.borrow();
    assert_eq!(s.actor_setup, vec![vec![0, 1], vec![0, 1]]);
}

/// CreateActorSetting with an index that names no actor must raise a Lua error. No actor is created in this test.
#[test]
fn create_actor_setting_rejects_unknown_actor() {
    let st: State = Rc::new(RefCell::new(ScriptState::new("")));
    let lua = new_lua().unwrap();
    setup_driv3r(&lua, &st).unwrap();
    assert!(lua.load("MISSION.CreateActorSetting({[1] = true})").exec().is_err());
}

/// CreateWireCollection adds one wire collection, and AddSoundBankToLoadList appends bank numbers in call order.
#[test]
fn create_wire_collection_and_sound_bank() {
    let st = run("", "local w = MISSION.CreateWireCollection(4)\nMISSION.AddSoundBankToLoadList(7)\nMISSION.AddSoundBankToLoadList(9)");
    let s = st.borrow();
    assert_eq!(s.wires.len(), 1);
    assert_eq!(s.logic.sound_banks.items, vec![7, 9]);
}

/// GetActorById takes a 1-based id and returns the 0-based index. With two actors, ids 0 and 3 are rejected.
#[test]
fn get_actor_by_id_checks_range() {
    let st = run("", &two_chars("assert(MISSION.GetActorById(2) ~= nil)\nassert(not pcall(MISSION.GetActorById, 3))\nassert(not pcall(MISSION.GetActorById, 0))"));
    let s = st.borrow();
    assert_eq!(get_actor_by_id(&s, 2).unwrap(), 1);
    assert!(get_actor_by_id(&s, 3).is_err());
}

/// SetActorUID stores the uid. SetActorObjectPositionXYZA writes the position and a heading in degrees into the
/// creation data of the actor object.
#[test]
fn set_actor_uid_and_position() {
    let st = run("", &two_chars("MISSION.SetActorUID(a, 42)\nMISSION.SetActorObjectPositionXYZA(a, 10, 20, 30, 90)\nMISSION.SetActorObjectPositionXYZA(b, 1, 1, 1, 0)"));
    let s = st.borrow();
    match &s.objects[0] {
        MissionObject::Character { creation_data, position, uid, .. } => {
            assert_eq!(*uid, 42);
            assert_eq!((position.x, position.y, position.z), (10.0, 20.0, 30.0));
            assert_eq!(creation_data.len(), 28);
            assert_eq!(&creation_data[0x4..0x8], &10.0f32.to_le_bytes());
            assert_eq!(&creation_data[0x8..0xC], &20.0f32.to_le_bytes());
            // forward = (cos a, 0, sin a) with a = 90 degrees in f64
            let a = (std::f64::consts::PI / 180.0) * 90.0;
            assert_eq!(&creation_data[0x10..0x14], &(zartex_core::script::X87::xcos(a) as f32).to_le_bytes());
            assert_eq!(&creation_data[0x14..0x18], &0.0f32.to_le_bytes());
            assert_eq!(&creation_data[0x18..0x1C], &(zartex_core::script::X87::xsin(a) as f32).to_le_bytes());
        }
        other => panic!("unexpected object {other:?}"),
    }
}

/// FindPlayerActor returns the first character whose role is 1. GetLogicStart returns the node of type 1, and
/// GetNodeById rejects an id that has no node.
#[test]
fn find_player_actor_and_logic_start() {
    let st = run("", &two_chars("assert(MISSION.FindPlayerActor() ~= nil)\nlocal l = MISSION.LogicStart()\nassert(MISSION.GetLogicStart() ~= nil)\nassert(MISSION.GetNodeById(1) ~= nil)\nassert(not pcall(MISSION.GetNodeById, 99))"));
    let s = st.borrow();
    assert_eq!(find_player_actor(&s).unwrap(), Some(0));
    assert_eq!(get_logic_start(&s).unwrap().map(|x| x.0), Some(0));
    assert!(get_node_by_id(&s, 2).is_err());
}

/// SetChaseVehicle has two overloads. With a numeric fourth argument the first overload is used, which adds one
/// property to the node. With three arguments the second overload is used. The first created node has type id 137.
#[test]
fn set_chase_vehicle_both_overloads() {
    let st = run("", &two_chars("MISSION.SetChaseVehicle(a, 50, 3, 1700)\nMISSION.SetChaseVehicle(a, 50, 3)"));
    let s = st.borrow();
    let n = &s.logic.nodes.definitions;
    assert_eq!(n.len(), 2);
    assert_eq!(n[0].type_id, 137);
    assert_eq!(n[0].properties.len(), 8); // numeric 4th arg -> overload 1 (writes a second pWireCollection actor prop)
    assert_eq!(n[1].properties.len(), 7); // 3 args -> overload 2 (MoonSharp tie-break, proven against the oracle)
    assert_eq!(s.wires.len(), 2);
}

/// SetCharacterPositionTo has two overloads. The one given a position and an angle creates a marker actor and object.
/// The one given another actor does not.
#[test]
fn set_character_position_to_both_overloads() {
    let st = run("", &two_chars("MISSION.SetCharacterPositionTo(a, 1, 2, 3, 90)\nMISSION.SetCharacterPositionTo(a, b)"));
    let s = st.borrow();
    // overload A creates a marker actor and object, overload B does not
    assert_eq!(s.logic.actors.definitions.len(), 3);
    assert_eq!(s.objects.len(), 3);
    assert!(matches!(s.objects[2], MissionObject::Marker { .. }));
    let n = &s.logic.nodes.definitions;
    assert_eq!(n.len(), 2);
    assert_eq!(n[0].type_id, 18);
    assert_eq!(n[0].properties.len(), 12);
    assert_eq!(n[1].properties.len(), 12);
}

/// Both overloads of SetCharacterForceGetOutVehicleTo create a type 18 node, with value 17 in property index 5.
#[test]
fn set_character_force_get_out_vehicle_to_both_overloads() {
    let st = run("", &two_chars("MISSION.SetCharacterForceGetOutVehicleTo(a, 1, 2, 3)\nMISSION.SetCharacterForceGetOutVehicleTo(a, b)"));
    let s = st.borrow();
    assert_eq!(s.logic.actors.definitions.len(), 3);
    assert_eq!(s.logic.nodes.definitions.len(), 2);
    // CharacterControl always creates a type 18 node; the 17 goes into pProperty
    for n in &s.logic.nodes.definitions {
        assert_eq!(n.type_id, 18);
    }
    assert_eq!(s.logic.nodes.definitions[0].properties[5].value, zartex_core::logic::PropValue::Int(17));
    assert_eq!(s.logic.nodes.definitions[1].properties[5].value, zartex_core::logic::PropValue::Int(17));
}

/// ImportMissionScriptFromFile loads assets/template.mpc by a relative name, and the counts of nodes, actors, objects
/// and wire collections match the parsed template. A missing file name or an empty name raises an error.
#[test]
fn import_mission_script_from_file() {
    let dir = format!("{}/assets", env!("CARGO_MANIFEST_DIR"));
    let template = std::fs::read(format!("{dir}/template.mpc")).unwrap();
    let expected = MissionFile::from_bytes(&template).unwrap();
    let st = run(&dir, r#"assert(MISSION.ImportMissionScriptFromFile("template.mpc") == true)"#);
    let s = st.borrow();
    assert_eq!(s.logic.nodes.definitions.len(), expected.mission.logic.nodes.definitions.len());
    assert_eq!(s.logic.actors.definitions.len(), expected.mission.logic.actors.definitions.len());
    assert_eq!(s.objects.len(), expected.mission.objects.objects.len());
    assert_eq!(s.wires.len(), expected.mission.logic.wires.collections.len());
    drop(s);
    // A failed import must leave the new state with no objects.
    let st2 = run(&dir, r#"assert(not pcall(MISSION.ImportMissionScriptFromFile, "missing.mpc"))
        assert(not pcall(MISSION.ImportMissionScriptFromFile, ""))"#);
    assert_eq!(st2.borrow().objects.len(), 0);
}
