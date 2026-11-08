//! Debug helper, not an assertion: compiles the script named by ZARTEX_DIFF and prints the buffers that differ from
//! the reference file named by ZARTEX_GOLDEN. A golden file whose name ends in .sp is compiled as a DPL package.
//! Both variables must be set; the test unwraps them and panics when one is missing.
//! Run: ZARTEX_DIFF=<script.lua> ZARTEX_GOLDEN=<file> cargo test --test diff_tool -- --ignored --nocapture
use zartex_core::chunk::*;

/// Turns a context id into text: its four little-endian bytes as characters, with zero bytes dropped.
fn ctx(c: i32) -> String {
    c.to_le_bytes().iter().filter(|&&x| x != 0).map(|&x| x as char).collect()
}

/// Walks a package tree and adds (path, data) to out for every buffer. Each path is the parent path, a slash,
/// the context text and #index.
fn collect(p: &Package, path: &str, out: &mut Vec<(String, Vec<u8>)>) {
    for (i, n) in p.children.iter().enumerate() {
        let name = format!("{}/{}#{}", path, ctx(n.c().context), i);
        match n {
            Node::Package(q) => collect(q, &name, out),
            Node::Buffer(b) => out.push((name, b.data().to_vec())),
        }
    }
}

/// Compiles the script named by ZARTEX_DIFF, reads the file named by ZARTEX_GOLDEN, and prints each pair of buffers
/// whose bytes differ, with the first differing position and the bytes around it.
#[test]
#[ignore]
fn diff_buffers() {
    let script = std::env::var("ZARTEX_DIFF").unwrap();
    let golden = std::env::var("ZARTEX_GOLDEN").unwrap();
    // The file name decides the compiler: a name ending in .sp is compiled as a DPL package, any other as a mission.
    let out = if golden.ends_with(".sp") {
        zartex_core::api_dpl::compile_dpl_file(&script).unwrap()
    } else {
        zartex_core::compile::compile_file(&script).unwrap()
    };
    let g = std::fs::read(&golden).unwrap();
    let (mut a, mut b) = (vec![], vec![]);
    collect(&ChunkFile::from_bytes(&out).unwrap().content, "", &mut a);
    collect(&ChunkFile::from_bytes(&g).unwrap().content, "", &mut b);
    println!("buffers: ours {} golden {}", a.len(), b.len());
    // Buffers are paired by position, so buffers past the end of the shorter list are not compared.
    for ((pa, da), (pb, db)) in a.iter().zip(&b) {
        if da != db {
            let at = da.iter().zip(db).position(|(x, y)| x != y);
            println!("DIFF {pa} vs {pb}: len {} vs {}, first diff {:?}", da.len(), db.len(), at);
            if let Some(i) = at {
                let lo = i.saturating_sub(16);
                println!("  ours  : {:02x?}", &da[lo..(i + 24).min(da.len())]);
                println!("  golden: {:02x?}", &db[lo..(i + 24).min(db.len())]);
            }
        }
    }
}
