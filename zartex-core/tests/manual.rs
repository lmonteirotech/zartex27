//! Keeps the manual source (`docs/manual-source/manual.md`) honest. It checks, without any game installed, that:
//! - every image the manual shows exists in `docs/manual-source/images`;
//! - the Lua functions documented in the manual are exactly the functions of the API (the list in `oracle/api_signatures.json`),
//!   and the functions of the main script class really exist in the Lua environment;
//! - the constants documented in the manual exist with the documented value, and every constant of the environment is documented;
//! - the command-line arguments of the manual are the ones the programs read, and the other way round;
//! - the menu commands and buttons of Appendix B exist in the editor's source.
//!
//! When you change a function, a constant, an argument or a menu, change the manual in the same pull request, and this test will pass.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;
use zartex_core::compile::{new_lua, setup_driv3r};
use zartex_core::luaapi::State;
use zartex_core::script::ScriptState;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    let p = root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

fn manual() -> String {
    read("docs/manual-source/manual.md")
}

#[test]
fn every_image_of_the_manual_exists() {
    let m = manual();
    let mut n = 0;
    for part in m.split("images/").skip(1) {
        let name: String = part.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.' || *c == '-').collect();
        let p = root().join("docs/manual-source/images").join(&name);
        assert!(p.exists(), "the manual shows images/{name}, which does not exist");
        n += 1;
    }
    assert!(n >= 10, "only {n} images found in the manual");
}

/// (name, classes) of every function of the API, from the signature list.
/// Methods of the original script class that the host calls to load a script; they are not functions a script can call,
/// so the manual does not document them.
const NOT_CALLABLE_FROM_LUA: &[&str] = &["LoadScriptFromFile", "LoadFromFile"];

fn api_functions() -> Vec<(String, BTreeSet<String>)> {
    let j = read("oracle/api_signatures.json");
    let mut out: Vec<(String, BTreeSet<String>)> = Vec::new();
    // the file is a list of objects with "class" and "name": read both with a simple scan
    let mut class = String::new();
    for line in j.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("\"class\": \"") {
            class = v.trim_end_matches(['"', ',']).to_string();
        } else if let Some(v) = t.strip_prefix("\"name\": \"") {
            let name = v.trim_end_matches(['"', ',']).to_string();
            if NOT_CALLABLE_FROM_LUA.contains(&name.as_str()) {
                continue;
            }
            match out.iter_mut().find(|(n, _)| *n == name) {
                Some((_, c)) => {
                    c.insert(class.clone());
                }
                None => out.push((name, BTreeSet::from([class.clone()]))),
            }
        }
    }
    out
}

fn lua_env() -> mlua::Lua {
    let st: State = Rc::new(RefCell::new(ScriptState::new("")));
    let lua = new_lua().unwrap();
    setup_driv3r(&lua, &st).unwrap();
    // the state must outlive the Lua functions that hold it
    std::mem::forget(st);
    lua
}

#[test]
fn the_documented_lua_functions_are_the_functions_of_the_api() {
    let documented: BTreeSet<String> = manual().lines().filter_map(|l| l.strip_prefix("### MISSION.").or_else(|| l.strip_prefix("### PACKAGE."))).map(|n| n.trim().to_string()).collect();
    let api = api_functions();
    let names: BTreeSet<String> = api.iter().map(|(n, _)| n.clone()).collect();
    let missing: Vec<_> = names.difference(&documented).collect();
    let extra: Vec<_> = documented.difference(&names).collect();
    assert!(missing.is_empty(), "functions of the API that the manual does not document: {missing:?}");
    assert!(extra.is_empty(), "functions in the manual that the API does not have: {extra:?}");
}

#[test]
fn the_functions_of_the_main_script_class_exist_in_lua() {
    let lua = lua_env();
    let mut checked = 0;
    let mut absent = Vec::new();
    for (name, classes) in api_functions() {
        if !classes.contains("LuaMissionScript") {
            continue;
        }
        let ok: bool = lua.load(format!("return MISSION.{name} ~= nil")).eval().unwrap_or(false);
        checked += 1;
        if !ok {
            absent.push(name);
        }
    }
    assert!(checked > 100, "only {checked} functions checked");
    assert!(absent.is_empty(), "documented functions that are not defined in the Lua environment: {absent:?}");
}

#[test]
fn the_documented_constants_are_the_constants_of_the_environment() {
    let lua = lua_env();
    let m = manual();
    let start = m.find("# 16 Lua constants").expect("chapter 16 is missing");
    let end = m.find("# Appendix").unwrap_or(m.len());
    let mut documented = BTreeSet::new();
    for line in m[start..end].lines() {
        let cells: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
        if cells.len() < 4 || !cells[1].starts_with('`') {
            continue;
        }
        let name = cells[1].trim_matches('`');
        if name == "Constant" {
            continue;
        }
        // the value: first number of the cell
        let value: String = cells[2].trim_start_matches('`').chars().take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.').collect();
        let got: f64 = lua.load(format!("return {name}")).eval().unwrap_or_else(|e| panic!("constant {name} is documented but not defined: {e}"));
        let want: f64 = value.parse().unwrap_or_else(|_| panic!("cannot read the value of {name}: {}", cells[2]));
        assert_eq!(got, want, "constant {name}");
        documented.insert(name.to_string());
    }
    // every constant of the environment
    let globals: mlua::Table = lua.globals();
    let mut defined = BTreeSet::new();
    for pair in globals.pairs::<String, mlua::Value>() {
        let (k, v) = pair.unwrap();
        if matches!(v, mlua::Value::Number(_) | mlua::Value::Integer(_)) && k.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') {
            defined.insert(k);
        }
    }
    let undocumented: Vec<_> = defined.difference(&documented).collect();
    assert!(undocumented.is_empty(), "constants that the manual does not document: {undocumented:?}");
    assert!(documented.len() > 60, "only {} constants found in chapter 16", documented.len());
}

#[test]
fn the_command_line_of_the_manual_matches_the_programs() {
    let m = manual();
    let c14 = &m[m.find("# 14 Command line").unwrap()..m.find("# 15 Lua language reference").unwrap()];
    let app = read("zartex-app/src/main.rs");
    let luac = read("zartex-core/src/bin/zartex-luac.rs");
    // arguments of the editor: `"--name" =>` in main.rs
    let mut in_code: BTreeSet<String> = BTreeSet::new();
    for part in app.split("\"--").skip(1) {
        let name: String = part.chars().take_while(|c| c.is_ascii_lowercase() || *c == '-' || c.is_ascii_digit()).collect();
        if part[name.len()..].starts_with("\" =>") {
            in_code.insert(format!("--{name}"));
        }
    }
    assert!(in_code.len() >= 7, "found only {in_code:?} in main.rs");
    for a in &in_code {
        assert!(c14.contains(&format!("`{a}")), "the editor reads {a}, the manual (chapter 14) does not describe it");
    }
    for part in c14.split("`--").skip(1) {
        let name: String = part.chars().take_while(|c| c.is_ascii_lowercase() || *c == '-' || c.is_ascii_digit()).collect();
        assert!(app.contains(&format!("\"--{name}\"")), "the manual describes --{name}, which the editor does not read");
    }
    // the compiler
    for flag in ["-o", "-output", "-dpl", "-driver4"] {
        assert!(luac.contains(&format!("\"{flag}\"")), "zartex-luac does not read {flag}");
        assert!(c14.contains(&format!("`{flag}`")) || c14.contains(&format!("`{flag} ")), "the manual does not describe {flag}");
    }
    // environment variables of the editor
    for part in app.split("var(\"ZM_").skip(1) {
        let name: String = part.chars().take_while(|c| c.is_ascii_uppercase() || *c == '_').collect();
        assert!(c14.contains(&format!("ZM_{name}")), "the editor reads ZM_{name}, the manual does not describe it");
    }
}

#[test]
fn the_menu_commands_of_appendix_b_exist_in_the_editor() {
    let m = manual();
    let app = read("zartex-app/src/main.rs");
    let start = m.find("# Appendix B").unwrap();
    let end = m[start..].find("# Appendix C").map(|e| start + e).unwrap_or(m.len());
    let mut n = 0;
    for line in m[start..end].lines() {
        let cells: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
        if cells.len() < 5 {
            continue;
        }
        let (where_, cmd) = (cells[1], cells[2].trim_end_matches("..."));
        if matches!(where_, "File" | "View" | "Lua" | "Help" | "Lua script tab" | "Objects tab" | "Assets tab") {
            assert!(app.contains(cmd), "Appendix B lists \"{cmd}\" ({where_}), which is not in the editor's source");
            n += 1;
        }
    }
    assert!(n >= 20, "only {n} menu commands checked");
}
