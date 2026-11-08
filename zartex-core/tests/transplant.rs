//! Transplant test: load assets/template.mpc, replace each buffer with the golden buffer that has the same path,
//! save, and compare the result with the golden file. Buffers are set in two modes: always, or only when changed.
//! This isolates the chunk container's dirty and relayout handling from mission serialization.
//! Needs the golden files in oracle/golden. No game files are needed.
use std::path::PathBuf;
use zartex_core::chunk::*;

/// Reads a file from oracle/golden, relative to the crate directory.
fn golden(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/golden").join(name)).unwrap()
}

/// Collect (path, data) for every buffer.
fn buffers(p: &Package, path: &str, out: &mut Vec<(String, Vec<u8>)>) {
    for (i, n) in p.children.iter().enumerate() {
        let name = format!("{}/{}#{}", path, n.c().context, i);
        match n {
            Node::Package(q) => buffers(q, &name, out),
            Node::Buffer(b) => out.push((name, b.data().to_vec())),
        }
    }
}

/// Walks the package tree and sets the buffer whose path equals target to data. Returns true when such a buffer is found.
/// With only_if_changed, set_data (which marks the buffer dirty) is skipped when the buffer already holds the same bytes.
fn set_by_path(p: &mut Package, path: &str, target: &str, data: &[u8], only_if_changed: bool) -> bool {
    for i in 0..p.children.len() {
        // The path is built the same way as in buffers, so a path from the golden tree matches here.
        let name = format!("{}/{}#{}", path, p.children[i].c().context, i);
        match &mut p.children[i] {
            Node::Package(q) => {
                if set_by_path(q, &name, target, data, only_if_changed) {
                    return true;
                }
            }
            Node::Buffer(b) => {
                if name == target {
                    if !(only_if_changed && b.data() == data) {
                        b.set_data(data.to_vec());
                    }
                    return true;
                }
            }
        }
    }
    false
}

/// Replaces every buffer of the template with the golden buffer at the same path, saves, and checks that the length and
/// bytes equal the golden file. name is the golden file name. only_if_changed is passed to set_by_path.
fn transplant(name: &str, only_if_changed: bool) {
    let gold = golden(name);
    let gold_tree = ChunkFile::from_bytes(&gold).unwrap();
    let mut bufs = vec![];
    buffers(&gold_tree.content, "", &mut bufs);

    // The template is the starting tree; each golden buffer is written into it at the same path.
    let mut t = ChunkFile::from_bytes(include_bytes!("../assets/template.mpc")).unwrap();
    for (path, data) in &bufs {
        assert!(set_by_path(&mut t.content, "", path, data, only_if_changed), "{name}: no buffer at {path}");
    }
    let out = t.to_bytes().unwrap();
    assert_eq!(out.len(), gold.len(), "{name}: length (only_if_changed={only_if_changed})");
    assert!(out == gold, "{name}: bytes differ (only_if_changed={only_if_changed})");
}

const DRIV3R: [&str; 7] = [
    "template.mpc", "deathmatch.mpc", "presidentsrun.mpc", "scriptedTraffic.mpc",
    "survival.mpc", "areatest.mpc", "checkpointRace.mpc",
];

/// Transplants each golden file with only_if_changed set to false, so every buffer is set.
#[test]
fn transplant_all_buffers_dirty() {
    for n in DRIV3R { transplant(n, false); }
}

/// Transplants each golden file with only_if_changed set to true, so buffers whose bytes are equal are not set.
#[test]
fn transplant_only_changed_buffers() {
    for n in DRIV3R { transplant(n, true); }
}
