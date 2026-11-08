//! zartex-luac: compile a Driv3r / Driver: PL Lua mission script (LuaC replacement).
//! usage: zartex-luac <script.lua> [-o out] [-dpl]
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 command-line compiler LuaC, which this binary replaces).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! This binary compiles one Lua mission script. The first argument is the script. With `-dpl` (or `-driver4`) the
//! script is compiled as a Parallel Lines package (.sp) by api_dpl::compile_dpl_file; otherwise it is compiled
//! to a .mpc mission by compile::compile_file. `-o` (or `-output`) sets the output file; the default is
//! mission.sp with -dpl and mission.mpc without it. It uses the compiler functions of zartex_core (api_dpl, compile).
//! Nothing in the crate uses this binary; it is run from the command line.
use std::process::exit;

/// Reads the command line, compiles the script and writes the output file. Exits with code 2 when no
/// argument is given, and with code 1 when compilation or writing the output fails.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: zartex-luac <script.lua> [-o out.mpc|out.sp] [-dpl]");
        exit(2);
    }
    // the first argument is the script; every other option is read from the whole list
    let input = &args[0];
    let dpl = args.iter().any(|a| a == "-dpl" || a == "-driver4");
    let out = args
        .iter()
        .position(|a| a == "-o" || a == "-output")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| if dpl { "mission.sp".into() } else { "mission.mpc".into() });
    println!("1>------ Compilation started ------");
    let res = if dpl { zartex_core::api_dpl::compile_dpl_file(input) } else { zartex_core::compile::compile_file(input) };
    match res {
        Ok(bytes) => match std::fs::write(&out, bytes) {
            Ok(()) => {
                println!("1> {input} -> {out}");
                println!("========== Compilation success ==========");
            }
            Err(e) => {
                println!("!>Cannot write {out}: {e}");
                exit(1);
            }
        },
        Err(e) => {
            println!("!>{e}");
            println!("========== Compilation failed ==========");
            exit(1);
        }
    }
}
