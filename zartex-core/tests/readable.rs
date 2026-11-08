//! Mission -> readable Lua -> mission: the script written with the API calls must rebuild the same actors (each with the
//! object it uses), nodes, wires, unused objects, sound banks and summary. Reports how much came out as readable calls.
//! The samples test reads the .mpc files in oracle/golden (at least 5 required). The installed-missions test needs the
//! DRIV3R_DIR environment variable (a Driv3r install folder) and returns early when it is not set.

use zartex_core::compile::run_driv3r_script;
use zartex_core::logic::*;
use zartex_core::mission::MissionFile;
use zartex_core::objects::MissionObject;
use zartex_core::readable::{export_readable, Stats};

/// Returns the string at index id, or an empty string when the index is negative or out of range.
fn text(strings: &[String], id: i16) -> String {
    strings.get(id.max(0) as usize).cloned().unwrap_or_default()
}

/// Builds a text form of one property for comparison. The name is lower-cased, so name comparison ignores case.
/// Wire indices on node properties of type 19 are replaced by the wires they point to. node is true for node properties.
fn prop_norm(strings: &[String], wires: &[Vec<WireNode>], p: &Property, node: bool) -> String {
    let v = match &p.value {
        PropValue::Str(s) => format!("Str({:?})", text(strings, *s)),
        PropValue::TextFile { value, index } => format!("TextFile({:?},{index})", text(strings, *value)),
        PropValue::Int(w) if node && p.type_id == 19 => format!("Wires({:?})", wires.get(*w as usize).map(|c| c.iter().map(|x| (x.wire_type, x.op_code, x.node_id)).collect::<Vec<_>>())),
        other => format!("{other:?}"),
    };
    format!("{:?}={}", text(strings, p.string_id).to_lowercase(), v)
}

/// Builds text lines for actors, nodes, orphan objects and sound banks. An object is an orphan when no actor uses it.
/// Orphan lines are sorted before they are added.
fn norm(actors: &[NodeDef], nodes: &[NodeDef], strings: &[String], wires: &[Vec<WireNode>], objects: &[MissionObject], banks: &[i32]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in actors.iter().enumerate() {
        let obj = if a.object_id >= 0 { objects.get(a.object_id as usize).map(|o| format!("{}", o.type_id())).unwrap_or("invalid".into()) } else { "none".into() };
        out.push(format!("actor {i} t{} n{:?} c{:?} obj {obj} [{}]", a.type_id, text(strings, a.string_id), &a.color[..3], a.properties.iter().map(|p| prop_norm(strings, wires, p, false)).collect::<Vec<_>>().join(" | ")));
    }
    for (i, n) in nodes.iter().enumerate() {
        out.push(format!("node {i} t{} n{:?} c{:?} [{}]", n.type_id, text(strings, n.string_id), &n.color[..3], n.properties.iter().map(|p| prop_norm(strings, wires, p, true)).collect::<Vec<_>>().join(" | ")));
    }
    let used: std::collections::HashSet<i32> = actors.iter().filter(|a| a.object_id >= 0).map(|a| a.object_id).collect();
    let mut orphans: Vec<String> = objects.iter().enumerate().filter(|(i, _)| !used.contains(&(*i as i32))).map(|(_, o)| format!("orphan {o:?}")).collect();
    orphans.sort();
    out.extend(orphans);
    out.push(format!("banks {banks:?}"));
    out
}

/// Exports one mission with export_readable, runs the script, and compares the rebuilt mission with the original.
/// The export statistics are added to total. Returns None on a match, or Some(message) on a mismatch or a script error.
/// A file that does not parse returns None and is not compared.
fn check(name: &str, bytes: &[u8], total: &mut Stats) -> Option<String> {
    let mf = MissionFile::from_bytes(bytes).ok()?;
    let m = &mf.mission;
    let orig_wires: Vec<Vec<WireNode>> = m.logic.wires.collections.iter().map(|c| c.wires.clone()).collect();
    let want = norm(&m.logic.actors.definitions, &m.logic.nodes.definitions, &m.logic.strings.strings, &orig_wires, &m.objects.objects, &m.logic.sound_banks.items);
    // Statistics are summed over all missions, for the line printed by report.
    let (lua, stats) = export_readable(m, mf.summary.as_ref(), name, false);
    total.actors += stats.actors;
    total.actors_readable += stats.actors_readable;
    total.nodes += stats.nodes;
    total.nodes_readable += stats.nodes_readable;
    total.normalized += stats.normalized;
    for (k, v) in stats.raw_kinds {
        *total.raw_kinds.entry(k).or_default() += v;
    }
    let path = std::env::temp_dir().join(format!("zartex_readable_{}.lua", name.replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', "_")));
    std::fs::write(&path, &lua).unwrap();
    // A script that fails to run is reported as a mismatch rather than a panic.
    let st = match run_driv3r_script(&path.to_string_lossy()) {
        Ok(s) => s,
        Err(e) => return Some(format!("{name}: exported script does not run: {e} ({})", path.display())),
    };
    let got = norm(&st.logic.actors.definitions, &st.logic.nodes.definitions, &st.logic.strings.strings, &st.wires, &st.objects, &st.logic.sound_banks.items);
    if let Some(s) = mf.summary.as_ref() {
        if (st.summary.start_position, st.summary.city_type, st.summary.mood_id) != (s.start_position, s.city_type, s.mission_id) {
            return Some(format!("{name}: summary differs ({})", path.display()));
        }
    }
    if want.len() != got.len() {
        return Some(format!("{name}: {} lines vs {} ({})", want.len(), got.len(), path.display()));
    }
    for (a, b) in want.iter().zip(&got) {
        if a != b {
            return Some(format!("{name}: first difference ({})\n  want {a}\n  got  {b}", path.display()));
        }
    }
    None
}

/// Prints the raw-call counts per kind (largest first, at most 25) and the share of actors and nodes written as
/// readable calls. what labels the line, and n is the number of missions.
fn report(what: &str, n: usize, t: &Stats) {
    let mut kinds: Vec<_> = t.raw_kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1));
    println!("written raw, by kind: {}", kinds.iter().take(25).map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
    let pct = |a: usize, b: usize| if b == 0 { 100.0 } else { a as f64 * 100.0 / b as f64 };
    println!("{what}: {n} missions; actors readable {}/{} ({:.1}%), nodes readable {}/{} ({:.1}%), {} with flags/alpha/type ids normalised", t.actors_readable, t.actors, pct(t.actors_readable, t.actors), t.nodes_readable, t.nodes, pct(t.nodes_readable, t.nodes), t.normalized);
}

/// Every .mpc in oracle/golden must round trip through readable Lua. Fails when fewer than 5 .mpc files are found.
#[test]
fn samples_round_trip_through_readable_lua() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/golden");
    let (mut n, mut total) = (0, Stats::default());
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().map(|x| x == "mpc").unwrap_or(false) {
            let r = check(&p.file_name().unwrap().to_string_lossy(), &std::fs::read(&p).unwrap(), &mut total);
            assert!(r.is_none(), "{}", r.unwrap());
            n += 1;
        }
    }
    report("samples", n, &total);
    assert!(n >= 5, "only {n} samples");
}

/// Runs the same check on every .mpc in $DRIV3R_DIR/Missions whose name does not start with template.
/// Returns early, checking nothing, when DRIV3R_DIR is unset or the Missions folder cannot be read.
#[test]
fn installed_missions_round_trip_through_readable_lua() {
    // Without an install folder configured, the test passes without checking anything.
    let Ok(dir) = std::env::var("DRIV3R_DIR") else { return };
    let Ok(rd) = std::fs::read_dir(format!("{dir}/Missions")) else { return };
    let (mut n, mut bad, mut total) = (0, Vec::new(), Stats::default());
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().map(|x| x.eq_ignore_ascii_case("mpc")).unwrap_or(false) && !p.file_stem().unwrap().to_string_lossy().to_lowercase().starts_with("template") {
            n += 1;
            if let Some(r) = check(&p.file_name().unwrap().to_string_lossy(), &std::fs::read(&p).unwrap(), &mut total) {
                bad.push(r);
            }
        }
    }
    report("installed", n, &total);
    for b in bad.iter().take(6) {
        println!("{b}");
    }
    assert!(bad.is_empty(), "{} of {n} missions do not round trip", bad.len());
}
