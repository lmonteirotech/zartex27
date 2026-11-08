//! Model-level round trip: each golden Driv3r mission in oracle/golden is parsed into MissionFile and written back
//! with to_bytes, and the bytes must equal the golden file. A further test checks that three golden missions are not empty.
//! Needs the files in oracle/golden and assets/template.mpc. No game files are needed.
use std::path::PathBuf;
use zartex_core::mission::*;

const DRIV3R: [&str; 7] = [
    "template.mpc", "deathmatch.mpc", "presidentsrun.mpc", "scriptedTraffic.mpc",
    "survival.mpc", "areatest.mpc", "checkpointRace.mpc",
];

/// Reads a file from oracle/golden, relative to the crate directory.
fn golden(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/golden").join(name)).unwrap()
}

/// Every mission in DRIV3R must load, save to identical bytes, and all failures are reported together.
#[test]
fn model_roundtrip_driv3r_golden() {
    let mut failed = vec![];
    for n in DRIV3R {
        let g = golden(n);
        let mut mf = match MissionFile::from_bytes(&g) {
            Ok(m) => m,
            Err(e) => {
                failed.push(format!("{n}: load: {e}"));
                continue;
            }
        };
        match mf.to_bytes() {
            Ok(out) if out == g => {}
            Ok(out) => {
                let at = out.iter().zip(&g).position(|(a, b)| a != b);
                failed.push(format!("{n}: bytes differ (len {} vs {}, first diff {:?})", out.len(), g.len(), at));
            }
            Err(e) => failed.push(format!("{n}: save: {e}")),
        }
    }
    assert!(failed.is_empty(), "{:#?}", failed);
}

/// The bundled assets/template.mpc must round trip byte for byte through MissionFile.
#[test]
fn template_model_roundtrip() {
    let t = include_bytes!("../assets/template.mpc");
    let mut mf = MissionFile::from_bytes(t).unwrap();
    assert_eq!(mf.to_bytes().unwrap(), t.to_vec());
}

/// Checks that three golden missions have actors, nodes and objects, and prints their counts.
#[test]
fn golden_models_are_not_trivially_empty() {
    for n in ["deathmatch.mpc", "presidentsrun.mpc", "scriptedTraffic.mpc"] {
        let mf = MissionFile::from_bytes(&golden(n)).unwrap();
        let l = &mf.mission.logic;
        println!("{n}: actors={} nodes={} strings={} wires={} objects={} props(nodes)={}",
            l.actors.definitions.len(), l.nodes.definitions.len(), l.strings.strings.len(),
            l.wires.collections.len(), mf.mission.objects.objects.len(),
            l.nodes.definitions.iter().map(|d| d.properties.len()).sum::<usize>());
        assert!(!l.actors.definitions.is_empty() && !l.nodes.definitions.is_empty());
        assert!(!mf.mission.objects.objects.is_empty());
    }
}

/// Ignored by default. Prints the node and actor color fields, the string table and the summary of template.mpc,
/// for manual inspection.
#[test]
#[ignore]
fn print_colors() {
    let mf = MissionFile::from_bytes(&golden("template.mpc")).unwrap();
    for d in &mf.mission.logic.nodes.definitions { println!("node type={} str={} color={:?} flags={} props={}", d.type_id, d.string_id, d.color, d.flags, d.properties.len()); }
    for d in &mf.mission.logic.actors.definitions { println!("actor type={} str={} color={:?} flags={} obj={}", d.type_id, d.string_id, d.color, d.flags, d.object_id); }
    println!("{:?}", mf.mission.logic.strings.strings);
    println!("{:?}", mf.summary);
}
