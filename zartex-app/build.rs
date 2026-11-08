//! Build script: embeds the build date, the build target and the program icon in the executable.
//!
//! What it does: sets `BUILD_DATE` (the UTC date of the build, from the system clock) and `BUILD_TARGET` (the target
//! triple) as environment values read by the program with `env!`, and on Windows compiles `assets/icon.ico` into the
//! executable. The date and target are shown in Help > About.
//! Uses: nothing from the repository. Used by: Cargo, before the crate is compiled.
fn main() {
    // seconds since 1970-01-01 (0 when the clock is before that)
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    // days since 1970-01-01, moved to a 400-year cycle that starts on 1 March (so that leap days fall at its end)
    let z = secs / 86400 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    // ---- the calendar date, from the day count: year, month and day
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    println!("cargo:rustc-env=BUILD_DATE={y:04}-{m:02}-{d:02}");
    println!("cargo:rustc-env=BUILD_TARGET={}", std::env::var("TARGET").unwrap_or_default());
    // the program icon (Explorer, taskbar)
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the icon: {e}");
        }
    }
}
