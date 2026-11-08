//! Smoke test for api_part_c: each ported LuaMissionScript method is called once from a Lua script, with valid arguments.
//! Checks the node type ids, actor count, object count and sound bank count that the calls create. No game files are needed.
use std::cell::RefCell;
use std::rc::Rc;
use zartex_core::compile::{new_lua, setup_driv3r};
use zartex_core::luaapi::{ret_actor, State};
use zartex_core::script::ScriptState;

const SCRIPT: &str = r#"
    ch = MISSION.CreateCharacter(0, 0, 0, 0, 1, 0, "Pers", 0, 0, 100, 0, 0, 0)
    MISSION.CharacterControl(ch, 1, 5.0)
    MISSION.CharacterNameControl(ch, "Bob")
    MISSION.SetCharacterTakeHealth(ch, 10)
    MISSION.SetCharacterAddHealth(ch, 10)
    MISSION.SetCharacterHealthTo(ch, 50)
    MISSION.SetCharacterAddFelony(ch, 1)
    MISSION.SetCharacterTakeFelony(ch, 1)
    MISSION.SetCharacterFelonyTo(ch, 2)
    MISSION.SetCharacterVulnerability(ch, 0.5)
    MISSION.SetCharacterUsePadFile(ch, nil, "f.pad", 2)
    MISSION.GiveCharacterWeapon(ch, 3, 2.0)
    MISSION.SetCharacterUnStuckInVehicle(ch)
    MISSION.SetCharacterStuckInVehicle(ch)
    MISSION.SetCharacterStopAnimation(ch)
    MISSION.SetCharacterPlayAnimation(ch, "anim", 2)
    MISSION.SetCharacterPersonalityIndexTo(ch, 3)
    MISSION.ClearCharacterWeapons(ch)
    MISSION.SetCharacterGetOutOfVehicle(ch)
    MISSION.CharacterEquipWeapon(ch)
    MISSION.CharacterReholsterWeapon(ch)
    MISSION.FadeScreen(1.0, 0.5)
    MISSION.FadeOut()
    MISSION.FadeIn()
    MISSION.BombCar(veh)
    MISSION.ActorHasReachedPoint(ch, 1, 2, 3, 4)
    MISSION.PlayAudio(1, 2)
    MISSION.CharacterDriveTo(ch, 1, 2, 3, 5)
    MISSION.CharacterDriveAroundArea(ch, area, 5)
    MISSION.CharacterChaseActor(ch, veh, 5)
    MISSION.CharacterDriveFollowingActor(ch, veh, 5)
    MISSION.ShowClock(30)
    MISSION.StartClock()
"#;

/// Runs SCRIPT, which calls the ported character and clock methods, and checks the node type ids in call order,
/// the actor, object and sound bank counts.
#[test]
fn ported_character_and_clock_methods_create_their_nodes() {
    let st: State = Rc::new(RefCell::new(ScriptState::new("")));
    let lua = new_lua().unwrap();
    setup_driv3r(&lua, &st).unwrap();

    // a vehicle and an area actor for the calls that need them
    let (veh, area) = {
        let mut s = st.borrow_mut();
        let v = s.create_actor(3, None, [127, 127, 127, 255], 0, 0, |_| Vec::new());
        let a = s.create_actor(4, None, [127, 127, 127, 255], 0, 0, |_| Vec::new());
        (v, a)
    };
    lua.globals().set("veh", ret_actor(&lua, &st, veh).unwrap()).unwrap();
    lua.globals().set("area", ret_actor(&lua, &st, area).unwrap()).unwrap();

    lua.load(SCRIPT).exec().unwrap();

    let s = st.borrow();
    // 32 calls, each adds exactly one node (the ActorCreation node for the CharacterDrive* family)
    assert_eq!(s.logic.nodes.definitions.len(), 32, "node count");
    assert_eq!(s.logic.nodes.definitions[0].type_id, 18, "CharacterControl type id");
    assert_eq!(s.logic.nodes.definitions[1].type_id, 107, "CharacterNameControl type id");
    assert_eq!(s.logic.nodes.definitions[20].type_id, 124, "FadeScreen type id");
    assert_eq!(s.logic.nodes.definitions[21].type_id, 124, "FadeOut type id");
    assert_eq!(s.logic.nodes.definitions[22].type_id, 124, "FadeIn type id");
    assert_eq!(s.logic.nodes.definitions[23].type_id, 122, "BombCar type id");
    assert_eq!(s.logic.nodes.definitions[24].type_id, 12, "ActorHasReachedPoint type id");
    assert_eq!(s.logic.nodes.definitions[25].type_id, 27, "PlayAudio type id");
    assert_eq!(s.logic.nodes.definitions[26].type_id, 101, "CharacterDriveTo ActorCreation node");
    assert_eq!(s.logic.nodes.definitions[30].type_id, 28, "ShowClock type id");
    assert_eq!(s.logic.nodes.definitions[31].type_id, 28, "StartClock type id");

    // 3 pre-made actors, plus the AI target of each CharacterDrive*/Chase call (4 of them), plus the
    // test volume of ActorHasReachedPoint
    assert_eq!(s.logic.actors.definitions.len(), 3 + 5, "actor count");
    // the character, plus one volume for ActorHasReachedPoint and one per CharacterDriveTo/Chase/Following
    assert_eq!(s.objects.len(), 1 + 1 + 3, "object count");
    // BombCar (bank 45) and PlayAudio (bank 1) each register their bank
    assert_eq!(s.logic.sound_banks.items.len(), 2, "sound banks");
}
