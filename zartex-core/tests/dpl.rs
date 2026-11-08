//! DPL (.sp) model round trip: each golden package is parsed into DplPackage, written back with to_bytes, and the
//! bytes must equal the golden file. A second test checks the fixed extra buffers of every mission against the
//! defaults that DplMission::new writes.
//! Needs DPL_template.sp, DPL_ambulance.sp, DPL_ambulance_init.sp, DPL_vehicle.sp and DPLtest.sp in oracle/golden.
use std::path::PathBuf;
use zartex_core::dpl::*;

const DPL: [&str; 5] = ["DPL_template.sp", "DPL_ambulance.sp", "DPL_ambulance_init.sp", "DPL_vehicle.sp", "DPLtest.sp"];

/// Reads a file from oracle/golden, relative to the crate directory.
fn golden(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oracle/golden").join(name)).unwrap()
}

/// Loads each DPL golden package, writes it back, and compares the bytes. Failures are collected and asserted together.
#[test]
fn dpl_model_roundtrip() {
    // Failures are collected so that one run lists every package that does not match.
    let mut failed = vec![];
    for n in DPL {
        let g = golden(n);
        let mut pkg = match DplPackage::from_bytes(&g) {
            Ok(p) => p,
            Err(e) => {
                failed.push(format!("{n}: load: {e}"));
                continue;
            }
        };
        match pkg.to_bytes() {
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

/// The fixed 'extras' CreateMission writes must equal what the oracle produced.
#[test]
fn dpl_extras_match_defaults() {
    for n in DPL {
        let pkg = DplPackage::from_bytes(&golden(n)).unwrap();
        let all = std::iter::once(&pkg.init).chain(pkg.missions.iter().map(|m| &m.1));
        for m in all {
            // The expected extras are the defaults that DplMission::new writes for this mission.
            let d = DplMission::new(None, m.id, m.x, m.y, m.flags, true);
            assert_eq!((&m.empr, &m.leps, &m.lene, &m.fing, &m.leat), (&d.empr, &d.leps, &d.lene, &d.fing, &d.leat), "{n}: extras of mission id {}", m.id);
        }
    }
}
