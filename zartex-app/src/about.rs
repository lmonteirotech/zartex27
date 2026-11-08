//! Help > About: what runs the app (system information), what it is built from, who made it, and its licence.
//! Holds the program name, version, build data and web links as constants, the SysInfo record, and gather, which fills it.
//! The About window in main.rs shows all of it; the renderer line of that window comes from gpu.rs through main.rs.
//! Uses the sysinfo crate (OS, processor, memory) and, on Windows, PowerShell (graphics card names). It uses no module
//! of this repository. Used by main.rs only (the About window and the Help menu).

// ---------------------------------------------------------------- program identity and links
/// Program name shown in the title bar and in the About window.
pub const NAME: &str = "Zartex'27";
/// Version of this crate, taken from its Cargo manifest by Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Build date (yyyy-mm-dd), set at build time by build.rs.
pub const BUILD_DATE: &str = env!("BUILD_DATE");
/// Target triple the program was built for, set at build time by build.rs.
pub const BUILD_TARGET: &str = env!("BUILD_TARGET");
/// The documentation page: set when the repository exists (the menu entry stays disabled until then).
pub const DOCS_URL: Option<&str> = None;
/// Project page of the author, opened from the Credits tab.
pub const AUTHOR_URL: &str = "https://github.com/lmonteirotech";
/// Full text of the GNU GPL version 3, opened from the License tab.
pub const LICENSE_URL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";

// ---------------------------------------------------------------- system information
/// Description of this computer, filled by gather. The default value has empty fields.
#[derive(Clone, Default)]
pub struct SysInfo {
    pub os: String,
    pub cpu: String,
    pub ram: String,
    pub gpus: Vec<String>,
}

/// Formats a byte count in gigabytes (base 1024) with one decimal place.
fn gib(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Reads the machine's description (takes a moment: run it in a thread).
pub fn gather() -> SysInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    // operating system name and kernel version, joined on one line
    let os = [sysinfo::System::long_os_version(), sysinfo::System::kernel_version().map(|k| format!("(kernel {k})"))].into_iter().flatten().collect::<Vec<_>>().join(" ");
    // processor name and the number of logical processors; unknown when none is listed
    let cpu = match sys.cpus().first() {
        Some(c) => format!("{} \u{00B7} {} threads", c.brand().trim(), sys.cpus().len()),
        None => "unknown".into(),
    };
    // when the OS name is empty, the Rust name of the target OS is used
    SysInfo { os: if os.is_empty() { std::env::consts::OS.into() } else { os }, cpu, ram: gib(sys.total_memory()), gpus: gpu_names() }
}

/// Names of the graphics cards, read with PowerShell (Get-CimInstance). Returns an empty list when PowerShell cannot run.
#[cfg(windows)]
fn gpu_names() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "(Get-CimInstance Win32_VideoController).Name"])
        // 0x0800_0000 is CREATE_NO_WINDOW: no console window opens for PowerShell
        .creation_flags(0x0800_0000)
        .output();
    match out {
        // the output is one name per line; empty lines are dropped
        Ok(o) => String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Other systems: no graphics card names are read, so the list is empty.
#[cfg(not(windows))]
fn gpu_names() -> Vec<String> {
    Vec::new()
}

/// One line for the About window: debug or release build, the target, and the build date.
pub fn build_line() -> String {
    format!("{} build \u{00B7} {} \u{00B7} built {}", if cfg!(debug_assertions) { "debug" } else { "release" }, BUILD_TARGET, BUILD_DATE)
}
