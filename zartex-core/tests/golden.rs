//! Byte-identity round trip for the golden files in oracle/golden: each .mpc or .sp file is loaded as a chunk file
//! and saved again, and the bytes must be equal. The test also round trips assets/template.mpc, which is bundled
//! with the crate. It needs oracle/golden to hold exactly 12 .mpc or .sp files (asserted). No game files are needed.
use std::path::PathBuf;
use zartex_core::chunk::*;

/// Returns the oracle/golden folder, relative to the crate directory.
fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/golden")
}

/// Lists the .mpc and .sp files in oracle/golden, sorted by path.
fn golden_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(golden_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("mpc") | Some("sp")))
        .collect();
    v.sort();
    v
}

/// Every golden file must load as a chunk file and save back to the same bytes. Mismatches are collected and asserted together.
#[test]
fn identity_roundtrip_all_golden() {
    let files = golden_files();
    // The golden set is fixed at 12 files; a different count means the folder has changed.
    assert_eq!(files.len(), 12);
    let mut failed = vec![];
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        let mut cf = ChunkFile::from_bytes(&orig).unwrap();
        let out = cf.to_bytes().unwrap();
        if out != orig {
            let at = out.iter().zip(&orig).position(|(a, b)| a != b);
            failed.push(format!("{}: len {} vs {}, first diff {:?}", f.display(), out.len(), orig.len(), at));
        }
    }
    assert!(failed.is_empty(), "{:#?}", failed);
}

/// The template in assets/template.mpc must load as a chunk file and save back to the same bytes.
#[test]
fn identity_roundtrip_template() {
    let orig = include_bytes!("../assets/template.mpc");
    let mut cf = ChunkFile::from_bytes(orig).unwrap();
    assert_eq!(cf.to_bytes().unwrap(), orig.to_vec());
}
