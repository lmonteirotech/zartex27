//! Default arguments for the "Add to mission" dialog, and a test that every creator runs with them.
//!
//! What it does: `default_args` writes the argument list of a creator call (`MISSION.<name>(...)`): actor parameters
//! take an existing actor of a matching kind, other parameters their default value. The tests run every creator on
//! a sample mission and check the results (the sample is `oracle/golden/survival.mpc`, read from the repository).
//! Uses: `creators` (`Creator`, `CREATORS`), `model` (`Project`, `Edit`), `zartex_core` (`compile`, `mission`, `logic`,
//! `nodenames`). Used by: `main.rs` (the Add to mission dialog).

use crate::creators::Creator;

/// Default arguments for a creator: actor parameters get an existing actor of the kind the parameter name suggests
/// (`actors` = index and type name of the mission's actors), so the call is valid as it stands.
pub fn default_args(c: &Creator, actors: &[(usize, String)]) -> String {
    // index of an actor for a parameter: the first actor whose type name contains a word the parameter name suggests
    // (for example "veh", "car" -> vehicle); the first actor of the mission when none matches, 0 when there is none
    let pick = |name: &str| -> usize {
        let n = name.to_lowercase();
        let want: &[&str] = if n.contains("char") || n.contains("player") || n.contains("ped") || n.contains("driver") || n.contains("target") || n.contains("chaser") {
            &["character", "cop"]
        } else if n.contains("veh") || n.contains("car") || n.contains("truck") || n.contains("bike") || n.contains("boat") {
            &["vehicle"]
        } else if n.contains("area") {
            &["area"]
        } else if n.contains("volume") || n.contains("zone") {
            &["volume"]
        } else if n.contains("cam") {
            &["camera"]
        } else if n.contains("path") {
            &["path"]
        } else if n.contains("prop") || n.contains("anim") {
            &["anim", "prop"]
        } else {
            &[]
        };
        actors.iter().find(|(_, t)| want.iter().any(|w| t.to_lowercase().contains(w))).map(|(i, _)| *i).unwrap_or_else(|| actors.first().map(|a| a.0).unwrap_or(0))
    };
    c.params.iter().map(|p| if p.ty == "Actor" { format!("MISSION.GetActor({})", pick(p.name)) } else { p.default.to_string() }).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creators::CREATORS;
    use zartex_core::compile::apply_snippet;
    use zartex_core::mission::MissionFile;

    /// Every creator of the table runs with its default arguments, adds the node or actor it promises, and the mission
    /// it produces can be written and read again. At least 90 per cent of the creators must pass.
    #[test]
    fn creators_run_with_their_default_arguments() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/golden/survival.mpc");
        let bytes = std::fs::read(path).unwrap();
        let (mut ok, mut failed) = (0, Vec::new());
        for c in CREATORS {
            let mut mf = MissionFile::from_bytes(&bytes).unwrap();
            let (a0, n0) = (mf.mission.logic.actors.definitions.len(), mf.mission.logic.nodes.definitions.len());
            let actors: Vec<(usize, String)> = mf.mission.logic.actors.definitions.iter().enumerate().map(|(i, a)| (i, zartex_core::nodenames::lookup(zartex_core::nodenames::ACTOR_NODE_TYPES, a.type_id).unwrap_or("?").to_string())).collect();
            let args = default_args(c, &actors);
            match apply_snippet(&mut mf.mission, &mut mf.summary, &format!("local created = MISSION.{}({args})", c.name)) {
                Ok(()) => {
                    let grew = if c.makes_node { mf.mission.logic.nodes.definitions.len() > n0 } else { mf.mission.logic.actors.definitions.len() > a0 };
                    assert!(grew, "{} did not add what it promises", c.name);
                    assert!(MissionFile::from_bytes(&mf.to_bytes().unwrap()).is_ok(), "{} produced a mission that cannot be reloaded", c.name);
                    ok += 1;
                }
                Err(e) => failed.push(format!("{}: {e}", c.name)),
            }
        }
        println!("{ok} of {} creators run with their defaults", CREATORS.len());
        for f in &failed {
            println!("  {f}");
        }
        assert!(ok * 10 >= CREATORS.len() * 9, "too many creators fail with their default arguments");
    }

    /// Deleting nodes and an actor keeps the mission consistent: wires point at existing nodes, actor references point at
    /// existing actors, and wire collections exist. The check also runs after a save and reload.
    #[test]
    fn deleting_keeps_references_valid() {
        use crate::model::*;
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle/golden/survival.mpc");
        let bytes = std::fs::read(path).unwrap();
        let valid = |p: &Project| {
            let l = &p.view().mission.logic;
            let (na, nn) = (l.actors.definitions.len() as i32, l.nodes.definitions.len() as i32);
            let wires_ok = l.wires.collections.iter().all(|c| c.wires.iter().all(|w| (w.node_id as i32) < nn));
            let refs_ok = l.actors.definitions.iter().chain(l.nodes.definitions.iter()).all(|d| d.properties.iter().all(|pr| pr.type_id != 7 || matches!(pr.value, zartex_core::logic::PropValue::Int(x) if x < na)));
            let colls_ok = l.nodes.definitions.iter().all(|d| match d.properties.first() { Some(zartex_core::logic::Property { type_id: 19, value: zartex_core::logic::PropValue::Int(w), .. }) => (*w as usize) < l.wires.collections.len(), _ => true });
            wires_ok && refs_ok && colls_ok
        };
        let mut p = Project::open_bytes("survival.mpc", &bytes).unwrap();
        assert!(valid(&p));
        let (n0, a0, o0) = (p.view().mission.logic.nodes.definitions.len(), p.view().mission.logic.actors.definitions.len(), p.view().mission.objects.objects.len());
        p.apply(Edit::DeleteNode(2));
        p.apply(Edit::DeleteNode(0));
        p.apply(Edit::DeleteActor(1));
        assert_eq!(p.view().mission.logic.nodes.definitions.len(), n0 - 2);
        assert_eq!(p.view().mission.logic.actors.definitions.len(), a0 - 1);
        assert!(p.view().mission.objects.objects.len() <= o0);
        assert!(valid(&p), "references are broken after deleting");
        p.sync_to_source();
        let out = match &mut p.source { Source::Mpc(mf) => mf.to_bytes().unwrap(), _ => unreachable!() };
        let q = Project::open_bytes("survival.mpc", &out).unwrap();
        assert!(valid(&q));
        assert_eq!(q.view().mission.logic.nodes.definitions.len(), n0 - 2);
    }
}
