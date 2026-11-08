//! Lua -> MPC and Lua -> SP parity against the golden output of the original LuaC, stored in oracle/golden.
//! Each Driv3r script must compile to the same bytes as its .mpc file, and each DPL script to its .sp file.
//! Mission scripts come from oracle/scripts_fixed when that file exists, otherwise from ../Zartex2/Scripts, a folder
//! next to the repository root. DPL scripts always come from ../Zartex2/Scripts. No game files are needed.
use std::path::PathBuf;
use zartex_core::compile::*;

/// Returns the repository root: CARGO_MANIFEST_DIR with ".." appended. The oracle and script folders are resolved from it.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Returns the Lua source of a mission script: oracle/scripts_fixed/<name>.lua when that file exists, otherwise
/// ../Zartex2/Scripts/<name>.lua.
fn script_path(name: &str) -> PathBuf {
    // areatest / checkpointRace come from the patched copies the oracle used
    let fixed = root().join("oracle/scripts_fixed").join(format!("{name}.lua"));
    if fixed.exists() {
        fixed
    } else {
        root().join("../Zartex2/Scripts").join(format!("{name}.lua"))
    }
}

/// Compiles the mission script name and compares the bytes with oracle/golden/<name>.mpc. Returns Err with the index
/// of the first differing byte when the bytes differ, or with the compile error when compilation fails.
fn check(name: &str) -> Result<(), String> {
    let golden = std::fs::read(root().join("oracle/golden").join(format!("{name}.mpc"))).unwrap();
    let out = compile_file(script_path(name).to_str().unwrap()).map_err(|e| format!("{name}: {e}"))?;
    if out == golden {
        return Ok(());
    }
    let at = out.iter().zip(&golden).position(|(a, b)| a != b);
    Err(format!("{name}: bytes differ (len {} vs {}, first diff {:?})", out.len(), golden.len(), at))
}

const DRIV3R: [&str; 7] =
    ["template", "deathmatch", "presidentsrun", "scriptedTraffic", "survival", "areatest", "checkpointRace"];

/// Checks all seven mission scripts listed in DRIV3R and reports every mismatch together.
#[test]
fn driv3r_scripts_match_oracle() {
    let errs: Vec<String> = DRIV3R.iter().filter_map(|n| check(n).err()).collect();
    assert!(errs.is_empty(), "\n{}", errs.join("\n"));
}

/// Checks only the template mission script, as a single case.
#[test]
fn template_matches_oracle() {
    check("template").unwrap();
}

const DPL: [&str; 5] = ["DPL_template", "DPL_ambulance", "DPL_ambulance_init", "DPL_vehicle", "DPLtest"];

/// Compiles the DPL script name from ../Zartex2/Scripts as a package and compares the bytes with oracle/golden/<name>.sp.
fn check_dpl(name: &str) -> Result<(), String> {
    let golden = std::fs::read(root().join("oracle/golden").join(format!("{name}.sp"))).unwrap();
    let path = root().join("../Zartex2/Scripts").join(format!("{name}.lua"));
    let out = zartex_core::api_dpl::compile_dpl_file(path.to_str().unwrap()).map_err(|e| format!("{name}: {e}"))?;
    if out == golden {
        return Ok(());
    }
    let at = out.iter().zip(&golden).position(|(a, b)| a != b);
    Err(format!("{name}: bytes differ (len {} vs {}, first diff {:?})", out.len(), golden.len(), at))
}

/// Checks all DPL package scripts listed in DPL and reports every mismatch together.
#[test]
fn dpl_scripts_match_oracle() {
    let errs: Vec<String> = DPL.iter().filter_map(|n| check_dpl(n).err()).collect();
    assert!(errs.is_empty(), "\n{}", errs.join("\n"));
}
