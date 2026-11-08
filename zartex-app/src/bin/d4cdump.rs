//! Structure of a Parallel Lines city file: tags, counts and sizes per level.
//!
//! usage: d4cdump <city file .d4c> [max depth, default 3]
//! What it does: parses the file with `formats::chnk`, then prints, for every chunk path, how many chunks there are and
//! their total size, down to the given depth. Uses `formats::chnk` only.
use formats::chnk::Chunk;
use std::collections::BTreeMap;

fn walk(c: &Chunk, depth: usize, max: usize, path: &str, stats: &mut BTreeMap<String, (usize, usize, u8)>) {
    for e in &c.entries {
        let key = format!("{path}/{}", e.tag());
        let s = stats.entry(key.clone()).or_insert((0, 0, e.version));
        s.0 += 1;
        s.1 += e.data.len();
        if e.is_chnk() && depth < max {
            if let Ok(k) = e.children() {
                walk(&k, depth + 1, max, &key, stats);
            }
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let d = std::fs::read(&a[1]).unwrap();
    println!("read {} MB in {} ms", d.len() >> 20, t.elapsed().as_millis());
    let root = Chunk::parse(&d).unwrap();
    println!("root version {} entries {}", root.version, root.entries.len());
    let max: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut st = BTreeMap::new();
    walk(&root, 0, max, "", &mut st);
    for (k, (n, bytes, v)) in st {
        println!("{k:60} x{n:<6} {bytes:>12} bytes  ver {v}");
    }
    for e in root.entries.iter().take(6) {
        println!("top {} desc {:?} len {}", e.tag(), e.desc, e.data.len());
    }
}
