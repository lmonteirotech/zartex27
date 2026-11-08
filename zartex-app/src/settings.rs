//! Game installations (Driv3r, Driver: Parallel Lines) and what the app finds in them.
//! Holds the Settings record (game folders, graphics acceleration switch, theme and accent colour), its load and save
//! from settings.txt in the ZartexM folder of APPDATA (key=value lines), the scan of a game folder for its mission and
//! city files (scan), and the district start points of Parallel Lines (pl_landmarks).
//! Exposes Game, Settings, Scan, scan and pl_landmarks. Uses crate::theme (the Appearance type of the theme setting).
//! Used by main.rs only: the settings window, the mission lists and the 3D views.

use std::path::{Path, PathBuf};

/// The two supported installations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Game {
    /// Driv3r.
    Driv3r,
    /// Driver: Parallel Lines.
    ParallelLines,
}

/// The settings kept between runs. Game folders are None until the user sets them.
#[derive(Clone)]
pub struct Settings {
    /// Root folder of Driv3r, or None.
    pub driv3r: Option<PathBuf>,
    /// Root folder of Driver: Parallel Lines, or None.
    pub pl: Option<PathBuf>,
    /// The user switched graphics acceleration (Vulkan) off. Default false: it is on whenever the machine supports it.
    pub gpu_off: bool,
    pub appearance: crate::theme::Appearance,
    /// Custom accent colour (r, g, b)
    pub accent: [u8; 3],
    /// Use the accent colour of Windows (only where it exists)
    pub accent_windows: bool,
}

/// What a recheck of an installation found.
#[derive(Clone, Default)]
pub struct Scan {
    pub ok: bool,
    pub message: String,
    /// Driv3r: every `missionNN.mpc`; Parallel Lines: the `*_missions.sp` packages.
    pub files: Vec<PathBuf>,
    pub cities: usize,
}

/// Path of the settings file (APPDATA\ZartexM\settings.txt), or None when APPDATA is not set.
fn config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("ZartexM").join("settings.txt"))
}

impl Settings {
    /// Reads the settings file. Each line is key=value; a folder is kept only when it exists; unknown keys are ignored. A missing file gives the defaults.
    pub fn load() -> Settings {
        let mut s = Settings::default();
        if let Some(t) = config_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            // keys: appearance, accent (RRGGBB hex), accent_windows, gpu_off, and the two game folders
            for line in t.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k.trim() {
                        "appearance" => {
                            if let Some(a) = crate::theme::Appearance::parse(v) {
                                s.appearance = a;
                            }
                            continue;
                        }
                        "accent" => {
                            let h = v.trim().trim_start_matches('#');
                            if h.len() == 6 {
                                if let Ok(x) = u32::from_str_radix(h, 16) {
                                    s.accent = [(x >> 16) as u8, (x >> 8) as u8, x as u8];
                                }
                            }
                            continue;
                        }
                        "accent_windows" => {
                            s.accent_windows = v.trim() == "1";
                            continue;
                        }
                        _ => {}
                    }
                    // gpu_off=1 switches graphics acceleration off; without the key it stays on
                    if k.trim() == "gpu_off" {
                        s.gpu_off = v.trim() == "1";
                        continue;
                    }
                    // any other key is a game folder: kept only if the folder exists
                    let p = PathBuf::from(v.trim());
                    if !p.is_dir() {
                        continue;
                    }
                    match k.trim() {
                        "driv3r" => s.driv3r = Some(p),
                        "parallel_lines" => s.pl = Some(p),
                        _ => {}
                    }
                }
            }
        }
        s
    }

    /// Writes the settings file, creating its folder first. Game folders are written only when they are set.
    pub fn save(&self) {
        let Some(c) = config_path() else { return };
        if let Some(d) = c.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut t = String::new();
        if self.gpu_off {
            t += "gpu_off=1\n";
        }
        // one key=value line per setting
        t += &format!("appearance={}\naccent={:02X}{:02X}{:02X}\naccent_windows={}\n", self.appearance.key(), self.accent[0], self.accent[1], self.accent[2], if self.accent_windows { 1 } else { 0 });
        if let Some(p) = &self.driv3r {
            t += &format!("driv3r={}\n", p.display());
        }
        if let Some(p) = &self.pl {
            t += &format!("parallel_lines={}\n", p.display());
        }
        let _ = std::fs::write(c, t);
    }

    /// Root folder of a game, or None when it is not set.
    pub fn path(&self, g: Game) -> Option<&Path> {
        match g {
            Game::Driv3r => self.driv3r.as_deref(),
            Game::ParallelLines => self.pl.as_deref(),
        }
    }
}

/// Files of a folder with the given extension (any case) whose name, without the extension, ends with suffix. Sorted. Empty when the folder cannot be read.
fn list_dir(dir: &Path, ext: &str, suffix: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().map(|x| x.eq_ignore_ascii_case(ext)).unwrap_or(false)
                        && p.file_stem().map(|s| s.to_string_lossy().to_lowercase().ends_with(suffix)).unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Looks for the mission files of a game folder and counts its city files. Returns ok = false with a message when no mission file is found.
pub fn scan(game: Game, root: &Path) -> Scan {
    match game {
        Game::Driv3r => {
            // Driv3r: every missionNN.mpc except the template files; the cities are the .d3c files in Cities
            let files = list_dir(&root.join("Missions"), "mpc", "");
            let files: Vec<PathBuf> = files.into_iter().filter(|p| !p.file_stem().unwrap().to_string_lossy().to_lowercase().starts_with("template")).collect();
            let cities = std::fs::read_dir(root.join("Cities")).map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("d3c")).unwrap_or(false)).count()).unwrap_or(0);
            if files.is_empty() {
                Scan { ok: false, message: "No Missions\\*.mpc found. Is this the Driv3r root folder?".into(), files, cities }
            } else {
                Scan { ok: true, message: format!("{} missions, {} city files", files.len(), cities), files, cities }
            }
        }
        Game::ParallelLines => {
            // Parallel Lines: the *_missions.sp packages in LifeEvents; the cities are the .d4c files in Cities
            let files = list_dir(&root.join("LifeEvents"), "sp", "_missions");
            let cities = std::fs::read_dir(root.join("Cities")).map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("d4c")).unwrap_or(false)).count()).unwrap_or(0);
            if files.is_empty() {
                Scan { ok: false, message: "No LifeEvents\\*_missions.sp found. Is this the Parallel Lines root folder?".into(), files, cities }
            } else {
                Scan { ok: true, message: format!("{} mission packages, {} city files", files.len(), cities), files, cities }
            }
        }
    }
}

/// District start points of `Cities\levels.txt`: (name, x, y, z).
pub fn pl_landmarks(root: &Path) -> Vec<(String, [f32; 3])> {
    let Ok(t) = std::fs::read_to_string(root.join("Cities").join("levels.txt")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut on = false;
    for line in t.lines() {
        let l = line.trim();
        if l.eq_ignore_ascii_case("STARTPOINTS") {
            on = true;
            continue;
        }
        if l.eq_ignore_ascii_case("ENDSTARTPOINTS") {
            break;
        }
        if !on || !l.starts_with('"') {
            continue;
        }
        let Some(end) = l[1..].find('"') else { continue };
        let name = l[1..1 + end].to_string();
        let nums: Vec<f32> = l[end + 2..].split(',').filter_map(|v| v.trim().parse::<f32>().ok()).collect();
        if nums.len() == 3 {
            out.push((name, [nums[0], nums[1], nums[2]]));
        }
    }
    out
}

// defaults: no folders, follow the system theme, Fluent blue accent, and the Windows accent on Windows
impl Default for Settings {
    fn default() -> Self {
        Settings { driv3r: None, pl: None, gpu_off: false, appearance: crate::theme::Appearance::System, accent: [0x00, 0x67, 0xC0], accent_windows: cfg!(windows) }
    }
}
