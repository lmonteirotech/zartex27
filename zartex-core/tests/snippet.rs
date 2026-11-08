//! Lua snippets run on an existing mission: new nodes are appended and can be wired to existing ones.
//! Uses oracle/golden/survival.mpc, which must exist. No game files are needed.
use zartex_core::compile::apply_snippet;
use zartex_core::mission::MissionFile;

/// apply_snippet on survival.mpc appends one actor and two nodes, wires them to existing nodes, keeps the result
/// loadable, and leaves the mission unchanged when the snippet fails.
#[test]
fn snippet_appends_and_wires() {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/golden/survival.mpc")).unwrap();
    let mut mf = MissionFile::from_bytes(&bytes).unwrap();
    let (n0, a0, w0) = (mf.mission.logic.nodes.definitions.len(), mf.mission.logic.actors.definitions.len(), mf.mission.logic.wires.collections.len());
    let first_out = mf.mission.logic.wires.collections.iter().map(|c| c.wires.len()).sum::<usize>();
    // The snippet creates a character, a timer and a CharacterHasDied node. The node returned by MISSION.GetNode(0)
    // is wired to the timer, and the timer is wired to the death node.
    apply_snippet(
        &mut mf.mission,
        &mut mf.summary,
        "local p = MISSION.CreateCharacter(10,2,30,0, SKIN_TANNER, 1, 'Extra', -1,-1,1.0,0.0,0,1.0)\nlocal t = MISSION.Timer(7)\nMISSION.GetNode(0).wireCollection.Add(t)\nt.wireCollection.Add(MISSION.CharacterHasDied(p))",
    )
    .unwrap();
    assert_eq!(mf.mission.logic.actors.definitions.len(), a0 + 1);
    assert_eq!(mf.mission.logic.nodes.definitions.len(), n0 + 2);
    assert!(mf.mission.logic.wires.collections.len() >= w0 + 2);
    assert!(mf.mission.logic.wires.collections.iter().map(|c| c.wires.len()).sum::<usize>() >= first_out + 2);
    // the result still saves and reloads
    let out = mf.to_bytes().unwrap();
    let back = MissionFile::from_bytes(&out).unwrap();
    assert_eq!(back.mission.logic.nodes.definitions.len(), n0 + 2);
    // a bad snippet leaves the mission untouched
    // A snippet that fails must not change the mission it was applied to.
    let mut mf2 = MissionFile::from_bytes(&bytes).unwrap();
    assert!(apply_snippet(&mut mf2.mission, &mut mf2.summary, "MISSION.GetNode(9999)").is_err());
    assert_eq!(mf2.mission.logic.nodes.definitions.len(), n0);
}
