//! Looks at the header of the first MDXN packages of a Parallel Lines city file.
//!
//! usage: mdxnprobe <city file .d4c>
//! What it does: finds the `MDXN` packages in the `GESR` and `GEGR` chunks (the first three are shown), prints the header
//! words, the spacing between the tables, and the vertex declarations with the first vertices as floats. Uses `formats::chnk`.
use formats::chnk::Chunk;

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = std::fs::read(&a[1]).unwrap();
    let root = Chunk::parse(&d).unwrap();
    let mut shown = 0;
    for e in root.entries.iter().filter(|e| e.tag() == "GESR" || e.tag() == "GEGR") {
        let k = e.children().unwrap();
        for m in k.find("MDXN") {
            let x = m.data;
            println!("== {} {:?} MDXN ver {} len {}", e.tag(), e.desc, m.version, x.len());
            let names = ["version", "uid", "nModels", "modelsOff", "nLod", "lodOff", "nSub", "subOff", "handle", "reserved", "texOff", "matOff", "nIdx", "idxLen", "idxOff", "nVb", "vbOff", "w17", "w18", "w19"];
            for (i, n) in names.iter().enumerate() {
                print!("{n}={:#x} ", u32_at(x, i * 4));
            }
            println!();
            let (nm, mo, nl, lo, ns, so) = (u32_at(x, 8), u32_at(x, 12), u32_at(x, 16), u32_at(x, 20), u32_at(x, 24), u32_at(x, 28));
            let mut offs = vec![(mo, "models", nm), (lo, "lod", nl), (so, "sub", ns), (u32_at(x, 56), "idx", u32_at(x, 48)), (u32_at(x, 64), "vdecl", u32_at(x, 60)), (u32_at(x, 44), "mat", 1), (u32_at(x, 40), "tex", 1)];
            offs.sort();
            for w in offs.windows(2) {
                let span = w[1].0 as i64 - w[0].0 as i64;
                println!("  {:7} @{:#x} count {} -> span to next {} => {:.2} per record", w[0].1, w[0].0, w[0].2, span, if w[0].2 > 0 { span as f64 / w[0].2 as f64 } else { 0.0 });
            }
            vdecls(x);
            shown += 1;
            if shown >= 3 {
                return;
            }
        }
    }
}

#[allow(dead_code)]
pub fn vdecls(x: &[u8]) {
    let n = u32_at(x, 60) as usize;
    let off = u32_at(x, 64) as usize;
    for i in 0..n {
        let o = off + 32 * i;
        let w: Vec<u32> = (0..8).map(|k| u32_at(x, o + 4 * k)).collect();
        println!("  vb{i}: count {} len {} off {:#x} stride {} w4 {:#x} compression {} w6 {:#x} w7 {:#x}", w[0], w[1], w[2], w[3], w[4], w[5], w[6], w[7]);
        let vo = w[2] as usize;
        for v in 0..3 {
            let b = &x[vo + v * w[3] as usize..vo + (v + 1) * w[3] as usize];
            println!("     v{v}: {:02x?}", b);
            let f: Vec<String> = b.chunks(4).map(|c| format!("{:.3}", f32::from_le_bytes(c.try_into().unwrap_or([0; 4])))).collect();
            println!("        as f32: {}", f.join(" "));
        }
    }
    let nm = u32_at(x, 8) as usize;
    for m in 0..3.min(nm) {
        let o = 0x80 + 332 * m;
        let h: Vec<String> = (0..9).map(|k| format!("{:#x}", u32_at(x, o + 4 * k))).collect();
        println!("  model{m} head {}", h.join(" "));
    }
}
