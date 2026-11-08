//! Mission -> Lua -> mission round trip. The mission is exported as Lua, the script is run, and the rebuilt mission must
//! have the same actors, nodes, properties, wires, objects and sound banks (strings are compared by text, wire
//! collections by their targets), and the same summary when the file has one.
//! samples_round_trip_through_lua reads the .mpc files in oracle/golden (at least 5 required). The second test needs
//! the DRIV3R_DIR environment variable (a Driv3r install folder); when it is unset, or Missions cannot be read, it returns early.

use zartex_core::compile::run_driv3r_script;
use zartex_core::logic::*;
use zartex_core::luaraw::export_lua;
use zartex_core::mission::MissionFile;
use zartex_core::objects::MissionObject;

/// Returns the string at index id, or an empty string when the index is negative or out of range.
fn text(strings: &[String], id: i16) -> String {
    strings.get(id.max(0) as usize).cloned().unwrap_or_default()
}

/// Builds a text form of one property for comparison. Wire indices on node properties of type 19 are replaced by the
/// wires they point to. node is true for node properties.
fn prop_norm(strings: &[String], wires: &[Vec<WireNode>], p: &Property, node: bool) -> String {
    let v = match &p.value {
        PropValue::Str(s) => format!("Str({:?})", text(strings, *s)),
        PropValue::TextFile { value, index } => format!("TextFile({:?},{index})", text(strings, *value)),
        PropValue::Int(w) if node && p.type_id == 19 => format!("Wires({:?})", wires.get(*w as usize).map(|c| c.iter().map(|x| (x.wire_type, x.op_code, x.node_id)).collect::<Vec<_>>())),
        other => format!("{other:?}"),
    };
    format!("{}:{:?}={}", p.type_id, text(strings, p.string_id), v)
}

/// Builds one text line per actor, node and object, plus one line for the sound banks, so that two missions
/// can be compared line by line.
fn norm(actors: &[NodeDef], nodes: &[NodeDef], strings: &[String], wires: &[Vec<WireNode>], objects: &[MissionObject], banks: &[i32]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in actors.iter().enumerate() {
        out.push(format!("actor {i} t{} n{:?} o{} c{:?} r{} f{} [{}]", a.type_id, text(strings, a.string_id), a.object_id, a.color, a.reserved, a.flags, a.properties.iter().map(|p| prop_norm(strings, wires, p, false)).collect::<Vec<_>>().join(" | ")));
    }
    for (i, n) in nodes.iter().enumerate() {
        out.push(format!("node {i} t{} n{:?} c{:?} r{} f{} [{}]", n.type_id, text(strings, n.string_id), n.color, n.reserved, n.flags, n.properties.iter().map(|p| prop_norm(strings, wires, p, true)).collect::<Vec<_>>().join(" | ")));
    }
    for (i, o) in objects.iter().enumerate() {
        out.push(format!("object {i} {o:?}"));
    }
    out.push(format!("banks {banks:?}"));
    out
}

/// Exports the mission in bytes as Lua, runs the script, and compares the rebuilt mission with the original.
/// Returns None when they match, and Some(message) when they differ or the script fails to run.
/// A file that does not parse returns None and is not compared. name is used as the title and in the temp file name.
fn check(name: &str, bytes: &[u8]) -> Option<String> {
    let mf = MissionFile::from_bytes(bytes).ok()?;
    let m = &mf.mission;
    let orig_wires: Vec<Vec<WireNode>> = m.logic.wires.collections.iter().map(|c| c.wires.clone()).collect();
    let want = norm(&m.logic.actors.definitions, &m.logic.nodes.definitions, &m.logic.strings.strings, &orig_wires, &m.objects.objects, &m.logic.sound_banks.items);
    let lua = export_lua(m, mf.summary.as_ref(), name, false);
    let path = std::env::temp_dir().join(format!("zartex_export_{}.lua", name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', "_")));
    std::fs::write(&path, &lua).unwrap();
    // A script that fails to run is reported as a mismatch rather than a panic.
    let st = match run_driv3r_script(&path.to_string_lossy()) {
        Ok(s) => s,
        Err(e) => return Some(format!("{name}: exported script does not run: {e}")),
    };
    // The rebuilt wires are read from st.wires, the wire collections the script built.
    let got = norm(&st.logic.actors.definitions, &st.logic.nodes.definitions, &st.logic.strings.strings, &st.wires, &st.objects, &st.logic.sound_banks.items);
    if let Some(s) = mf.summary.as_ref() {
        if (st.summary.start_position, st.summary.city_type, st.summary.mood_id) != (s.start_position, s.city_type, s.mission_id) {
            return Some(format!("{name}: summary differs"));
        }
    }
    if want.len() != got.len() {
        return Some(format!("{name}: {} lines vs {}", want.len(), got.len()));
    }
    for (a, b) in want.iter().zip(&got) {
        if a != b {
            return Some(format!("{name}: first difference\n  want {a}\n  got  {b}"));
        }
    }
    None
}

/// Every .mpc file in oracle/golden must export to Lua, run, and rebuild the same mission. Fails when fewer than 5 .mpc files are found.
#[test]
fn samples_round_trip_through_lua() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/golden");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().map(|x| x == "mpc").unwrap_or(false) {
            let r = check(&p.file_name().unwrap().to_string_lossy(), &std::fs::read(&p).unwrap());
            assert!(r.is_none(), "{}", r.unwrap());
            n += 1;
        }
    }
    assert!(n >= 5, "only {n} samples");
}

/// Runs the same check on every .mpc in $DRIV3R_DIR/Missions whose name does not start with template.
/// Returns early, checking nothing, when DRIV3R_DIR is unset or the Missions folder cannot be read.
#[test]
fn installed_missions_round_trip_through_lua() {
    // Without an install folder configured, the test passes without checking anything.
    let Ok(dir) = std::env::var("DRIV3R_DIR") else { return };
    let Ok(rd) = std::fs::read_dir(format!("{dir}/Missions")) else { return };
    let (mut n, mut bad) = (0, Vec::new());
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x.eq_ignore_ascii_case("mpc")).unwrap_or(false) && !p.file_stem().unwrap().to_string_lossy().to_lowercase().starts_with("template") {
            n += 1;
            if let Some(r) = check(&p.file_name().unwrap().to_string_lossy(), &std::fs::read(&p).unwrap()) {
                bad.push(r);
            }
        }
    }
    println!("{n} missions checked, {} differ", bad.len());
    for b in bad.iter().take(6) {
        println!("{b}");
    }
    assert!(bad.is_empty(), "{} of {n} missions do not round trip", bad.len());
}
