//! Smoke test for the part-a actor creators (api_part_a.rs): every method runs from Lua with valid
//! arguments and creates the expected actors/objects.
//! No game files are needed: the script runs in memory on a state made with ScriptState::new("").
use std::cell::RefCell;
use std::rc::Rc;
use zartex_core::compile::{new_lua, setup_driv3r};
use zartex_core::luaapi::State;
use zartex_core::objects::MissionObject;
use zartex_core::script::ScriptState;

const SCRIPT: &str = r#"
local char = MISSION.CreateCharacter(1, 2, 3, 0, 5, 0, "Hero", 0, 0, 100, 0, 0, 100, "", 131073, 0, 155, 200)
MISSION.CreateAIPath(char, {{0, 0, 0}, {1, 0, 0}, {2, 0, 1}}, 1.5, 0, 2, 1, "path")
MISSION.TestVolume(1, 2, 3)
MISSION.Marker(1, 2, 3, 4, 5, 6)
MISSION.CreateArea(1, 2, 3, 4, 5, 6)
MISSION.GetAnimatedObjectAt(1, 2, 3, 4)
MISSION.CreateSwitch(1, 2, 3, 90)
MISSION.AITarget(char, 1, 2, 3, 1.0, 2.0, 1)
MISSION.SpecialEffect(1, 2, 3, 2, 5.0, 10.0, 1000)
MISSION.CreateObjectiveIcon(1, 2, 3)
-- overload 1: (actor, height, maxAdaptiveHeight, type, canGoUp, ...)
MISSION.CreateObjectiveIconAttachedTo(char, 1, 0, 2, true)
-- overload 2: (actor, x, height, z, maxAdaptiveHeight, type, canGoUp, ...)
MISSION.CreateObjectiveIconAttachedTo(char, 0.0, 2, 0.0, 0, 0, true)
local veh = MISSION.CreateVehicle(10, 20, 30, 45, 7, 3)
MISSION.CreateCollectable(1, 2, 3, 1, 5)
MISSION.CreateCharacterInVehicle(veh, 9, 0, "Driver", 1, 0, 100, 0, 0, 100, 0.5, 0.25, 0.0, 2)
MISSION.CreateCamera(0, 0, 0, 10, 20, 30)
MISSION.CreateCameraLookingAtPosition(0, 0, 0, 1, 2, 3)
"#;

/// Runs the Lua SCRIPT above, which calls each creator once, and checks the actor type ids, the mission objects
/// and the spool vehicle list that the calls produce.
#[test]
fn part_a_creators_run_and_create_expected_definitions() {
    let st: State = Rc::new(RefCell::new(ScriptState::new("")));
    let lua = new_lua().unwrap();
    setup_driv3r(&lua, &st).unwrap();
    lua.load(SCRIPT).exec().unwrap();

    let s = st.borrow();
    // 1 character + 16 creators, the camera-looking-at call adds a target volume too
    let expected = 18;
    assert_eq!(s.logic.actors.definitions.len(), expected, "actor definitions");
    assert_eq!(s.objects.len(), expected, "mission objects");
    assert_eq!(s.spool_vehicles, vec![7], "spool_vehicles");

    // The expected ids are the type ids the creators write, listed in the order the actors are created.
    let type_ids: Vec<u8> = s.logic.actors.definitions.iter().map(|d| d.type_id).collect();
    assert_eq!(type_ids, vec![2, 6, 4, 105, 4, 104, 101, 7, 8, 5, 5, 5, 3, 103, 2, 9, 4, 9]);

    // CreateAIPath: 3 path points with w = 1, creation data 32 bytes, character index 0
    match &s.objects[1] {
        MissionObject::Path { path, .. } => {
            assert_eq!(path.len(), 3);
            assert_eq!(path[2].x, 2.0);
            assert_eq!(path[2].z, 1.0);
            assert_eq!(path[2].w, 1.0);
        }
        other => panic!("expected path, got {other:?}"),
    }

    // CreateVehicle: 108 creation bytes, position at offset 44 is X = 10
    let vehicle = match &s.objects[12] {
        MissionObject::Vehicle { creation_data, position, uid } => {
            assert_eq!(creation_data.len(), 108);
            assert_eq!(&creation_data[44..48], &10.0f32.to_le_bytes());
            assert_eq!(*uid, 7);
            assert_eq!(position.y, 20.0);
            (creation_data.clone(), *position)
        }
        other => panic!("expected vehicle, got {other:?}"),
    };
    let _ = vehicle;

    // CreateCharacterInVehicle (actor 14): creation data replaced, uid = vehicle actor ObjectId (12)
    let char_actor = &s.logic.actors.definitions[14];
    match &s.objects[char_actor.object_id as usize] {
        MissionObject::Character { creation_data, uid, .. } => {
            assert_eq!(&creation_data[0..4], &[1u8, 0, 1, 13]);
            assert_eq!(creation_data.len(), 4 + 12 + 4);
            assert_eq!(creation_data[16], 2);
            assert_eq!(*uid, s.logic.actors.definitions[12].object_id);
        }
        other => panic!("expected character, got {other:?}"),
    }

    // CreateObjectiveIconAttachedTo overload 1 wrote the actor index into pAttachTo
    assert_eq!(s.logic.actors.definitions[10].properties[0].type_id, 7);
    assert_eq!(s.logic.actors.definitions[11].properties[0].type_id, 7);
}
