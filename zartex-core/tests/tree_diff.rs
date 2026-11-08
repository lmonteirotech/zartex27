//! Debug dump of chunk trees, not an assertion: prints every package and buffer of the embedded template, of
//! oracle/golden/template.mpc and of oracle/golden/DPL_template.sp, with description, alignment, version and offset.
//! The dump_trees test is ignored by default. Run: cargo test --test tree_diff -- --ignored --nocapture
use zartex_core::chunk::*;

/// Turns a context id into text: its four little-endian bytes as characters, with zero bytes dropped.
fn ctx(c: i32) -> String {
    let b = c.to_le_bytes();
    b.iter().filter(|&&x| x != 0).map(|&x| x as char).collect()
}

/// Prints one line per child of p, indented by depth, and recurses into packages. For each buffer it adds
/// (path, length, byte sum) to out.
fn walk(p: &Package, depth: usize, path: String, out: &mut Vec<(String, usize, usize)>) {
    for (i, n) in p.children.iter().enumerate() {
        let name = format!("{}/{}[{}]", path, ctx(n.c().context), i);
        match n {
            Node::Package(q) => {
                println!("{}{} PKG ({} children) desc={:?} al={} ver={} off={:#x}", "  ".repeat(depth), name, q.children.len(), q.c.description, q.c.alignment, q.c.version, q.c.offset);
                walk(q, depth + 1, name, out);
            }
            Node::Buffer(b) => {
                println!("{}{} BUF {} bytes desc={:?} al={} ver={} off={:#x}", "  ".repeat(depth), name, b.data().len(), b.c.description, b.c.alignment, b.c.version, b.c.offset);
                out.push((name, b.data().len(), b.data().iter().map(|&x| x as usize).sum()));
            }
        }
    }
}

/// Prints the trees of the embedded template, the golden template.mpc and the golden DPL_template.sp.
/// Ignored by default.
#[test]
#[ignore]
fn dump_trees() {
    let g = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/golden");
    println!("=== template");
    let t = ChunkFile::from_bytes(include_bytes!("../assets/template.mpc")).unwrap();
    walk(&t.content, 0, String::new(), &mut vec![]);
    println!("=== template.mpc golden");
    let t = ChunkFile::load(g.join("template.mpc")).unwrap();
    walk(&t.content, 0, String::new(), &mut vec![]);
    println!("=== DPL_template.sp");
    let t = ChunkFile::load(g.join("DPL_template.sp")).unwrap();
    walk(&t.content, 0, String::new(), &mut vec![]);
}
