//! Zartex'27 editor window. The App struct holds the state; the rest of this file draws it and reacts to it.
//! Order of this file: start-up (App::new, the command line and environment variables used by the screenshot tests),
//! file and game-folder actions (open, save, compile and export Lua), the frame loop (eframe::App::ui), the panels (title
//! bar and menus, toolbar, status line, bottom tabs, left list, right inspector, centre view), the asset previews, the
//! Missions tab, the Add, About and Settings windows, the logic graph, and the small drawing helpers.
//! Exposes main and kv (pub(crate), used by propedit.rs and overview.rs). The window does not read or write missions
//! itself: it calls zartex-core for that and model.rs for the editing model.
//! Uses the sibling modules declared below (model, view3d, city3d, camera, propedit, overview, about, settings, theme,
//! missions, plassets, assets, creators, gpu and the rest) and zartex_core for reading, writing, compiling and exporting.
//! Used by nothing: it is the root of the zartex-app binary.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// modules of the app. Windows and panels: about, settings, theme, overview, propedit, missions, camera.
// Data and editing: model, mood, assets, creators, creator_tools, skin_names.
// 3D and graphics: view3d, city3d, character3d, vehicle3d, modelbox, plassets, gpu.
mod about;
mod assets;
mod plassets;
mod camera;
mod creator_tools;
mod gpu;
mod creators;
mod character3d;
mod city3d;
mod missions;
mod model;
mod propedit;
mod modelbox;
mod vehicle3d;
mod mood;
mod overview;
mod settings;
mod skin_names;
mod theme;
mod view3d;

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use model::*;
use settings::{Game, Scan, Settings};
use std::path::PathBuf;

// ---------------------------------------------------------------- app state
/// The current selection. Node, Actor and Object index the mission's logic nodes, actors and map objects. Actors of both games use Actor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sel {
    /// Nothing selected: the inspector shows the mission overview.
    None,
    /// A logic node, by index.
    Node(usize),
    /// An actor, by index.
    Actor(usize),
    /// A map object, by index.
    Object(usize),
}

/// Tabs of the bottom panel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bottom {
    /// The event log.
    Console,
    /// The Lua script editor.
    Lua,
    /// Problems found in the open mission.
    Warnings,
    /// The cutscene shot list and timeline.
    Cameras,
}

/// Tabs of the left panel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    /// Actors, logic nodes, map objects, strings and sound banks of the open mission.
    Objects,
    /// Missions found in the game folder.
    Missions,
    /// Skies, moods, vehicles and characters of the game folder (Parallel Lines: its packages).
    Assets,
}

/// Something picked in the Assets tab. Its preview is shown in the inspector.
#[derive(Clone, PartialEq)]
enum Asset {
    /// A Driv3r sky file name.
    Sky(String),
    /// A Driv3r mood, by its number.
    Mood(i16),
    /// A Driv3r vehicle model, by its id.
    Vehicle(u32),
    /// A character skin, by its id (see assets::skin_name).
    Skin(u32),
    /// Parallel Lines: (package, texture) and (package, model) of the open city file
    PlTex(usize, usize),
    PlModel(usize, usize),
}

/// The centre view.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Center {
    /// The logic graph of nodes and wires.
    Graph,
    /// The City 3D view.
    City,
}

/// Kind of a console line.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    /// Plain information.
    Info,
    /// A successful step.
    Ok,
    /// An error (shown in red).
    Err,
}

/// The whole application state, owned by eframe. It holds the open project and the selection, the view settings and caches, the queued edits, and the fields used by the screenshot tests.
struct App {
    // open file, selection, and the mission shown
    project: Option<Project>,
    sel: Sel,
    filter: String,
    // logic graph: pan, zoom, and the cached node positions
    pan: Vec2,
    zoom: f32,
    layout_cache: Option<(String, usize, Vec<(f32, f32)>)>,
    // console log and the Lua tab text
    log: Vec<(Level, String)>,
    lua_text: String,
    lua_path: Option<PathBuf>,
    // which tab is open in the centre and in the bottom panel
    bottom: Bottom,
    center: Center,
    // settings, and the scan of each game folder
    settings: Settings,
    scans: [Scan; 2],
    game: Game,
    show_settings: bool,
    left_tab: LeftTab,
    // edits queued by the panels; applied at the start of the next frame
    pending: Vec<Edit>,
    pl_landmarks: Vec<(String, glam::Vec3)>,
    mood_cache: (Option<PathBuf>, Option<mood::Mood>),
    // cutscene playback: time in seconds and the play state
    cam_t: f32,
    playing: bool,
    show_model: bool,
    show_add: bool,
    // theme: the Windows accent colour, and the theme last applied
    win_accent: Option<[u8; 3]>,
    theme_applied: Option<(bool, [u8; 3])>,
    accent_poll: std::time::Instant,
    accent_poll_start: std::time::Instant,
    // About window: open flag, selected tab, and the system information (filled by a thread)
    show_about: bool,
    about_tab: usize,
    sys_info: std::sync::Arc<std::sync::Mutex<Option<about::SysInfo>>>,
    sys_started: bool,
    // 3D view options (set in the View menu and the minimap)
    map_lighting: bool,
    minimap_on: bool,
    // Assets tab: the picked asset and its search text
    asset: Option<Asset>,
    asset_filter: String,
    // Parallel Lines assets: the opened city packages, the selected city and package, and the texture on show
    pl: plassets::PlStore,
    pl_city: usize,
    pl_pkg: usize,
    pl_tex: Option<((usize, usize, usize), egui::TextureHandle)>,
    // caches of the game folder: skies, vehicle ids of the city, character skins of the mission
    skies: Vec<String>,
    veh_cache: Option<(i32, Vec<u32>)>,
    skin_cache: Option<(String, Vec<u32>)>,
    sky_box: modelbox::SkyBox,
    // Add to mission window
    add_filter: String,
    add_sel: Option<usize>,
    add_args: String,
    add_wire: bool,
    // moods of the game folder: id and label
    mood_list: Vec<(i16, String)>,
    sky_on: bool,
    labels_on: bool,
    icon_tex: Option<egui::TextureHandle>,
    roads_on: bool,
    model_box: modelbox::ModelBox,
    // City 3D view state and the selection it last reacted to
    city: view3d::CityView,
    prev_sel: Sel,
    // automation (for screenshots)
    shot_path: Option<PathBuf>,
    shot_after: u32,
    frames: u32,
    shot_requested: bool,
    fit_pending: bool,
    shot_slot: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
}

/// Starting script of File > New Driv3r mission: a player character, a start node, a death check and a fail node. It is also the first text of the Lua tab.
const SAMPLE_LUA: &str = "-- Driv3r mission script\nMISSION.missionSummary.Level = \"Miami_Day\"\nlocal Player = MISSION.CreateCharacter(-1894.65, 1.06, -453.17, 0, SKIN_TANNER, 1, \"Player\", -1, -1, 1.0, 0.0, 0, 1.0)\nlocal Start = MISSION.LogicStart(\"Mission Name - Start\")\nlocal dead = MISSION.CharacterHasDied(Player)\nStart.wireCollection.Add(dead)\ndead.wireCollection.Add(MISSION.FailMission(0))\n";

impl App {
    /// Creates the application: fonts, the saved settings, the command line and the environment variables, then the files given on the command line.
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let mut app = App {
            project: None,
            sel: Sel::None,
            filter: String::new(),
            pan: Vec2::new(40.0, 40.0),
            zoom: 1.0,
            layout_cache: None,
            log: vec![(Level::Info, "Zartex'27 ready. Open a .mpc / .sp mission or a Lua script.".into())],
            lua_text: SAMPLE_LUA.into(),
            lua_path: None,
            bottom: Bottom::Console,
            center: Center::Graph,
            settings: Settings::load(),
            scans: [Scan::default(), Scan::default()],
            game: Game::Driv3r,
            show_settings: false,
            left_tab: LeftTab::Missions,
            pending: Vec::new(),
            pl_landmarks: Vec::new(),
            mood_cache: (None, None),
            cam_t: 0.0,
            playing: false,
            show_model: true,
            show_add: false,
            win_accent: theme::windows_accent(),
            theme_applied: None,
            accent_poll: std::time::Instant::now(),
            accent_poll_start: std::time::Instant::now(),
            show_about: false,
            about_tab: 0,
            sys_info: Default::default(),
            sys_started: false,
            map_lighting: false,
            minimap_on: true,
            asset: None,
            asset_filter: String::new(),
            pl: Default::default(),
            pl_city: 0,
            pl_pkg: 0,
            pl_tex: None,
            skies: Vec::new(),
            veh_cache: None,
            skin_cache: None,
            sky_box: modelbox::SkyBox::new(),
            add_filter: String::new(),
            add_sel: None,
            add_args: String::new(),
            add_wire: true,
            mood_list: Vec::new(),
            sky_on: true,
            labels_on: true,
            icon_tex: None,
            roads_on: false,
            model_box: modelbox::ModelBox::new(),
            city: view3d::CityView::new(),
            prev_sel: Sel::None,
            shot_path: None,
            shot_after: 0,
            frames: 0,
            shot_requested: false,
            fit_pending: true,
            shot_slot: Default::default(),
        };
        // graphics acceleration: install the hook, then apply the saved on/off switch
        gpu::install_hook();
        gpu::apply_preference(app.settings.gpu_off);
        app.rescan();
        // the settings window opens by itself when no game folder is set
        app.show_settings = app.settings.driv3r.is_none() && app.settings.pl.is_none();
        // command line (used by the tests): --screenshot FILE (save a PNG and close), --game DIR, --view3d,
        // --select-node N, --bottom console|lua|warnings|cameras, --time SECONDS, --mission N. Any other argument
        // that does not start with -- is a file to open.
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        let mut select_node = None;
        let mut mission = None;
        while i < args.len() {
            match args[i].as_str() {
                "--screenshot" => {
                    app.shot_path = args.get(i + 1).map(PathBuf::from);
                    app.shot_after = 14;
                    i += 1;
                }
                "--view3d" => app.center = Center::City,
                "--game" => {
                    if let Some(g) = args.get(i + 1) {
                        app.settings.driv3r = Some(PathBuf::from(g));
                        app.rescan();
                    }
                    i += 1;
                }
                "--select-node" => {
                    select_node = args.get(i + 1).and_then(|s| s.parse::<usize>().ok());
                    i += 1;
                }
                "--bottom" => {
                    app.bottom = match args.get(i + 1).map(|s| s.as_str()) {
                        Some("lua") => Bottom::Lua,
                        Some("warnings") => Bottom::Warnings,
                        Some("cameras") => Bottom::Cameras,
                        _ => Bottom::Console,
                    };
                    i += 1;
                }
                "--time" => {
                    app.cam_t = args.get(i + 1).and_then(|t| t.parse().ok()).unwrap_or(0.0);
                    i += 1;
                }
                "--mission" => {
                    mission = args.get(i + 1).and_then(|s| s.parse::<usize>().ok());
                    i += 1;
                }
                path if !path.starts_with("--") => app.open_path(PathBuf::from(path)),
                _ => {}
            }
            i += 1;
        }
        // --mission and --select-node are applied after the files are opened
        if let (Some(n), Some(p)) = (mission, &mut app.project) {
            p.current = n.min(p.views.len() - 1);
            app.layout_cache = None;
        }
        if let Some(n) = select_node {
            app.sel = Sel::Node(n);
        }
        // environment variables (used by the screenshot tests): ZM_SELECT_OBJECT and ZM_SELECT_ACTOR (an index),
        // ZM_APPEARANCE (light, dark or system), ZM_ABOUT (a tab number), ZM_SETTINGS (opens Settings),
        // ZM_ASSET (vehicle:ID, mood:ID, sky:NAME, pltex:P:I or plmodel:P:I) and ZM_EXPORT (readable or raw).
        if let Some(n) = std::env::var("ZM_SELECT_OBJECT").ok().and_then(|v| v.parse::<usize>().ok()) {
            app.sel = Sel::Object(n);
        }
        if let Some(a) = std::env::var("ZM_APPEARANCE").ok().and_then(|v| theme::Appearance::parse(&v)) {
            app.settings.appearance = a;
        }
        app.show_about = std::env::var("ZM_ABOUT").is_ok();
        if let Some(t) = std::env::var("ZM_ABOUT").ok().and_then(|v| v.parse::<usize>().ok()) {
            app.about_tab = t.min(2);
        }
        if std::env::var("ZM_SETTINGS").is_ok() {
            app.show_settings = true;
        }
        if let Ok(a) = std::env::var("ZM_ASSET") {
            app.left_tab = LeftTab::Assets;
            app.asset = Some(match a.split_once(':') {
                Some(("vehicle", id)) => Asset::Vehicle(id.parse().unwrap_or(9)),
                Some(("mood", id)) => Asset::Mood(id.parse().unwrap_or(1)),
                Some(("sky", n)) => Asset::Sky(n.to_string()),
                Some(("pltex", r)) | Some(("plmodel", r)) => {
                    let (p, i) = r.split_once(':').unwrap_or(("0", "0"));
                    let (p, i) = (p.parse().unwrap_or(0), i.parse().unwrap_or(0));
                    app.game = Game::ParallelLines;
                    app.pl_pkg = p;
                    if a.starts_with("pltex") { Asset::PlTex(p, i) } else { Asset::PlModel(p, i) }
                }
                _ => Asset::Sky("sky_policehq.d3s".into()),
            });
        }
        if let Some(n) = std::env::var("ZM_SELECT_ACTOR").ok().and_then(|v| v.parse::<usize>().ok()) {
            app.sel = Sel::Actor(n);
        }
        if let Ok(kind) = std::env::var("ZM_EXPORT") {
            app.export_lua(false, kind == "raw");
        }
        app
    }

    /// Reads both game folders: skies, moods, Parallel Lines start points, and the scan of each folder. Runs at start-up and after a folder changes.
    fn rescan(&mut self) {
        self.skies = self.settings.driv3r.as_deref().map(assets::skies).unwrap_or_default();
        // the skies are listed again; the vehicle and character caches are emptied so that they reload
        self.veh_cache = None;
        self.skin_cache = None;
        // moods the game offers: `Moods\moodNN.txt` with their time of day, weather and sky
        self.mood_list = self
            .settings
            .driv3r
            .as_deref()
            .map(|root| {
                let mut v: Vec<(i16, String)> = std::fs::read_dir(root.join("Moods"))
                    .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| {
                        let n = e.file_name().to_string_lossy().to_lowercase();
                        let id: i16 = n.strip_prefix("mood")?.strip_suffix(".txt")?.parse().ok()?;
                        let m = mood::Mood::load(&e.path())?;
                        let t = m.get("TIME_OF_DAY").unwrap_or("?").to_lowercase();
                        let w = m.get("WEATHER").unwrap_or("?").to_lowercase();
                        let sky = m.get("SKYDOME_NAME").map(|s| s.rsplit(['\\', '/']).next().unwrap_or(s).trim_end_matches(".d3s").trim_start_matches("sky_").to_string()).unwrap_or_default();
                        Some((id, format!("{id:02} \u{00B7} {t} \u{00B7} {w} \u{00B7} {sky}")))
                    }).collect())
                    .unwrap_or_default();
                v.sort();
                v
            })
            .unwrap_or_default();
        // Parallel Lines district start points, from Cities and levels.txt
        self.pl_landmarks = self.settings.pl.as_deref().map(|p| settings::pl_landmarks(p).into_iter().map(|(n, v)| (n, glam::Vec3::from(v))).collect()).unwrap_or_default();
        // scan result per game (index 0 Driv3r, 1 Parallel Lines); empty when no folder is set
        for (k, g) in [Game::Driv3r, Game::ParallelLines].into_iter().enumerate() {
            self.scans[k] = match self.settings.path(g) {
                Some(p) => settings::scan(g, p),
                None => Scan::default(),
            };
        }
    }

    /// Asks for the root folder of a game with a folder dialog, stores it and rescans. A cancelled dialog changes nothing.
    fn pick_path(&mut self, g: Game) {
        let title = match g {
            Game::Driv3r => "Select the root directory of your Driv3r installation",
            Game::ParallelLines => "Select the root directory of your Driver: Parallel Lines installation",
        };
        if let Some(p) = rfd::FileDialog::new().set_title(title).pick_folder() {
            match g {
                Game::Driv3r => self.settings.driv3r = Some(p),
                Game::ParallelLines => self.settings.pl = Some(p),
            }
            self.settings.save();
            self.rescan();
        }
    }

    /// Writes the open Driv3r mission as a Lua script: into the Lua tab, and optionally to a file. Readable (API calls,
    /// the default) or raw (every field kept, `MISSION.Raw*` calls).
    fn export_lua(&mut self, to_file: bool, raw: bool) {
        let Some(p) = &self.project else {
            self.log(Level::Err, "Open a Driv3r mission first.");
            return;
        };
        let v = p.view();
        if v.kind != GameKind::Driv3r {
            self.log(Level::Err, "Lua export works on Driv3r missions only.");
            return;
        }
        let title = std::path::Path::new(&p.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        // raw: every field as MISSION.Raw* calls; readable: API calls where possible and the rest raw
        let (text, note) = if raw {
            (zartex_core::luaraw::export_lua(&v.mission, v.summary.as_ref(), &title, false), "raw".to_string())
        } else {
            let (t, st) = zartex_core::readable::export_readable(&v.mission, v.summary.as_ref(), &title, false);
            (t, format!("readable: {} of {} actors and {} of {} nodes written as API calls, the rest raw", st.actors_readable, st.actors, st.nodes_readable, st.nodes))
        };
        let n = text.lines().count();
        self.lua_text = text.clone();
        self.lua_path = None;
        self.bottom = Bottom::Lua;
        self.log(Level::Ok, format!("Exported {title} as Lua ({n} lines, {note}): compile it again with Compile Lua"));
        if to_file {
            let stem = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "mission".into());
            if let Some(path) = rfd::FileDialog::new().add_filter("Lua", &["lua"]).set_file_name(format!("{stem}.lua")).save_file() {
                match std::fs::write(&path, text) {
                    Ok(()) => {
                        self.log(Level::Ok, format!("Saved {}", path.display()));
                        self.lua_path = Some(path);
                    }
                    Err(e) => self.log(Level::Err, format!("Cannot write: {e}")),
                }
            }
        }
    }

    /// Runs the Lua tab on the open Driv3r mission: new actors, nodes and wires are appended to it.
    fn add_snippet(&mut self) {
        let src = self.lua_text.clone();
        self.run_snippet(&src);
    }

    /// Runs Lua source against the open Driv3r mission: what it creates is appended to the mission. Parallel Lines missions are refused. On success the layout cache is cleared and the project is marked unsaved. On an error the console says the mission was not changed.
    fn run_snippet(&mut self, src: &str) {
        let Some(p) = &mut self.project else {
            self.log(Level::Err, "Open a Driv3r mission first.");
            return;
        };
        let cur = p.current.min(p.views.len() - 1);
        if p.views[cur].kind != GameKind::Driv3r {
            self.log(Level::Err, "Add to mission works on Driv3r missions only.");
            return;
        }
        let v = &mut p.views[cur];
        let (n0, a0) = (v.mission.logic.nodes.definitions.len(), v.mission.logic.actors.definitions.len());
        match zartex_core::compile::apply_snippet(&mut v.mission, &mut v.summary, src) {
            Ok(()) => {
                let (n1, a1) = (v.mission.logic.nodes.definitions.len(), v.mission.logic.actors.definitions.len());
                p.dirty = true;
                self.layout_cache = None;
                self.log(Level::Ok, format!("Added {} actors and {} nodes to the mission", a1 - a0, n1 - n0));
            }
            Err(e) => self.log(Level::Err, format!("{e} (the mission was not changed)")),
        }
    }

    /// Picks light or dark (the system's, when asked to follow it) and the accent colour, and applies them on change.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.appearance {
            theme::Appearance::Light => false,
            theme::Appearance::Dark => true,
            theme::Appearance::System => ctx.system_theme() == Some(egui::Theme::Dark),
        };
        // the Windows accent colour is read again at most every 20 seconds
        if self.settings.accent_windows && self.accent_poll.elapsed().as_secs() >= 20 {
            self.win_accent = theme::windows_accent();
            self.accent_poll = std::time::Instant::now();
        }
        let accent = if self.settings.accent_windows { self.win_accent.unwrap_or(self.settings.accent) } else { self.settings.accent };
        if self.theme_applied != Some((dark, accent)) {
            theme::apply(ctx, dark, accent);
            self.theme_applied = Some((dark, accent));
        }
    }

    /// Applies the edits queued during the previous frame to the open project, in the order they were queued.
    fn apply_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let edits = std::mem::take(&mut self.pending);
        // deleting a node or an actor renumbers the others, so the selection and the cached layout are reset
        if edits.iter().any(|e| matches!(e, Edit::DeleteNode(_) | Edit::DeleteActor(_))) {
            self.sel = Sel::None;
            self.prev_sel = Sel::None;
            self.layout_cache = None;
        }
        // the mission picker appears only when the file holds more than one mission
        if let Some(p) = &mut self.project {
            for e in edits {
                p.apply(e);
            }
        }
    }

    /// Adds a line to the console log.
    fn log(&mut self, level: Level, s: impl Into<String>) {
        self.log.push((level, s.into()));
    }

    /// Opens a file. A .lua script goes to the Lua tab and is compiled; a .mpc or .sp file is read as a mission. The selection and the view are reset.
    fn open_path(&mut self, path: PathBuf) {
        let p = path.to_string_lossy().to_string();
        let lower = p.to_lowercase();
        // Lua: loaded into the Lua tab, then compiled
        if lower.ends_with(".lua") {
            match std::fs::read_to_string(&path) {
                Ok(t) => {
                    self.lua_text = t;
                    self.lua_path = Some(path);
                    self.bottom = Bottom::Lua;
                    self.log(Level::Info, format!("Loaded script {p}"));
                    self.compile_lua();
                }
                Err(e) => self.log(Level::Err, format!("Cannot read {p}: {e}")),
            }
            return;
        }
        match std::fs::read(&path) {
            Ok(bytes) => match Project::open_bytes(&p, &bytes) {
                Ok(pr) => {
                    let (a, n, o) = counts(pr.view());
                    self.log(Level::Ok, format!("Opened {p}: {a} actors, {n} logic nodes, {o} objects"));
                    self.game = if pr.view().kind == GameKind::ParallelLines { Game::ParallelLines } else { Game::Driv3r };
                    self.project = Some(pr);
                    self.city.recenter();
                    self.sel = Sel::None;
                    self.layout_cache = None;
                    self.fit_pending = true;
                }
                Err(e) => self.log(Level::Err, format!("Cannot open {p}: {e}")),
            },
            Err(e) => self.log(Level::Err, format!("Cannot read {p}: {e}")),
        }
    }

    /// Compiles the Lua tab into a mission. Text with PACKAGE or INITMISSION is a Parallel Lines script, anything else a Driv3r script. The result replaces the open project and is marked unsaved.
    fn compile_lua(&mut self) {
        // the game is chosen from the text: PACKAGE or INITMISSION means Parallel Lines
        let dpl = self.lua_text.contains("PACKAGE") || self.lua_text.contains("INITMISSION");
        // compile the original file when the editor text is unchanged (keeps relative paths working)
        let same_as_file = self.lua_path.as_ref().map(|src| std::fs::read_to_string(src).map(|t| t == self.lua_text).unwrap_or(false)).unwrap_or(false);
        let tmp = if same_as_file {
            self.lua_path.clone().unwrap()
        } else {
            let t = std::env::temp_dir().join(if dpl { "zartex_m_script_dpl.lua" } else { "zartex_m_script.lua" });
            let _ = std::fs::write(&t, &self.lua_text);
            t
        };
        self.log(Level::Info, "------ Compilation started ------");
        let path = tmp.to_string_lossy().to_string();
        // the compiler returns the bytes of a package or a mission, which are then read back as a project
        let res = if dpl {
            zartex_core::api_dpl::compile_dpl_file(&path).map_err(|e| e.to_string()).and_then(|b| Project::from_compiled_sp("script", &b))
        } else {
            zartex_core::compile::compile_file(&path).map_err(|e| e.to_string()).and_then(|b| Project::from_compiled_mpc("script", &b))
        };
        match res {
            Ok(pr) => {
                let (a, n, o) = counts(pr.view());
                self.log(Level::Ok, format!("Compiled ({}): {a} actors, {n} logic nodes, {o} objects", if dpl { "Driver: PL" } else { "Driv3r" }));
                self.project = Some(pr);
                    self.city.recenter();
                self.sel = Sel::None;
                self.layout_cache = None;
                self.fit_pending = true;
            }
            Err(e) => {
                self.log(Level::Err, e);
                self.bottom = Bottom::Console;
            }
        }
    }

    /// Saves the open project under a new name. The edited views are copied back first. Parallel Lines packages of the game (lookup count_hi not zero) are refused, because saving would lose their extra data.
    fn save_as(&mut self) {
        let Some(p) = &mut self.project else { return };
        if matches!(&p.source, Source::Sp(k) if k.lookup.count_hi != 0) {
            self.log(Level::Err, "Saving the game's own Parallel Lines packages is not supported yet: their extra data would be lost. Compile a Lua script to build a package instead.");
            return;
        }
        // copy the edited views back into the container first (see model.rs)
        p.sync_to_source();
        let (bytes, ext) = match &mut p.source {
            Source::Mpc(mf) => (mf.to_bytes().map_err(|e| e.to_string()), "mpc"),
            Source::Sp(pkg) => (pkg.to_bytes().map_err(|e| e.to_string()), "sp"),
        };
        let dlg = rfd::FileDialog::new().add_filter("Mission", &[ext]).set_file_name(format!("mission.{ext}"));
        if let Some(path) = dlg.save_file() {
            let msg = match bytes {
                Ok(b) => match std::fs::write(&path, b) {
                    Ok(()) => {
                        p.dirty = false;
                        (Level::Ok, format!("Saved {}", path.display()))
                    }
                    Err(e) => (Level::Err, format!("Cannot write: {e}")),
                },
                Err(e) => (Level::Err, e),
            };
            self.log(msg.0, msg.1);
        }
    }

    /// Node positions of the logic graph, from model::layout. Kept in layout_cache until the file or the mission index changes.
    fn get_layout(&mut self) -> Vec<(f32, f32)> {
        let Some(p) = &self.project else { return vec![] };
        let key = (p.path.clone(), p.current);
        if let Some((k, c, l)) = &self.layout_cache {
            if *k == key.0 && *c == key.1 {
                return l.clone();
            }
        }
        let l = layout(p.view());
        self.layout_cache = Some((key.0, key.1, l.clone()));
        l
    }
}

/// Number of actors, logic nodes and map objects of a mission view, in that order.
fn counts(v: &MissionView) -> (usize, usize, usize) {
    (v.mission.logic.actors.definitions.len(), v.mission.logic.nodes.definitions.len(), v.mission.objects.objects.len())
}

// ---------------------------------------------------------------- UI

impl eframe::App for App {
    /// One frame: handles dropped files and screenshot requests, moves the playback on, applies the theme and the queued edits, then draws the panels and the windows.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;

        // a file dropped on the window is opened
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| Some(f.path().to_path_buf())).collect());
        if let Some(p) = dropped.into_iter().next() {
            self.open_path(p);
        }

        // screenshot test (--screenshot FILE): after a number of frames, and after ZM_SHOT_WAIT_MS, a screenshot is requested; when it arrives it is saved and the window closes
        if let Some(path) = self.shot_path.clone() {
            if self.frames >= self.shot_after && !self.shot_requested && self.accent_poll_start.elapsed().as_millis() >= std::env::var("ZM_SHOT_WAIT_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0) {
                self.shot_requested = true;
                let slot = self.shot_slot.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                    *slot.lock().unwrap() = Some(img);
                })));
            }
            let shot = self.shot_slot.lock().unwrap().take();
            if let Some(img) = shot {
                let (w, h) = (img.width() as u32, img.height() as u32);
                let mut raw = Vec::with_capacity((w * h * 4) as usize);
                for px in &img.pixels {
                    raw.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
                }
                let _ = image::save_buffer(&path, &raw, w, h, image::ColorType::Rgba8);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }

        // cutscene playback: the time moves on by the frame time, at most 0.1 s per frame
        if self.playing {
            self.cam_t += ctx.input(|i| i.stable_dt).min(0.1);
            ctx.request_repaint();
        }
        // order: theme, then the edits queued in the previous frame, then the panels
        self.apply_theme(&ctx);
        self.apply_pending();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        // an undecorated maximized window overhangs the screen by its resize border
        let pad = 0.0;
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(ui.max_rect().shrink(pad)));
        // the panels: title bar and toolbar, status line, bottom, left, right; then the centre
        self.top_bar(&mut inner);
        self.status_bar(&mut inner);
        self.bottom_panel(&mut inner);
        self.left_panel(&mut inner);
        self.right_panel(&mut inner);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::window_bg()).inner_margin(egui::Margin::same(8))).show_inside(&mut inner, |ui| {
            match self.center {
                Center::Graph => self.graph(ui),
                Center::City => self.city_tab(ui),
            }
        });
        // windows are drawn over the panels; each one clears its open flag when its close button is used
        self.settings_window(&ctx);
        self.add_window(&ctx);
        self.about_window(&ctx);
    }
}

impl App {
    /// The toolbar under the title bar: Open, Save as, Fit graph, the switch between the logic graph and City 3D, Compile Lua, and for a file with several missions a mission picker.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        self.title_bar(ui);
        egui::Panel::top("toolbar").frame(egui::Frame::new().fill(theme::card_bg()).stroke(Stroke::new(1.0, theme::card_stroke())).inner_margin(egui::Margin::symmetric(10, 6))).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                flat_buttons(ui);
                if ui.button("Open").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Mission / script", &["mpc", "sp", "lua"]).pick_file() {
                        self.open_path(p);
                    }
                }
                if ui.button("Save as").clicked() {
                    self.save_as();
                }
                ui.separator();
                if ui.button("Fit graph").clicked() {
                    self.fit_pending = true;
                }
                ui.separator();
                if tab_button(ui, self.center == Center::Graph, "Logic graph").clicked() {
                    self.center = Center::Graph;
                }
                if tab_button(ui, self.center == Center::City, "City 3D").clicked() {
                    self.center = Center::City;
                }
                ui.separator();
                ui.separator();
                // Compile Lua compiles the Lua tab (see compile_lua)
                let run = egui::Button::new(RichText::new("Compile Lua").color(Color32::WHITE)).fill(theme::accent());
                if ui.add(run).clicked() {
                    self.compile_lua();
                }
                let mut changed = false;
                if let Some(p) = &mut self.project {
                    if p.views.len() > 1 {
                        ui.separator();
                        ui.label(RichText::new("Mission").color(theme::text_dim()));
                        let cur = p.views[p.current].title.clone();
                        egui::ComboBox::from_id_salt("missionpick").selected_text(cur).show_ui(ui, |ui| {
                            for i in 0..p.views.len() {
                                let t = p.views[i].title.clone();
                                if ui.selectable_label(p.current == i, t).clicked() {
                                    p.current = i;
                                    changed = true;
                                }
                            }
                        });
                    }
                    // the city name comes from the summary (Driv3r missions only)
                    if let Some(s) = &p.view().summary {
                        ui.separator();
                        let city = match s.city_type {
                            1 => "Miami (day)",
                            2 => "Miami (night)",
                            3 => "Nice (day)",
                            4 => "Nice (night)",
                            5 => "Istanbul (day)",
                            6 => "Istanbul (night)",
                            _ => "Unknown city",
                        };
                        ui.label(RichText::new(city).color(theme::text_dim()));
                    }
                }
                if changed {
                    self.layout_cache = None;
                    self.fit_pending = true;
                    self.sel = Sel::None;
                    self.city.recenter();
                }
            });
        });
    }

    /// The status line at the bottom of the window: game, counts of the open mission, and whether there are unsaved changes.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(theme::bar_bg()).inner_margin(egui::Margin::symmetric(12, 3))).show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                let small = |t: String| RichText::new(t).size(11.5).color(theme::text_dim());
                match &self.project {
                    Some(p) => {
                        let v = p.view();
                        let (a, n, o) = counts(v);
                        ui.label(small(if v.kind == GameKind::Driv3r { "Driv3r".into() } else { "Driver: Parallel Lines".into() }));
                        ui.separator();
                        ui.label(small(format!("{a} actors \u{00B7} {n} nodes \u{00B7} {o} objects \u{00B7} {} strings", v.mission.logic.strings.strings.len())));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(small(if p.dirty { "Unsaved changes".into() } else { "Saved".into() }));
                        });
                    }
                    None => {
                        ui.label(small("No mission loaded".into()));
                    }
                }
            });
        });
    }

    /// The bottom panel: tabs for the console, cameras, the Lua script and the warnings. The buttons of the Lua tab only set flags; the flags are acted on after the panel is drawn.
    fn bottom_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("bottom")
            .size_range(90.0..=600.0)
            .resizable(true)
            .default_size(170.0)
            .frame(egui::Frame::new().fill(theme::card_bg()).stroke(Stroke::new(1.0, theme::card_stroke())).inner_margin(egui::Margin::same(8)))
            .show_inside(ui, |ui| {
                // the number of warnings is shown in the tab name
                let nwarn = self.project.as_ref().map(|p| warnings(p.view()).len()).unwrap_or(0);
                let mut compile = false;
                let mut open_script = false;
                let mut add_snippet = false;
                ui.horizontal(|ui| {
                    for (b, name) in [(Bottom::Console, "Console".to_string()), (Bottom::Cameras, "Cameras".to_string()), (Bottom::Lua, "Lua script".to_string()), (Bottom::Warnings, format!("Warnings ({nwarn})"))] {
                        if tab_button(ui, self.bottom == b, &name).clicked() {
                            self.bottom = b;
                        }
                    }
                    if self.bottom == Bottom::Lua {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(RichText::new("Run").color(Color32::WHITE)).fill(theme::accent())).clicked() {
                                compile = true;
                            }
                            if ui.button("Open script...").clicked() {
                                open_script = true;
                            }
                            if ui.button("Add to mission").on_hover_text("Run this Lua on the open Driv3r mission: what it creates is appended, and MISSION.GetNode(i) / MISSION.GetActor(i) reach the existing nodes and actors").clicked() {
                                add_snippet = true;
                            }
                        });
                    }
                });
                ui.separator();
                // the body of the selected tab; the Cameras tab is drawn by cameras_panel
                match self.bottom {
                    Bottom::Console => {
                        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                            for (lvl, s) in &self.log {
                                let (c, tag) = match lvl {
                                    Level::Info => (theme::text_dim(), "      "),
                                    Level::Ok => (Color32::from_rgb(0x0F, 0x7B, 0x0F), "[ok]  "),
                                    Level::Err => (Color32::from_rgb(0xC4, 0x2B, 0x1C), "[err] "),
                                };
                                ui.label(RichText::new(format!("{tag} {s}")).monospace().color(c));
                            }
                        });
                    }
                    Bottom::Lua => {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            ui.add(egui::TextEdit::multiline(&mut self.lua_text).code_editor().desired_width(f32::INFINITY).desired_rows(8));
                        });
                    }
                    Bottom::Cameras => self.cameras_panel(ui),
                    Bottom::Warnings => {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            match &self.project {
                                Some(p) => {
                                    let w = warnings(p.view());
                                    if w.is_empty() {
                                        ui.label(RichText::new("No warnings.").color(theme::text_dim()));
                                    }
                                    for s in w {
                                        ui.label(RichText::new(format!("\u{26A0}  {s}")).color(Color32::from_rgb(0xC1, 0x7A, 0x00)));
                                    }
                                }
                                None => {
                                    ui.label(RichText::new("Open a mission to see warnings.").color(theme::text_dim()));
                                }
                            };
                        });
                    }
                }
                // the flags of the buttons are acted on here, after the tab bodies have been drawn
                if open_script {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Lua", &["lua"]).pick_file() {
                        self.open_path(p);
                    }
                }
                if compile {
                    self.compile_lua();
                }
                if add_snippet {
                    self.add_snippet();
                }
            });
    }

    /// The left panel: the Driv3r / Parallel Lines switch and the Missions, Objects and Assets tabs. The Objects tab lists the actors, logic nodes, map objects, strings and sound banks of the open mission, with a search box.
    fn left_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("objects")
            .resizable(true)
            .default_size(310.0)
            .size_range(280.0..=520.0)
            .frame(egui::Frame::new().fill(theme::card_bg()).stroke(Stroke::new(1.0, theme::card_stroke())).inner_margin(egui::Margin::same(8)))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if tab_button(ui, self.game == Game::Driv3r, "Driv3r").clicked() {
                        self.game = Game::Driv3r;
                    }
                    if tab_button(ui, self.game == Game::ParallelLines, "Parallel Lines").clicked() {
                        self.game = Game::ParallelLines;
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if tab_button(ui, self.left_tab == LeftTab::Missions, "Missions").clicked() {
                        self.left_tab = LeftTab::Missions;
                    }
                    if tab_button(ui, self.left_tab == LeftTab::Objects, "Objects").clicked() {
                        self.left_tab = LeftTab::Objects;
                    }
                    if tab_button(ui, self.left_tab == LeftTab::Assets, "Assets").clicked() {
                        self.left_tab = LeftTab::Assets;
                    }
                });
                ui.add_space(2.0);
                // the Missions and Assets tabs draw their own lists and end here
                if self.left_tab == LeftTab::Missions {
                    self.missions_tab(ui);
                    return;
                }
                if self.left_tab == LeftTab::Assets {
                    self.assets_tab(ui);
                    return;
                }
                if ui.button("+ Add actor or node...").clicked() {
                    self.show_add = true;
                }
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search objects").desired_width(f32::INFINITY));
                ui.add_space(4.0);
                ui.label(RichText::new("MISSION OBJECTS").size(11.0).color(theme::text_dim()));
                let Some(p) = &self.project else {
                    ui.add_space(12.0);
                    ui.label(RichText::new("Nothing loaded.\nDrop a .mpc, .sp or .lua file here.").color(theme::text_dim()));
                    return;
                };
                let v = p.view();
                let f = self.filter.to_lowercase();
                let hit = |s: &str| f.is_empty() || s.to_lowercase().contains(&f);
                // a click on a list entry is kept here and applied after the scroll area is drawn
                let mut new_sel = None;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let na = v.mission.logic.actors.definitions.len();
                    egui::CollapsingHeader::new(format!("Actors ({na})")).default_open(true).show(ui, |ui| {
                        for i in 0..na {
                            let l = actor_label(v, i);
                            if hit(&l) && ui.selectable_label(self.sel == Sel::Actor(i), &l).clicked() {
                                new_sel = Some(Sel::Actor(i));
                            }
                        }
                    });
                    let nn = v.mission.logic.nodes.definitions.len();
                    egui::CollapsingHeader::new(format!("Logic nodes ({nn})")).default_open(true).show(ui, |ui| {
                        for i in 0..nn {
                            let l = node_label(v, i);
                            if hit(&l) && ui.selectable_label(self.sel == Sel::Node(i), &l).clicked() {
                                new_sel = Some(Sel::Node(i));
                            }
                        }
                    });
                    let no = v.mission.objects.objects.len();
                    egui::CollapsingHeader::new(format!("Objects ({no})")).show(ui, |ui| {
                        for i in 0..no {
                            let l = object_label(&v.mission.objects.objects[i]);
                            if hit(&l) && ui.selectable_label(self.sel == Sel::Object(i), &l).clicked() {
                                new_sel = Some(Sel::Object(i));
                            }
                        }
                    });
                    egui::CollapsingHeader::new(format!("Strings ({})", v.mission.logic.strings.strings.len())).show(ui, |ui| {
                        for (i, s) in v.mission.logic.strings.strings.iter().enumerate() {
                            if hit(s) {
                                ui.label(RichText::new(format!("{i:>3}  {s}")).monospace().size(12.0).color(theme::text_dim()));
                            }
                        }
                    });
                    egui::CollapsingHeader::new(format!("Sound banks ({})", v.mission.logic.sound_banks.items.len())).show(ui, |ui| {
                        for b in &v.mission.logic.sound_banks.items {
                            ui.label(format!("bank {b}"));
                        }
                    });
                });
                if let Some(s) = new_sel {
                    self.sel = s;
                }
            });
    }

    /// The inspector on the right: the asset preview when an asset is picked; otherwise the overview, or the details, wires, properties and delete button of the selected node, actor or object.
    fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(340.0)
            .frame(egui::Frame::new().fill(theme::card_bg()).stroke(Stroke::new(1.0, theme::card_stroke())).inner_margin(egui::Margin::same(10)))
            .size_range(320.0..=580.0)
            .show_inside(ui, |ui| {
                // the mood file of the open mission is read again only when its path changes
                let mood_path = self.project.as_ref().and_then(|p| {
                    let id = p.view().summary.as_ref()?.mission_id;
                    Some(self.settings.driv3r.as_ref()?.join("Moods").join(format!("mood{id:02}.txt")))
                });
                if self.mood_cache.0 != mood_path {
                    self.mood_cache = (mood_path.clone(), mood_path.and_then(|m| mood::Mood::load(&m)));
                }
                // an asset picked in the Assets tab replaces the inspector content
                if self.left_tab == LeftTab::Assets && self.asset.is_some() {
                    self.asset_preview(ui);
                    return;
                }
                let Some(p) = &self.project else {
                    ui.label(RichText::new("Inspector").strong());
                    return;
                };
                let v = p.view();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.sel {
                    // nothing selected: the mission overview (overview::show)
                    Sel::None => {
                        overview::show(ui, v, self.mood_cache.1.as_ref(), None, &mut self.pending, &self.mood_list);
                    }
                    // a logic node: id, type, note, flags, the wires it sends, and its properties
                    Sel::Node(i) => {
                        let Some(n) = v.mission.logic.nodes.definitions.get(i) else { return };
                        ui.label(RichText::new(format!("Inspector \u{00B7} {}", node_type_name(v, n.type_id))).strong());
                        ui.add_space(4.0);
                        kv(ui, "Node id", i.to_string());
                        kv(ui, "Type", format!("{} ({})", node_type_name(v, n.type_id), n.type_id));
                        kv(ui, "Note", string_of(v, n.string_id));
                        kv(ui, "Flags", format!("{:#06x}", n.flags));
                        if let Some(w) = wire_index(n) {
                            let count = v.mission.logic.nodes.definitions.len();
                            ui.add_space(6.0);
                            ui.label(RichText::new("WIRES OUT").size(11.0).color(theme::text_dim()));
                            let wires: Vec<zartex_core::logic::WireNode> = v.mission.logic.wires.collections.get(w).map(|c| c.wires.clone()).unwrap_or_default();
                            for (k, x) in wires.iter().enumerate() {
                                if (x.node_id as usize) >= count {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    ui.label(format!("\u{2192} {}", node_label(v, x.node_id as usize)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.small_button("\u{2715}").on_hover_text("Remove this wire").clicked() {
                                            self.pending.push(Edit::WireRemove(i, k));
                                        }
                                        let mut t = x.wire_type as i32;
                                        if ui.add(egui::DragValue::new(&mut t).range(0..=255).prefix("type ")).changed() {
                                            self.pending.push(Edit::WireType(i, k, t as u8));
                                        }
                                    });
                                });
                            }
                            // add a wire
                            let sel_id = egui::Id::new(("wire_target", i));
                            let mut target: usize = ui.data_mut(|d| d.get_temp::<usize>(sel_id)).unwrap_or(0).min(count.saturating_sub(1));
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt(sel_id).width(150.0).selected_text(format!("{target}: {}", node_label(v, target))).show_ui(ui, |ui| {
                                    for t in 0..count {
                                        ui.selectable_value(&mut target, t, format!("{t}: {}", node_label(v, t)));
                                    }
                                });
                                if ui.button("Add wire").clicked() {
                                    self.pending.push(Edit::WireAdd(i, target, 1));
                                }
                            });
                            ui.data_mut(|d| d.insert_temp(sel_id, target));
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("PROPERTIES").size(11.0).color(theme::text_dim()));
                        propedit::show(ui, v, &n.properties, 1, Owner::Node(i), &mut self.pending);
                        {
                            // deleting needs two clicks: the first arms the button, the second deletes the node
                            let did = egui::Id::new(("del", i, 0));
                            let armed = ui.data_mut(|d| d.get_temp::<bool>(did)).unwrap_or(false);
                            ui.add_space(10.0);
                            if !armed {
                                if ui.button("Delete node...").clicked() {
                                    ui.data_mut(|d| d.insert_temp(did, true));
                                }
                            } else {
                                ui.label(RichText::new("Delete this node? Wires into it are removed and later nodes are renumbered.").size(12.0).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
                                ui.horizontal(|ui| {
                                    if ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(0xC4, 0x2B, 0x1C))).clicked() {
                                        self.pending.push(Edit::DeleteNode(i));
                                        ui.data_mut(|d| d.remove_temp::<bool>(did));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        ui.data_mut(|d| d.remove_temp::<bool>(did));
                                    }
                                });
                            }
                        }
                    }
                    // an actor: id, type, note, object id and flags; then the 3D model, position, properties and delete
                    Sel::Actor(i) => {
                        let Some(a) = v.mission.logic.actors.definitions.get(i) else { return };
                        ui.label(RichText::new(format!("Inspector \u{00B7} {}", actor_type_name(a.type_id))).strong());
                        ui.add_space(4.0);
                        kv(ui, "Actor id", i.to_string());
                        kv(ui, "Type", format!("{} ({})", actor_type_name(a.type_id), a.type_id));
                        kv(ui, "Note", string_of(v, a.string_id));
                        kv(ui, "Object id", a.object_id.to_string());
                        kv(ui, "Flags", format!("{:#06x}", a.flags));
                        // the 3D box also shows when the vehicle or character is picked from the Actors list
                        if v.kind == GameKind::Driv3r {
                            let obj = v.mission.objects.objects.get(a.object_id.max(0) as usize);
                            let is_model = matches!(obj, Some(zartex_core::objects::MissionObject::Vehicle { .. }) | Some(zartex_core::objects::MissionObject::Character { .. }));
                            if is_model {
                                ui.add_space(4.0);
                                ui.checkbox(&mut self.show_model, "3D model");
                                if self.show_model {
                                    let stem = std::path::Path::new(&p.path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                                    match (self.settings.driv3r.as_deref(), obj, v.summary.as_ref()) {
                                        (Some(root), Some(zartex_core::objects::MissionObject::Vehicle { uid, .. }), Some(sm)) => self.model_box.vehicle(ui, root, sm.city_type, (*uid).max(0) as u32),
                                        (Some(root), Some(zartex_core::objects::MissionObject::Character { skin_id, .. }), _) => self.model_box.character(ui, root, &stem, *skin_id),
                                        _ => {
                                            ui.label(RichText::new("Needs the Driv3r game folder (File > Settings).").size(12.0).color(theme::text_dim()));
                                        }
                                    }
                                }
                                ui.add_space(6.0);
                            }
                        }
                        // the actor's position (its Matrix property); a change is queued as ActorPos
                        if let Some(mut pos) = actor_position(a) {
                            ui.add_space(6.0);
                            ui.label(RichText::new("POSITION").size(11.0).color(theme::text_dim()));
                            let mut changed = false;
                            for (k, name) in ["X", "Y (height)", "Z"].iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(*name);
                                    changed |= ui.add(egui::DragValue::new(&mut pos[k]).speed(0.1).max_decimals(3)).changed();
                                });
                            }
                            if changed {
                                self.pending.push(Edit::ActorPos(i, pos));
                            }
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("PROPERTIES").size(11.0).color(theme::text_dim()));
                        propedit::show(ui, v, &a.properties, 0, Owner::Actor(i), &mut self.pending);
                        {
                            // the same two-click delete as for nodes
                            let did = egui::Id::new(("del", i, 1));
                            let armed = ui.data_mut(|d| d.get_temp::<bool>(did)).unwrap_or(false);
                            ui.add_space(10.0);
                            if !armed {
                                if ui.button("Delete actor...").clicked() {
                                    ui.data_mut(|d| d.insert_temp(did, true));
                                }
                            } else {
                                ui.label(RichText::new("Delete this actor? References to it become none and later actors are renumbered.").size(12.0).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
                                ui.horizontal(|ui| {
                                    if ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(Color32::from_rgb(0xC4, 0x2B, 0x1C))).clicked() {
                                        self.pending.push(Edit::DeleteActor(i));
                                        ui.data_mut(|d| d.remove_temp::<bool>(did));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        ui.data_mut(|d| d.remove_temp::<bool>(did));
                                    }
                                });
                            }
                        }
                    }
                    // a map object: index, kind, position, and a 3D model for vehicles and characters
                    Sel::Object(i) => {
                        let Some(o) = v.mission.objects.objects.get(i) else { return };
                        ui.label(RichText::new("Inspector \u{00B7} Object").strong());
                        ui.add_space(4.0);
                        kv(ui, "Index", i.to_string());
                        kv(ui, "Kind", object_label(o));
                        if let Some(mut pos) = object_position(o) {
                            ui.add_space(6.0);
                            ui.label(RichText::new("POSITION").size(11.0).color(theme::text_dim()));
                            let mut changed = false;
                            for (k, name) in ["X", "Y (height)", "Z"].iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(*name);
                                    changed |= ui.add(egui::DragValue::new(&mut pos[k]).speed(0.1).max_decimals(3)).changed();
                                });
                            }
                            if changed {
                                self.pending.push(Edit::ObjectPos(i, pos));
                            }
                            ui.add_space(6.0);
                        }
                        if let zartex_core::objects::MissionObject::Vehicle { uid, .. } = o {
                            ui.add_space(4.0);
                            ui.checkbox(&mut self.show_model, "3D model of this vehicle");
                            if self.show_model {
                                match (self.settings.driv3r.as_deref(), v.summary.as_ref()) {
                                    (Some(root), Some(s)) => self.model_box.vehicle(ui, root, s.city_type, (*uid).max(0) as u32),
                                    _ => {
                                        ui.label(RichText::new("Needs the Driv3r game folder (File > Settings).").size(12.0).color(theme::text_dim()));
                                    }
                                }
                            }
                            ui.add_space(6.0);
                        }
                        if let zartex_core::objects::MissionObject::Character { skin_id, .. } = o {
                            ui.add_space(4.0);
                            ui.checkbox(&mut self.show_model, "3D model of this character");
                            if self.show_model {
                                let stem = std::path::Path::new(&p.path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                                match self.settings.driv3r.as_deref() {
                                    Some(root) if p.view().kind == GameKind::Driv3r => self.model_box.character(ui, root, &stem, *skin_id),
                                    _ => {
                                        ui.label(RichText::new("Needs a Driv3r mission and the Driv3r game folder (File > Settings).").size(12.0).color(theme::text_dim()));
                                    }
                                }
                            }
                            ui.add_space(6.0);
                        }
                        // a Camera object: its aim is edited as heading and tilt, computed from the negated forward vector (see camera.rs)
                        if let zartex_core::objects::MissionObject::Camera { forward, .. } = o {
                            let (mut yaw, mut pitch) = camera::yaw_pitch([-forward.x, -forward.y, -forward.z]);
                            ui.label(RichText::new("AIM").size(11.0).color(theme::text_dim()));
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("Heading");
                                changed |= ui.add(egui::DragValue::new(&mut yaw).speed(0.5).suffix(" deg")).changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Tilt");
                                changed |= ui.add(egui::DragValue::new(&mut pitch).speed(0.3).range(-89.0..=89.0).suffix(" deg")).changed();
                            });
                            if changed {
                                self.pending.push(Edit::CameraForward(i, camera::dir_from_yaw_pitch(yaw, pitch)));
                            }
                            ui.label(RichText::new("Drag the white dot of a camera in City 3D to turn it.").size(11.0).color(theme::text_dim()));
                            ui.add_space(6.0);
                        }
                        // the raw data of the object, for reference
                        ui.label(RichText::new(format!("{o:#?}")).monospace().size(11.0).color(theme::text_dim()));
                    }
                });
            });
    }

    /// The City 3D view in the centre. Parallel Lines missions show their actors as map objects; Driv3r missions show the mission's objects on the city map and need the game folder. Clicks and drags become selections and queued edits.
    fn city_tab(&mut self, ui: &mut egui::Ui) {
        let Some(p) = &self.project else {
            ui.centered_and_justified(|ui| ui.label("Open a mission first (Missions tab on the left)."));
            return;
        };
        let mv = p.view();
        // the game folder of the mission's own game is used for the city models
        let (game, game_root) = match mv.kind {
            GameKind::Driv3r => (Game::Driv3r, self.settings.driv3r.clone()),
            GameKind::ParallelLines => (Game::ParallelLines, self.settings.pl.clone()),
        };
        if mv.kind == GameKind::ParallelLines {
            // Parallel Lines: actors are drawn as map objects (model::actors_as_objects), so the object index is the actor index
            let objs_vec = actors_as_objects(mv);
            let objs = &objs_vec;
            if self.sel != self.prev_sel {
                self.prev_sel = self.sel;
                if let Sel::Actor(i) = self.sel {
                    if let Some(pos) = objs.get(i).and_then(view3d::object_pos) {
                        self.city.focus = Some(pos);
                    }
                }
            }
            if self.city.cam.target == glam::Vec3::ZERO {
                self.city.focus = objs.iter().find_map(view3d::object_pos).or_else(|| self.pl_landmarks.first().map(|l| l.1));
            }
            let selected = if let Sel::Actor(i) = self.sel { Some(i) } else { None };
            let lm = self.pl_landmarks.clone();
            // cutscene preview: the shot path is drawn only while the Cameras tab is open; Parallel Lines has no preview camera
            let sh = if self.bottom == Bottom::Cameras { camera::shots(mv) } else { Vec::new() };
            let path: Vec<(glam::Vec3, usize)> = sh.iter().enumerate().map(|(k, s)| (glam::Vec3::from(s.pose.pos), k + 1)).collect();
            let extras = view3d::CamExtras { preview: None, path: &path, sky: None, labels: self.labels_on, roads: self.roads_on, preview_light: None, map_light: None, minimap: self.minimap_on };
            let then = std::path::Path::new(&p.path).file_stem().map(|n| n.to_string_lossy().to_lowercase().contains("then")).unwrap_or(false);
            match self.city.ui(ui, game_root.as_deref(), then as i32, objs, None, selected, Some(&lm), &extras) {
                Some(view3d::CityEvent::Click(i)) => {
                    self.sel = Sel::Actor(i);
                    self.prev_sel = self.sel;
                }
                Some(view3d::CityEvent::MoveObject(i, v)) => {
                    self.sel = Sel::Actor(i);
                    self.prev_sel = self.sel;
                    self.pending.push(Edit::ActorPos(i, v.to_array()));
                }
                _ => {}
            }
            return;
        }
        // Driv3r needs the game folder for the city models
        if game_root.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new("The game folder is not set").size(16.0));
                ui.add_space(10.0);
                if ui.button("Open settings").clicked() {
                    self.show_settings = true;
                }
            });
            return;
        }
        let _ = game;
        let city = mv.summary.as_ref().map(|s| s.city_type).unwrap_or(0);
        let objs = &mv.mission.objects.objects;
        let first_char = objs.iter().find_map(|o| if let zartex_core::objects::MissionObject::Character { position, .. } = o { Some(position) } else { None });
        // the player start from the summary, at the height of the first character; the view starts there
        let start = mv.summary.as_ref().map(|s| glam::Vec3::new(s.start_position.x, first_char.map(|c| c.y).unwrap_or(0.0), s.start_position.y));
        if start.is_some() && self.city.cam.target == glam::Vec3::ZERO {
            self.city.focus = start;
        }
        if self.sel != self.prev_sel {
            self.prev_sel = self.sel;
            if let Sel::Object(i) = self.sel {
                if let Some(pos) = objs.get(i).and_then(view3d::object_pos) {
                    self.city.focus = Some(pos);
                }
            }
        }
        let selected = if let Sel::Object(i) = self.sel { Some(i) } else { None };
        // cutscene preview (Driv3r): while the Cameras tab is open, the shots are drawn as a numbered path
        let sh = if self.bottom == Bottom::Cameras { camera::shots(mv) } else { Vec::new() };
        let path: Vec<(glam::Vec3, usize)> = sh.iter().enumerate().map(|(k, s)| (glam::Vec3::from(s.pose.pos), k + 1)).collect();
        // the camera pose at the playback time (camera::pose_at); none when the Cameras tab is closed
        let preview = if self.bottom == Bottom::Cameras { camera::pose_at(&sh, self.cam_t) } else { None };
        // the mood's sky is used in the preview only when 'Mission sky in preview' is ticked
        let sky_name: Option<String> = if self.sky_on && preview.is_some() { self.mood_cache.1.as_ref().and_then(|m| m.get("SKYDOME_NAME")).map(|x| x.to_string()) } else { None };
        // the mood's lighting: the preview always uses it; the map uses it only when the View menu option is on
        let light = self.mood_cache.1.as_ref().and_then(|m| m.lighting());
        let extras = view3d::CamExtras { preview, path: &path, sky: sky_name.as_deref(), labels: self.labels_on, roads: false, preview_light: light.clone(), map_light: if self.map_lighting { light } else { None }, minimap: self.minimap_on };
        match self.city.ui(ui, game_root.as_deref(), city, objs, start, selected, None, &extras) {
            Some(view3d::CityEvent::Click(i)) => {
                self.sel = Sel::Object(i);
                self.prev_sel = self.sel;
            }
            Some(view3d::CityEvent::MoveObject(i, v)) => {
                self.sel = Sel::Object(i);
                self.prev_sel = self.sel;
                self.pending.push(Edit::ObjectPos(i, v.to_array()));
            }
            // events from the 3D view: a click selects; a move or an aim queues an Edit
            Some(view3d::CityEvent::MoveStart(v)) => self.pending.push(Edit::StartPos(v.x, v.z)),
            Some(view3d::CityEvent::AimCamera(i, d)) => {
                self.sel = Sel::Object(i);
                self.prev_sel = self.sel;
                self.pending.push(Edit::CameraForward(i, d.to_array()));
            }
            None => {}
        }
    }

    /// The program icon as a texture (title bar, About).
    fn icon_id(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        if self.icon_tex.is_none() {
            // the PNG is decoded and scaled to 128 px once, then kept as a texture
            if let Ok(i) = image::load_from_memory(include_bytes!("../assets/icon.png")) {
                let i = i.resize(128, 128, image::imageops::FilterType::Lanczos3).to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
                self.icon_tex = Some(ctx.load_texture("app_icon", ci, egui::TextureOptions::LINEAR));
            }
        }
        self.icon_tex.as_ref().map(|t| t.id())
    }

    /// The title bar: icon, the menus File, View, Lua and Help, the file name with an unsaved marker, and the minimise, maximise and close buttons. The window has no system decorations, so the dragging and these buttons are drawn here.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let icon = self.icon_id(&ctx);
        egui::Panel::top("titlebar").frame(egui::Frame::new().fill(theme::bar_bg()).inner_margin(egui::Margin { left: 10, right: 0, top: 0, bottom: 0 })).show_inside(ui, |ui| {
            ui.set_height(34.0);
            ui.horizontal_centered(|ui| {
                // no gaps: the window buttons must end exactly at the screen edge
                ui.spacing_mut().item_spacing.x = 0.0;
                flat_buttons(ui);
                match icon {
                    Some(id) => {
                        ui.add(egui::Image::new((id, Vec2::splat(24.0))));
                    }
                    None => {
                        ui.label(RichText::new("\u{25A0}").color(theme::accent()).size(15.0));
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Zartex'27").strong());
                ui.add_space(12.0);
                // menus; each item closes its menu with ui.close()
                menu_item(ui, "File", |ui| {
                    if ui.button("New Driv3r mission").clicked() {
                        self.lua_text = SAMPLE_LUA.into();
                        self.compile_lua();
                        self.bottom = Bottom::Lua;
                        ui.close();
                    }
                    if ui.button("Open mission or script...").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Mission / script", &["mpc", "sp", "lua"]).pick_file() {
                            self.open_path(p);
                        }
                        ui.close();
                    }
                    if ui.button("Save").clicked() {
                        self.save_in_place();
                        ui.close();
                    }
                    if ui.button("Save as...").clicked() {
                        self.save_as();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Settings...").clicked() {
                        self.show_settings = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                menu_item(ui, "View", |ui| {
                    if ui.button("Logic graph").clicked() {
                        self.center = Center::Graph;
                        ui.close();
                    }
                    if ui.button("City 3D").clicked() {
                        self.center = Center::City;
                        ui.close();
                    }
                    if ui.button("Fit graph").clicked() {
                        self.fit_pending = true;
                        ui.close();
                    }
                    ui.checkbox(&mut self.minimap_on, "Minimap over the 3D view");
                    ui.separator();
                    ui.checkbox(&mut self.map_lighting, "Mission lighting and fog on the map (heavier)").on_hover_text("Day or night, light colours and fog of the mission's mood. The camera preview always uses them.");
                });
                menu_item(ui, "Lua", |ui| {
                    if ui.button("Compile script").clicked() {
                        self.compile_lua();
                        ui.close();
                    }
                    if ui.button("Export mission as Lua (to the Lua tab)").on_hover_text("Readable: MISSION.CreateCharacter(...), MISSION.ActorCreation(...), wires. Objects keep position and parameters, not internal bytes. What the API cannot express stays as raw calls.").clicked() {
                        self.export_lua(false, false);
                        ui.close();
                    }
                    if ui.button("Export mission as Lua file...").clicked() {
                        self.export_lua(true, false);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Export raw Lua (to the Lua tab)").on_hover_text("Exact: every field of every actor, node and object as MISSION.Raw* calls.").clicked() {
                        self.export_lua(false, true);
                        ui.close();
                    }
                    if ui.button("Export raw Lua file...").clicked() {
                        self.export_lua(true, true);
                        ui.close();
                    }
                });
                menu_item(ui, "Help", |ui| {
                    match about::DOCS_URL {
                        Some(u) => {
                            if ui.button("Documentation").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(u));
                                ui.close();
                            }
                        }
                        None => {
                            ui.add_enabled(false, egui::Button::new("Documentation")).on_disabled_hover_text("The documentation link will be added with the repository");
                        }
                    }
                    if ui.button("About Zartex'27").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                    ui.separator();
                    ui.label(RichText::new("Drag a marker in City 3D to move it. Shift-drag changes its height.").color(theme::text_dim()));
                });
                // space kept at the right end for the three window buttons (46 px each)
                let ctrl_w = 3.0 * 46.0;
                let avail = ui.available_width() - ctrl_w;
                // the free middle part drags the window; a double click maximises or restores it
                let (drag_rect, drag_resp) = ui.allocate_exact_size(Vec2::new(avail.max(10.0), 34.0), Sense::click_and_drag());
                if let Some(p) = &self.project {
                    let name = std::path::Path::new(&p.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    ui.painter().text(drag_rect.center(), Align2::CENTER_CENTER, format!("{}{}", name, if p.dirty { " \u{2022}" } else { "" }), FontId::proportional(13.0), theme::text_dim());
                }
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                if drag_resp.double_clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                // the three buttons are painted by hand and send minimise, maximise or close to the window
                for (k, kind) in ["min", "max", "close"].iter().enumerate() {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(46.0, 34.0), Sense::click());
                    let hot = resp.hovered();
                    let (bg, fg) = match (*kind, hot) {
                        ("close", true) => (Color32::from_rgb(0xC4, 0x2B, 0x1C), Color32::WHITE),
                        (_, true) => (theme::hover_bg(), theme::text()),
                        _ => (Color32::TRANSPARENT, theme::text()),
                    };
                    ui.painter().rect_filled(r, 0.0, bg);
                    let c = r.center();
                    let st = Stroke::new(1.2, fg);
                    match *kind {
                        "min" => {ui.painter().line_segment([c + Vec2::new(-5.0, 0.0), c + Vec2::new(5.0, 0.0)], st);}
                        "max" => {
                            if maximized {
                                ui.painter().rect_stroke(Rect::from_center_size(c + Vec2::new(-1.0, 1.0), Vec2::splat(8.0)), 0.0, st, StrokeKind::Middle);
                                ui.painter().line_segment([c + Vec2::new(-1.0, -4.0), c + Vec2::new(5.0, -4.0)], st);
                                ui.painter().line_segment([c + Vec2::new(5.0, -4.0), c + Vec2::new(5.0, 3.0)], st);
                            } else {
                                ui.painter().rect_stroke(Rect::from_center_size(c, Vec2::splat(10.0)), 0.0, st, StrokeKind::Middle);
                            }
                        }
                        _ => {
                            ui.painter().line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(5.0, 5.0)], st);
                            ui.painter().line_segment([c + Vec2::new(-5.0, 5.0), c + Vec2::new(5.0, -5.0)], st);
                        }
                    }
                    if resp.clicked() {
                        match k {
                            0 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                            1 => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                            _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        }
                    }
                }
            });
        });
    }

    /// Saves to the file the project was opened from. The original is copied once to name.bak (when no .bak exists yet). Without a file on disk it behaves as Save as.
    fn save_in_place(&mut self) {
        let Some(p) = &mut self.project else { return };
        if matches!(&p.source, Source::Sp(k) if k.lookup.count_hi != 0) {
            self.log(Level::Err, "Saving the game's own Parallel Lines packages is not supported yet: their extra data would be lost. Compile a Lua script to build a package instead.");
            return;
        }
        p.sync_to_source();
        let path = PathBuf::from(&p.path);
        if !path.is_file() {
            self.save_as();
            return;
        }
        let bytes = match &mut p.source {
            Source::Mpc(mf) => mf.to_bytes().map_err(|e| e.to_string()),
            Source::Sp(pkg) => pkg.to_bytes().map_err(|e| e.to_string()),
        };
        // the .bak copy is made only when no .bak file exists yet
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        let msg = match bytes {
            Ok(b) => {
                if !bak.exists() {
                    let _ = std::fs::copy(&path, &bak);
                }
                match std::fs::write(&path, b) {
                    Ok(()) => {
                        p.dirty = false;
                        (Level::Ok, format!("Saved {} (original kept as {})", path.display(), bak.file_name().unwrap().to_string_lossy()))
                    }
                    Err(e) => (Level::Err, format!("Cannot write: {e}")),
                }
            }
            Err(e) => (Level::Err, e),
        };
        self.log(msg.0, msg.1);
    }

    /// The Cameras tab of the bottom panel: play and stop, the time, the display options, a timeline with one segment per shot (its blend part shaded), and the shot list with editable duration, blend time and zoom.
    fn cameras_panel(&mut self, ui: &mut egui::Ui) {
        let Some(p) = &self.project else {
            ui.label(RichText::new("Open a mission to see its cameras.").color(theme::text_dim()));
            return;
        };
        let v = p.view();
        let shots = camera::shots(v);
        let total = camera::total_time(&shots);
        if shots.is_empty() {
            ui.label(RichText::new("This mission has no camera shots (no CameraSelect nodes pointing at cameras).").color(theme::text_dim()));
        } else {
            // playback stops at the end of the sequence
            if self.cam_t >= total {
                self.cam_t = total;
                self.playing = false;
            }
            ui.horizontal(|ui| {
                // play and stop, the time readout, and the preview options
                let label = if self.playing { "Pause" } else { "Play" };
                if ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(theme::accent())).clicked() {
                    if !self.playing && self.cam_t >= total {
                        self.cam_t = 0.0;
                    }
                    self.playing = !self.playing;
                }
                if ui.button("Stop").clicked() {
                    self.playing = false;
                    self.cam_t = 0.0;
                }
                ui.label(RichText::new(format!("{:.1} / {:.1} s", self.cam_t, total)).monospace());
                ui.checkbox(&mut self.sky_on, "Mission sky in preview");
                ui.checkbox(&mut self.labels_on, "Labels");
                ui.checkbox(&mut self.roads_on, "Roads (experimental)").on_hover_text("Parallel Lines road points from AIEX. They do not line up with the streets yet: the layout is only partly understood.");
                ui.label(RichText::new("Order and timing follow the CameraSelect chain; timers may differ.").size(11.0).color(theme::text_dim()));
            });
            // timeline
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click_and_drag());
            if (resp.clicked() || resp.dragged()) && total > 0.0 {
                if let Some(m) = resp.interact_pointer_pos() {
                    self.cam_t = ((m.x - rect.left()) / rect.width() * total).clamp(0.0, total);
                    self.playing = false;
                }
            }
            // one segment per shot, placed by its start and duration; the playhead is a line in the accent colour
            let painter = ui.painter_at(rect);
            for (k, sh) in shots.iter().enumerate() {
                let x0 = rect.left() + sh.start / total.max(0.001) * rect.width();
                let x1 = rect.left() + (sh.start + sh.duration) / total.max(0.001) * rect.width();
                let r = Rect::from_min_max(Pos2::new(x0 + 1.0, rect.top()), Pos2::new((x1 - 1.0).max(x0 + 2.0), rect.bottom()));
                let base = theme::seg(k);
                painter.rect_filled(r, 4.0, base);
                if sh.blend > 0.0 && k > 0 {
                    let bw = (sh.blend / total.max(0.001) * rect.width()).min(r.width());
                    painter.rect_filled(Rect::from_min_size(r.min, Vec2::new(bw, r.height())), 4.0, theme::seg_blend());
                }
                painter.text(r.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, format!("{}  {:.1}s", k + 1, sh.duration), FontId::proportional(12.0), theme::text());
            }
            let hx = rect.left() + self.cam_t / total.max(0.001) * rect.width();
            painter.line_segment([Pos2::new(hx, rect.top() - 2.0), Pos2::new(hx, rect.bottom() + 2.0)], Stroke::new(2.0, theme::accent()));
            ui.add_space(4.0);
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (k, sh) in shots.iter().enumerate() {
                ui.horizontal(|ui| {
                    // the shot that contains the playback time is highlighted
                    let current = self.cam_t >= sh.start && self.cam_t < sh.start + sh.duration;
                    let label = format!("{}  {} \u{00B7} actor {}", k + 1, "Camera", sh.actor);
                    // a click on a shot moves the playback to its start and selects its camera object (or actor)
                    if ui.selectable_label(current, label).clicked() {
                        self.cam_t = sh.start;
                        self.playing = false;
                        self.sel = match sh.object {
                            Some(o) => Sel::Object(o),
                            None => Sel::Actor(sh.actor),
                        };
                    }
                    // the shot list queues edits: duration and blend time change the CameraSelect node, zoom changes the camera actor
                    let mut d = sh.duration;
                    ui.label(RichText::new("duration").color(theme::text_dim()));
                    if ui.add(egui::DragValue::new(&mut d).speed(0.05).range(0.0..=600.0).suffix(" s")).changed() {
                        self.pending.push(Edit::NodeFloat(sh.node, "pDuration", d));
                    }
                    let mut b = sh.blend;
                    ui.label(RichText::new("blend in").color(theme::text_dim()));
                    if ui.add(egui::DragValue::new(&mut b).speed(0.05).range(0.0..=600.0).suffix(" s")).changed() {
                        self.pending.push(Edit::NodeFloat(sh.node, "pBlendTime", b));
                    }
                    let mut z = sh.zoom;
                    ui.label(RichText::new("zoom").color(theme::text_dim()));
                    if ui.add(egui::DragValue::new(&mut z).speed(0.02).range(0.1..=20.0)).changed() {
                        self.pending.push(Edit::ActorFloat(sh.actor, "pCameraZoom", z));
                    }
                });
            }
            // animation actors and the nodes that play them
            let anims = camera::animations(v);
            if !anims.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("ANIMATIONS").size(11.0).color(theme::text_dim()));
                for (i, desc) in anims {
                    let oi = v.mission.logic.actors.definitions[i].object_id;
                    if ui.selectable_label(false, format!("Animation actor {i} \u{00B7} {desc}")).clicked() {
                        self.sel = if oi >= 0 { Sel::Object(oi as usize) } else { Sel::Actor(i) };
                    }
                }
            }
        });
    }

    /// The player start as [x, y, z]: x and z from the summary, y the height of the first character (0 when there is none). Zeros when no mission is open.
    fn start_point(&self) -> [f32; 3] {
        let Some(p) = &self.project else { return [0.0; 3] };
        let v = p.view();
        let y = v.mission.objects.objects.iter().find_map(|o| if let zartex_core::objects::MissionObject::Character { position, .. } = o { Some(position.y) } else { None }).unwrap_or(0.0);
        match &v.summary {
            Some(s) => [s.start_position.x, y, s.start_position.y],
            None => [0.0, y, 0.0],
        }
    }

    /// Models and textures of a Parallel Lines city file, one package at a time.
    fn pl_assets_tab(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.settings.pl.clone() else {
            ui.label(RichText::new("Set the Parallel Lines game folder in File > Settings.").color(theme::text_dim()));
            return;
        };
        let cities = plassets::city_files(&root);
        if cities.is_empty() {
            ui.label(RichText::new("No *_pc.d4c city file in the Cities folder.").color(theme::text_dim()));
            return;
        }
        self.pl_city = self.pl_city.min(cities.len() - 1);
        let name = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        egui::ComboBox::from_id_salt("pl_city").selected_text(name(&cities[self.pl_city])).width(ui.available_width()).show_ui(ui, |ui| {
            for (i, c) in cities.iter().enumerate() {
                if ui.selectable_label(self.pl_city == i, name(c)).clicked() {
                    self.pl_city = i;
                    self.pl_pkg = 0;
                }
            }
        });
        // the chosen city file is opened; an error is shown in red instead of the list
        self.pl.open(&cities[self.pl_city]);
        if let Some(e) = &self.pl.error {
            ui.label(RichText::new(e.clone()).size(12.0).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
            return;
        }
        let labels: Vec<String> = self.pl.index.iter().map(|r| if r.label == "GEGR" { "Global (GEGR)".to_string() } else { r.label.replace("GESR", "Super region ") }).collect();
        if labels.is_empty() {
            return;
        }
        self.pl_pkg = self.pl_pkg.min(labels.len() - 1);
        egui::ComboBox::from_id_salt("pl_pkg").selected_text(labels[self.pl_pkg].clone()).width(ui.available_width()).show_ui(ui, |ui| {
            for (i, l) in labels.iter().enumerate() {
                if ui.selectable_label(self.pl_pkg == i, l).clicked() {
                    self.pl_pkg = i;
                }
            }
        });
        let pkg = match self.pl.package(self.pl_pkg) {
            Ok(p) => p,
            Err(e) => {
                ui.label(RichText::new(e).size(12.0).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
                return;
            }
        };
        let mut pick: Option<Asset> = None;
        let cur = self.asset.clone();
        let pi = self.pl_pkg;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::CollapsingHeader::new(format!("Textures ({})", pkg.textures.len())).default_open(true).show(ui, |ui| {
                for (i, t) in pkg.textures.iter().enumerate() {
                    let l = format!("{i:>3}  {}x{}  {:?}", t.width, t.height, t.format);
                    if ui.selectable_label(cur == Some(Asset::PlTex(pi, i)), l).clicked() {
                        pick = Some(Asset::PlTex(pi, i));
                    }
                }
            });
            egui::CollapsingHeader::new(format!("Models ({})", pkg.models.len())).show(ui, |ui| {
                for (i, m) in pkg.models.iter().enumerate() {
                    let tris: usize = m.submodels.iter().map(|s| s.indices.len() / 3).sum();
                    let l = format!("{i:>3}  {} parts \u{00B7} {} triangles", m.submodels.len(), tris);
                    if ui.selectable_label(cur == Some(Asset::PlModel(pi, i)), l).clicked() {
                        pick = Some(Asset::PlModel(pi, i));
                    }
                }
            });
        });
        if let Some(a) = pick {
            self.asset = Some(a);
        }
    }

    /// The Assets tab of Driv3r: skies, moods, the vehicles of the current city and the characters of the open mission, filtered by the search box. Parallel Lines uses pl_assets_tab.
    fn assets_tab(&mut self, ui: &mut egui::Ui) {
        if self.game == Game::ParallelLines {
            self.pl_assets_tab(ui);
            return;
        }
        let Some(root) = self.settings.driv3r.clone() else {
            ui.label(RichText::new("Set the Driv3r game folder in File > Settings.").color(theme::text_dim()));
            return;
        };
        let (city, stem) = match &self.project {
            Some(p) => (p.view().summary.as_ref().map(|s| s.city_type).unwrap_or(0), std::path::Path::new(&p.path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()),
            None => (0, String::new()),
        };
        // the vehicle list (by city) and the skin list (by mission) are read again only when those change
        if self.veh_cache.as_ref().map(|c| c.0) != Some(city) {
            self.veh_cache = Some((city, assets::vehicle_ids(&root, city)));
        }
        if self.skin_cache.as_ref().map(|c| c.0.as_str()) != Some(stem.as_str()) {
            self.skin_cache = Some((stem.clone(), assets::mission_skins(&root, &stem)));
        }
        ui.add(egui::TextEdit::singleline(&mut self.asset_filter).hint_text("Search assets").desired_width(f32::INFINITY));
        let f = self.asset_filter.to_lowercase();
        let hit = |t: &str| f.is_empty() || t.to_lowercase().contains(&f);
        let mut pick: Option<Asset> = None;
        let cur = self.asset.clone();
        let skies = self.skies.clone();
        let moods = self.mood_list.clone();
        let vehicles = self.veh_cache.as_ref().map(|c| c.1.clone()).unwrap_or_default();
        let skins = self.skin_cache.as_ref().map(|c| c.1.clone()).unwrap_or_default();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::CollapsingHeader::new(format!("Skies ({})", skies.len())).default_open(true).show(ui, |ui| {
                for n in skies.iter().filter(|n| hit(n)) {
                    if ui.selectable_label(cur == Some(Asset::Sky(n.clone())), n.trim_end_matches(".d3s")).clicked() {
                        pick = Some(Asset::Sky(n.clone()));
                    }
                }
            });
            egui::CollapsingHeader::new(format!("Moods ({})", moods.len())).show(ui, |ui| {
                for (id, label) in moods.iter().filter(|m| hit(&m.1)) {
                    if ui.selectable_label(cur == Some(Asset::Mood(*id)), label).clicked() {
                        pick = Some(Asset::Mood(*id));
                    }
                }
            });
            egui::CollapsingHeader::new(format!("Vehicles of this city ({})", vehicles.len())).show(ui, |ui| {
                for id in vehicles.iter().filter(|i| hit(&format!("vehicle {i}"))) {
                    if ui.selectable_label(cur == Some(Asset::Vehicle(*id)), format!("Vehicle {id}")).clicked() {
                        pick = Some(Asset::Vehicle(*id));
                    }
                }
            });
            egui::CollapsingHeader::new(format!("Characters of this mission ({})", skins.len())).show(ui, |ui| {
                for h in skins.iter().filter(|h| hit(&assets::skin_name(**h))) {
                    if ui.selectable_label(cur == Some(Asset::Skin(*h)), assets::skin_name(*h)).clicked() {
                        pick = Some(Asset::Skin(*h));
                    }
                }
                if skins.is_empty() {
                    ui.label(RichText::new("Open a Driv3r mission to list its characters.").size(11.5).color(theme::text_dim()));
                }
            });
        });
        if pick.is_some() {
            self.asset = pick;
        }
    }

    /// Preview of a Parallel Lines texture (the image and its size) or model (a 3D box).
    fn pl_preview(&mut self, ui: &mut egui::Ui, asset: &Asset) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match asset {
            Asset::PlTex(p, i) => {
                let (p, i) = (*p, *i);
                ui.label(RichText::new(format!("Texture {i} \u{00B7} package {p}")).strong());
                let key = (self.pl_city, p, i);
                if self.pl_tex.as_ref().map(|t| t.0) != Some(key) {
                    self.pl_tex = match self.pl.texture_image(p, i) {
                        Ok(img) => Some((key, ui.ctx().load_texture("pl_tex", img, egui::TextureOptions::LINEAR))),
                        Err(e) => {
                            ui.label(e);
                            None
                        }
                    };
                }
                if let Some((_, t)) = &self.pl_tex {
                    let w = ui.available_width();
                    let sz = t.size_vec2();
                    let h = w * sz.y / sz.x.max(1.0);
                    let (r, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
                    ui.painter().rect_filled(r, 4.0, theme::canvas_bg());
                    ui.painter().image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    kv(ui, "Size", format!("{} x {}", sz.x, sz.y));
                }
            }
            Asset::PlModel(p, i) => {
                let (p, i) = (*p, *i);
                ui.label(RichText::new(format!("Model {i} \u{00B7} package {p}")).strong());
                let city = self.pl_city;
                let pl = &mut self.pl;
                self.model_box.custom(ui, &format!("pl:{city}:{p}:{i}"), &format!("Model {i}"), || pl.model_scene(p, i));
            }
            _ => {}
        });
    }

    /// Preview of the Driv3r asset picked in the Assets tab, with the buttons that create it at the player start in the open mission (through run_snippet).
    fn asset_preview(&mut self, ui: &mut egui::Ui) {
        if let Some(a @ (Asset::PlTex(..) | Asset::PlModel(..))) = self.asset.clone() {
            self.pl_preview(ui, &a);
            return;
        }
        let Some(root) = self.settings.driv3r.clone() else { return };
        let Some(asset) = self.asset.clone() else { return };
        let (city, stem) = match &self.project {
            Some(p) => (p.view().summary.as_ref().map(|s| s.city_type).unwrap_or(0), std::path::Path::new(&p.path).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()),
            None => (0, String::new()),
        };
        let can_add = self.project.as_ref().map(|p| p.view().kind == GameKind::Driv3r).unwrap_or(false);
        let [x, y, z] = self.start_point();
        let mut run: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match &asset {
            Asset::Sky(name) => {
                ui.label(RichText::new(format!("Sky \u{00B7} {}", name.trim_end_matches(".d3s"))).strong());
                self.sky_box.show(ui, &root, name);
                let used: Vec<String> = self.mood_list.iter().filter(|m| mood::Mood::load(&root.join("Moods").join(format!("mood{:02}.txt", m.0))).and_then(|mm| mm.get("SKYDOME_NAME").map(|s| s.to_lowercase().contains(&name.to_lowercase().trim_end_matches(".d3s").to_string()))).unwrap_or(false)).map(|m| m.1.clone()).collect();
                if !used.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("USED BY MOODS").size(11.0).color(theme::text_dim()));
                    for u in used {
                        ui.label(u);
                    }
                }
            }
            Asset::Mood(id) => {
                ui.label(RichText::new(format!("Mood {id:02}")).strong());
                match mood::Mood::load(&root.join("Moods").join(format!("mood{id:02}.txt"))) {
                    Some(m) => {
                        for k in ["TIME_OF_DAY", "WEATHER", "RAIN", "MUSIC_NAME", "SKYDOME_NAME", "SUN_ELEVATION", "FOG_NEARDISTANCE", "FOG_FARDISTANCE", "STREET_LIGHTS", "HEAD_LIGHTS"] {
                            if let Some(val) = m.get(k) {
                                kv(ui, &k.to_lowercase().replace('_', " "), val.to_string());
                            }
                        }
                        if let Some(sky) = m.get("SKYDOME_NAME") {
                            ui.add_space(6.0);
                            self.sky_box.show(ui, &root, sky);
                        }
                    }
                    None => {
                        ui.label(RichText::new("Cannot read this mood file.").color(theme::text_dim()));
                    }
                }
                ui.add_space(8.0);
                if self.project.as_ref().map(|p| p.view().summary.is_some()).unwrap_or(false) && ui.button("Use this mood in the open mission").clicked() {
                    self.pending.push(Edit::Summary(SummaryEdit::MoodId(*id)));
                }
            }
            Asset::Vehicle(id) => {
                ui.label(RichText::new(format!("Vehicle {id}")).strong());
                self.model_box.vehicle(ui, &root, city, *id);
                ui.add_space(8.0);
                if can_add && ui.button("Create this vehicle at the player start").clicked() {
                    run = Some(format!("MISSION.CreateVehicle({x}, {y}, {z}, 0, {id}, 0)"));
                }
            }
            Asset::PlTex(..) | Asset::PlModel(..) => {}
            Asset::Skin(h) => {
                ui.label(RichText::new(format!("Character \u{00B7} {}", assets::skin_name(*h))).strong());
                self.model_box.character(ui, &root, &stem, *h);
                ui.add_space(8.0);
                if can_add && ui.button("Create this character at the player start").clicked() {
                    run = Some(format!("MISSION.CreateCharacter({x}, {y}, {z}, 0, {h}, 1, \"{}\", -1, -1, 1.0, 0.0, 0, 1.0)", assets::skin_name(*h)));
                }
            }
        });
        // the Lua of a button is run after the preview has been drawn
        if let Some(src) = run {
            self.run_snippet(&src);
        }
    }

    /// The Missions tab: Driv3r missions grouped by city and mode (from missions::TABLE), with the other mission files under Other; Parallel Lines lists its package files.
    fn missions_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let k = if self.game == Game::Driv3r { 0 } else { 1 };
        if !self.scans[k].ok {
            ui.label(RichText::new(if self.settings.path(self.game).is_none() { "Game folder not set." } else { "Nothing found in the game folder." }).color(theme::text_dim()));
            if ui.button("Settings...").clicked() {
                self.show_settings = true;
            }
            return;
        }
        let cur = self.project.as_ref().map(|p| p.path.to_lowercase());
        let mut open: Option<PathBuf> = None;
        let files = self.scans[k].files.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.game == Game::Driv3r {
                // the file of a catalogue id is the one whose name is missionNN
                let find = |id: u32| files.iter().find(|f| f.file_stem().map(|s| s.to_string_lossy().eq_ignore_ascii_case(&missions::stem(id))).unwrap_or(false));
                let mut used: Vec<PathBuf> = Vec::new();
                for city in missions::City::ALL {
                    egui::CollapsingHeader::new(RichText::new(city.name()).strong()).default_open(city == missions::City::Miami).show(ui, |ui| {
                        let mut last_mode = "";
                        for d in missions::TABLE.iter().filter(|d| d.city == city) {
                            let present: Vec<(u32, &PathBuf)> = d.ids.iter().filter_map(|&id| find(id).map(|f| (id, f))).collect();
                            if present.is_empty() {
                                continue;
                            }
                            if d.mode != last_mode {
                                ui.add_space(4.0);
                                ui.label(RichText::new(d.mode.to_uppercase()).size(10.5).color(theme::text_dim()));
                                last_mode = d.mode;
                            }
                            for (n, (id, f)) in present.iter().enumerate() {
                                used.push((*f).clone());
                                let label = if n == 0 { format!("{}   ({})", d.name, missions::stem(*id)) } else { format!("      part {} ({})", n + 1, missions::stem(*id)) };
                                let on = cur.as_deref() == Some(&f.to_string_lossy().to_lowercase());
                                if ui.selectable_label(on, label).clicked() {
                                    open = Some((*f).clone());
                                }
                            }
                        }
                    });
                }
                // mission files that are not in the catalogue
                let others: Vec<&PathBuf> = files.iter().filter(|f| !used.contains(f)).collect();
                if !others.is_empty() {
                    egui::CollapsingHeader::new(RichText::new("Other").strong()).show(ui, |ui| {
                        for f in others {
                            let n = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                            if ui.selectable_label(cur.as_deref() == Some(&f.to_string_lossy().to_lowercase()), n).clicked() {
                                open = Some(f.clone());
                            }
                        }
                    });
                }
            } else {
                for f in &files {
                    let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    let pretty = match stem.to_lowercase().as_str() {
                        "nyc_now_missions" => "New York (now)".to_string(),
                        "nyc_then_missions" => "New York (then)".to_string(),
                        _ => stem,
                    };
                    let on = cur.as_deref() == Some(&f.to_string_lossy().to_lowercase());
                    if ui.selectable_label(on, pretty).clicked() {
                        open = Some(f.clone());
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Pick the mission inside the package with the list in the toolbar.").size(11.5).color(theme::text_dim()));
            }
        });
        if let Some(f) = open {
            self.open_path(f);
        }
    }

    /// The Add to mission window: pick an actor or logic node creator from creators::CREATORS, edit its arguments, and create it. A node can also be wired from the selected node.
    fn add_window(&mut self, ctx: &egui::Context) {
        if !self.show_add {
            return;
        }
        let mut open = true;
        let mut create: Option<String> = None;
        // the wire option refers to the node selected in the inspector
        let sel_node = if let Sel::Node(n) = self.sel { Some(n) } else { None };
        let actor_list: Vec<(usize, String)> = self.project.as_ref().map(|p| p.view().mission.logic.actors.definitions.iter().enumerate().map(|(i, a)| (i, actor_type_name(a.type_id))).collect()).unwrap_or_default();
        egui::Window::new("Add to mission").open(&mut open).collapsible(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).default_size([620.0, 460.0]).show(ctx, |ui| {
            ui.label(RichText::new("Pick what to create. It is appended to the open Driv3r mission by running the matching MISSION function; edit the arguments first if you like.").size(12.0).color(theme::text_dim()));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    ui.add(egui::TextEdit::singleline(&mut self.add_filter).hint_text("Search").desired_width(f32::INFINITY));
                    let f = self.add_filter.to_lowercase();
                    egui::ScrollArea::vertical().id_salt("addlist").max_height(340.0).show(ui, |ui| {
                        for (title, nodes) in [("ACTORS", false), ("LOGIC NODES", true)] {
                            ui.label(RichText::new(title).size(11.0).color(theme::text_dim()));
                            for (ci, c) in creators::CREATORS.iter().enumerate().filter(|(_, c)| c.makes_node == nodes && (f.is_empty() || c.name.to_lowercase().contains(&f))) {
                                if ui.selectable_label(self.add_sel == Some(ci), c.name).clicked() {
                                    self.add_sel = Some(ci);
                                    self.add_args = creator_tools::default_args(c, &actor_list);
                                }
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
                ui.separator();
                ui.vertical(|ui| {
                    match self.add_sel.and_then(|i| creators::CREATORS.get(i)) {
                        None => {
                            ui.label(RichText::new("Select an entry on the left.").color(theme::text_dim()));
                        }
                        Some(c) => {
                            ui.label(RichText::new(format!("MISSION.{}", c.name)).strong());
                            ui.label(RichText::new(if c.makes_node { "Creates a logic node" } else { "Creates an actor (and its map object)" }).size(11.5).color(theme::text_dim()));
                            ui.add_space(6.0);
                            ui.label(RichText::new("Arguments, in order:").size(11.5).color(theme::text_dim()));
                            for p in c.params {
                                ui.label(RichText::new(format!("  {} {}", p.ty, p.name)).monospace().size(11.5));
                            }
                            ui.add_space(6.0);
                            ui.add(egui::TextEdit::multiline(&mut self.add_args).code_editor().desired_rows(3).desired_width(f32::INFINITY));
                            ui.label(RichText::new("Actor and node arguments are numbers: MISSION.GetActor(3), MISSION.GetNode(9).").size(11.0).color(theme::text_dim()));
                            ui.add_space(6.0);
                            if c.makes_node {
                                ui.add_enabled(sel_node.is_some(), egui::Checkbox::new(&mut self.add_wire, match sel_node { Some(n) => format!("Wire from the selected node ({n}) to the new one"), None => "Wire from the selected node (select a node first)".to_string() }));
                            }
                            ui.add_space(8.0);
                            if ui.add(egui::Button::new(RichText::new("Create").color(Color32::WHITE)).fill(theme::accent())).clicked() {
                                // the Lua is the chosen creator's call; a wire from the selected node is added when ticked
                                let mut src = format!("local created = MISSION.{}({})\n", c.name, self.add_args.trim());
                                if let (true, true, Some(n)) = (c.makes_node, self.add_wire, sel_node) {
                                    src += &format!("MISSION.GetNode({n}).wireCollection.Add(created)\n");
                                }
                                create = Some(src);
                            }
                        }
                    }
                });
            });
        });
        if let Some(src) = create {
            self.run_snippet(&src);
        }
        if !open {
            self.show_add = false;
        }
    }

    /// The About window: a header, then three tabs (build and computer, credits, licence).
    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        // the system information is read once, in a background thread, and shown when it is ready
        if !self.sys_started {
            self.sys_started = true;
            let slot = self.sys_info.clone();
            std::thread::spawn(move || {
                let info = about::gather();
                if let Ok(mut g) = slot.lock() {
                    *g = Some(info);
                }
            });
        }
        let sys = self.sys_info.lock().ok().and_then(|g| g.clone());
        let icon = self.icon_id(ctx);
        let mut open = true;
        let link = |ui: &mut egui::Ui, text: &str, url: &str| {
            if ui.link(text).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        };
        let tab = &mut self.about_tab;
        egui::Window::new("About").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).fixed_size(Vec2::new(560.0, 0.0)).show(ctx, |ui| {
            ui.set_width(540.0);
            // header: icon, name, version, one line
            ui.horizontal(|ui| {
                match icon {
                    Some(id) => {
                        ui.add(egui::Image::new((id, Vec2::splat(64.0))));
                    }
                    None => {
                        ui.label(RichText::new("\u{25A0}").color(theme::accent()).size(40.0));
                    }
                }
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new(about::NAME).strong().size(22.0));
                    ui.label(RichText::new(format!("Version {}", about::VERSION)).size(13.5));
                    ui.label(RichText::new("Mission editor for Driv3r and Driver: Parallel Lines").color(theme::text_dim()));
                });
            });
            ui.add_space(6.0);
            ui.label(RichText::new("A rewrite of Zartex 2 in Rust. It reads, edits and builds missions, runs the Lua mission API, and shows the city in 3D with the mission's actors, objects and cutscene cameras.").size(12.5).color(theme::text_dim()));
            ui.add_space(6.0);
            // tabs
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (k, name) in ["Build and computer", "Credits", "License"].iter().enumerate() {
                    if tab_button(ui, *tab == k, name).clicked() {
                        *tab = k;
                    }
                }
            });
            ui.separator();
            ui.add_space(4.0);
            let label = |ui: &mut egui::Ui, t: &str| {
                ui.label(RichText::new(t).color(theme::text_dim()));
            };
            match *tab {
                0 => {
                    // tab 0: build data, the renderer, and this computer
                    ui.label(RichText::new("BUILD").size(11.0).color(theme::text_dim()));
                    egui::Grid::new("about_build").num_columns(2).spacing([18.0, 6.0]).striped(true).min_col_width(110.0).show(ui, |ui| {
                        label(ui, "Version");
                        ui.label(about::VERSION);
                        ui.end_row();
                        label(ui, "Build");
                        ui.label(about::build_line());
                        ui.end_row();
                        label(ui, "3D renderer");
                        ui.label(match (gpu::enabled(), gpu::support()) {
                            (true, gpu::Support::Available(n)) => format!("Vulkan \u{00B7} {n}"),
                            (_, gpu::Support::Available(_)) => "CPU (Vulkan available, switched off)".to_string(),
                            _ => "CPU (Vulkan not available)".to_string(),
                        });
                        ui.end_row();
                    });
                    ui.add_space(10.0);
                    ui.label(RichText::new("THIS COMPUTER").size(11.0).color(theme::text_dim()));
                    match &sys {
                        Some(sy) => {
                            egui::Grid::new("about_sys").num_columns(2).spacing([18.0, 6.0]).striped(true).min_col_width(110.0).show(ui, |ui| {
                                label(ui, "System");
                                ui.label(sy.os.clone());
                                ui.end_row();
                                label(ui, "Processor");
                                ui.label(sy.cpu.clone());
                                ui.end_row();
                                label(ui, "Memory");
                                ui.label(sy.ram.clone());
                                ui.end_row();
                                if sy.gpus.is_empty() {
                                    label(ui, "Graphics");
                                    ui.label(match gpu::support() { gpu::Support::Available(n) => n, _ => "unknown".to_string() });
                                    ui.end_row();
                                }
                                for g in &sy.gpus {
                                    label(ui, "Graphics");
                                    ui.label(g.clone());
                                    ui.end_row();
                                }
                            });
                        }
                        None => {
                            ui.label(RichText::new("Reading the system information...").color(theme::text_dim()));
                            ui.ctx().request_repaint();
                        }
                    }
                }
                1 => {
                    // tab 1: credits, one entry per project (name, people, link)
                    let entries: [(&str, &str, &str, &str); 3] = [
                        ("ZARTEX'27", "LMonteiroSt", "github.com/lmonteirotech", about::AUTHOR_URL),
                        ("ZARTEX 2: LUA MISSION API AND PARALLEL LINES SUPPORT", "BuilderDemo7", "github.com/BuilderDemo7/Zartex2", "https://github.com/BuilderDemo7/Zartex2"),
                        ("ZARTEX, DSCRIPT AND THE DRIVER TOOLS", "Fireboyd78/Mark Ludwig/CarLuver69", "github.com/Fireboyd78/driver-tools", "https://github.com/Fireboyd78/driver-tools"),
                    ];
                    for (k, (what, who, text, url)) in entries.iter().enumerate() {
                        if k > 0 {
                            ui.add_space(4.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }
                        ui.label(RichText::new(*what).size(11.0).color(theme::text_dim()));
                        ui.label(RichText::new(*who).strong().size(14.0));
                        link(ui, text, url);
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Contributors of the Zartex 2 repository: BuilderDemo7 (the only one listed there).").size(12.0).color(theme::text_dim()));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Built with egui, wgpu, mlua (Lua 5.2), glam and sysinfo. File readers shared with the game engine Rust Rework research.").size(12.0).color(theme::text_dim()));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Driv3r and Driver: Parallel Lines belong to their owners.").size(12.0).color(theme::text_dim()));
                }
                _ => {
                    // tab 2: the licence summary, and a link to the full text
                    ui.label(RichText::new("GNU General Public License, version 3 or later").strong());
                    ui.add_space(6.0);
                    ui.label("Zartex'27 is free software. You can redistribute it and modify it under the terms of the GNU General Public License, version 3 or (at your option) any later version.");
                    ui.add_space(6.0);
                    ui.label("It comes with no warranty.");
                    ui.add_space(8.0);
                    link(ui, "gnu.org/licenses/gpl-3.0", about::LICENSE_URL);
                }
            }
            ui.add_space(4.0);
        });
        if !open {
            self.show_about = false;
        }
    }

    /// The Settings window: game folders with their scan messages, the theme and accent colour, the graphics acceleration switch, and a recheck button. Changes are saved at once; a changed folder also triggers a rescan.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        // set while drawing: recheck (rescan the folders) and save_only (write the settings file)
        let mut recheck = false;
        let mut save_only = false;
        egui::Window::new("Settings").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, Vec2::ZERO).default_width(560.0).show(ctx, |ui| {
            ui.label(RichText::new("GAME INSTALLATIONS").size(11.0).color(theme::text_dim()));
            ui.add_space(4.0);
            // one card per game: the folder field (typed or chosen with Browse), and its scan message
            for (k, g, title) in [(0usize, Game::Driv3r, "Driv3r"), (1, Game::ParallelLines, "Driver: Parallel Lines")] {
                theme::card().show(ui, |ui| {
                    ui.label(RichText::new(title).strong());
                    let mut text = self.settings.path(g).map(|p| p.display().to_string()).unwrap_or_default();
                    ui.horizontal(|ui| {
                        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(380.0).hint_text("Root folder of the installation"));
                        if r.changed() {
                            let v = if text.trim().is_empty() { None } else { Some(PathBuf::from(text.trim())) };
                            match g {
                                Game::Driv3r => self.settings.driv3r = v,
                                Game::ParallelLines => self.settings.pl = v,
                            }
                            recheck = true;
                        }
                        if ui.button("Browse...").clicked() {
                            self.pick_path(g);
                        }
                    });
                    let sc = &self.scans[k];
                    if self.settings.path(g).is_none() {
                        ui.label(RichText::new("Not set").color(theme::text_dim()));
                    } else if sc.ok {
                        ui.label(RichText::new(format!("\u{2713} {}", sc.message)).color(Color32::from_rgb(0x10, 0x7C, 0x10)));
                    } else {
                        ui.label(RichText::new(format!("\u{2717} {}", sc.message)).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
                    }
                });
                ui.add_space(6.0);
            }
            ui.add_space(4.0);
            // theme and accent colour; the Windows accent option is only offered on Windows
            ui.label(RichText::new("APPEARANCE").size(11.0).color(theme::text_dim()));
            theme::card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Theme");
                    for (a, n) in [(theme::Appearance::System, "Follow the system"), (theme::Appearance::Light, "Light"), (theme::Appearance::Dark, "Dark")] {
                        if ui.radio_value(&mut self.settings.appearance, a, n).changed() {
                            save_only = true;
                        }
                    }
                });
                ui.add_space(4.0);
                let mut use_win = self.settings.accent_windows && cfg!(windows);
                ui.horizontal(|ui| {
                    ui.label("Accent color");
                    let mut c = self.settings.accent;
                    ui.add_enabled_ui(!use_win, |ui| {
                        if ui.color_edit_button_srgb(&mut c).changed() {
                            self.settings.accent = c;
                            save_only = true;
                        }
                    });
                    if use_win {
                        if let Some(w) = self.win_accent {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(26.0, 16.0), Sense::hover());
                            ui.painter().rect_filled(r, 4.0, Color32::from_rgb(w[0], w[1], w[2]));
                        }
                    }
                });
                if cfg!(windows) {
                    if ui.checkbox(&mut use_win, "Use the Windows accent color").changed() {
                        self.settings.accent_windows = use_win;
                        if use_win {
                            self.win_accent = theme::windows_accent();
                        }
                        save_only = true;
                    }
                } else {
                    ui.label(RichText::new("Using the system accent color is available on Windows only.").size(11.5).color(theme::text_dim()));
                }
            });
            ui.add_space(6.0);
            // the switch is enabled only when the graphics card supports acceleration (gpu::support); otherwise it is shown locked off
            ui.label(RichText::new("PERFORMANCE").size(11.0).color(theme::text_dim()));
            theme::card().show(ui, |ui| {
                match gpu::support() {
                    gpu::Support::Available(name) => {
                        let mut on = !self.settings.gpu_off;
                        if ui.checkbox(&mut on, "Graphics acceleration (Vulkan)").changed() {
                            self.settings.gpu_off = !on;
                            gpu::apply_preference(self.settings.gpu_off);
                            recheck = true;
                        }
                        ui.label(RichText::new(format!("\u{2713} {name}")).color(Color32::from_rgb(0x10, 0x7C, 0x10)));
                        ui.label(RichText::new(if on { "On: the map, the camera preview and the model boxes are drawn by the graphics card." } else { "Off: they are drawn by the CPU." }).size(11.5).color(theme::text_dim()));
                    }
                    gpu::Support::Unavailable(why) => {
                        ui.add_enabled(false, egui::Checkbox::new(&mut false, "Graphics acceleration (Vulkan)"));
                        ui.label(RichText::new(format!("\u{2717} Not available on this machine: {why}")).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
                        ui.label(RichText::new("Locked off: the CPU renderer is used. It enables itself on a machine with a Vulkan driver.").size(11.5).color(theme::text_dim()));
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Recheck contents").clicked() {
                    recheck = true;
                }
                ui.label(RichText::new("Missions and cities are listed from these folders.").color(theme::text_dim()));
            });
        });
        // changes are saved at once; a changed folder also triggers a rescan
        if save_only {
            self.settings.save();
        }
        if recheck {
            self.settings.save();
            self.rescan();
        }
        if !open {
            self.show_settings = false;
        }
    }

    /// The logic graph: nodes as cards, wires as curves coloured by wire type. Drag pans, the wheel zooms, a click on a node selects it, a click on empty space clears the selection.
    fn graph(&mut self, ui: &mut egui::Ui) {
        // node positions come from get_layout; the view is fitted when fit_pending is set
        let pos = self.get_layout();
        let Some(p) = &self.project else {
            let r = ui.available_rect_before_wrap();
            ui.painter().rect_filled(r, CornerRadius::same(8), theme::canvas_bg());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "Open a mission (.mpc / .sp) or compile a Lua script", FontId::proportional(15.0), theme::text_dim());
            return;
        };
        let v = p.view();
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, CornerRadius::same(8), theme::canvas_bg());
        painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, theme::card_stroke()), StrokeKind::Inside);
        let painter = painter.with_clip_rect(rect);

        // fit: zoom so that the whole graph fits, between 62 % and 100 %, and reset the pan
        if self.fit_pending && !pos.is_empty() {
            let (maxx, maxy) = pos.iter().fold((0.0f32, 0.0f32), |a, p| (a.0.max(p.0), a.1.max(p.1)));
            let w = maxx + 180.0;
            let h = maxy + 60.0;
            self.zoom = ((rect.width() - 32.0) / w).min((rect.height() - 32.0) / h).clamp(0.62, 1.0);
            self.pan = Vec2::new(16.0, 16.0);
            self.fit_pending = false;
        }

        // dragging pans; the wheel zooms around the mouse position
        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let old = self.zoom;
                self.zoom = (self.zoom * (1.0 + scroll * 0.0015)).clamp(0.25, 2.0);
                if let Some(m) = resp.hover_pos() {
                    let world = (m.to_vec2() - rect.min.to_vec2() - self.pan) / old;
                    self.pan = m.to_vec2() - rect.min.to_vec2() - world * self.zoom;
                }
            }
        }

        // background dots, spaced according to the zoom
        let step = 22.0 * self.zoom.max(0.6);
        let origin = rect.min + self.pan;
        let mut x = rect.left() + (origin.x - rect.left()).rem_euclid(step);
        while x < rect.right() {
            let mut y = rect.top() + (origin.y - rect.top()).rem_euclid(step);
            while y < rect.bottom() {
                painter.circle_filled(Pos2::new(x, y), 0.9, theme::grid());
                y += step;
            }
            x += step;
        }

        let z = self.zoom;
        let nw = 172.0 * z;
        let nh = 54.0 * z;
        let node_rect = |i: usize| -> Rect {
            let (px, py) = pos[i];
            Rect::from_min_size(origin + Vec2::new(px, py) * z, Vec2::new(nw, nh))
        };

        // a wire is a cubic curve from the right side of one node to the left side of the other
        for e in edges(v) {
            let a = node_rect(e.from);
            let b = node_rect(e.to);
            let (p0, p3) = (Pos2::new(a.right(), a.center().y), Pos2::new(b.left(), b.center().y));
            let dx = ((p3.x - p0.x).abs() * 0.5).max(30.0 * z);
            // the colour depends on the wire type: green, red, grey, otherwise the accent
            let c = match e.wire_type {
                1 | 5 | 11 | 13 => Color32::from_rgb(0x10, 0x7C, 0x10),
                3 | 4 => Color32::from_rgb(0xC4, 0x2B, 0x1C),
                2 | 6 | 12 | 14 => Color32::from_rgb(0x8A, 0x8A, 0x8A),
                _ => theme::accent(),
            };
            let active = matches!(self.sel, Sel::Node(s) if s == e.from || s == e.to);
            let stroke = Stroke::new((if active { 2.4 } else { 1.4 }) * z.max(0.7), if active { c } else { c.gamma_multiply(0.75) });
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke([p0, p0 + Vec2::new(dx, 0.0), p3 - Vec2::new(dx, 0.0), p3], false, Color32::TRANSPARENT, stroke));
        }

        let mut clicked: Option<usize> = None;
        for i in 0..pos.len() {
            let r = node_rect(i);
            if !r.intersects(rect) {
                continue;
            }
            let n = &v.mission.logic.nodes.definitions[i];
            let label = node_label(v, i);
            let tname = node_type_name(v, n.type_id);
            let head = theme::node_color(&format!("{label} {tname}"), n.type_id);
            let sel = self.sel == Sel::Node(i);
            // a node card: shadow, body, a header in the category colour with the label, then the type and the property count
            painter.rect_filled(r.translate(Vec2::new(0.0, 1.5)), CornerRadius::same(8), theme::overlay(14));
            painter.rect_filled(r, CornerRadius::same(8), theme::node_body());
            let hr = Rect::from_min_size(r.min, Vec2::new(r.width(), 20.0 * z));
            painter.rect_filled(hr, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, head);
            painter.rect_stroke(r, CornerRadius::same(8), if sel { Stroke::new(2.0, theme::accent()) } else { Stroke::new(1.0, theme::node_border()) }, StrokeKind::Inside);
            if z >= 0.5 {
                painter.with_clip_rect(hr).text(hr.left_center() + Vec2::new(8.0 * z, 0.0), Align2::LEFT_CENTER, &label, FontId::proportional((12.0 * z).max(9.0)), Color32::WHITE);
                painter.with_clip_rect(r).text(r.left_top() + Vec2::new(8.0 * z, 28.0 * z), Align2::LEFT_TOP, format!("{tname}  \u{00B7}  {} props", n.properties.len().saturating_sub(1)), FontId::proportional((11.0 * z).max(8.5)), theme::text_dim());
            }
            painter.circle_filled(Pos2::new(r.left(), r.center().y), 3.5 * z.max(0.7), theme::node_body());
            painter.circle_stroke(Pos2::new(r.left(), r.center().y), 3.5 * z.max(0.7), Stroke::new(1.2, head));
            painter.circle_filled(Pos2::new(r.right(), r.center().y), 3.5 * z.max(0.7), head);
            if resp.clicked() && resp.interact_pointer_pos().map(|p| r.contains(p)).unwrap_or(false) {
                clicked = Some(i);
            }
        }
        // a click on a node selects it; a click on empty space clears the selection
        if let Some(i) = clicked {
            self.sel = Sel::Node(i);
        } else if resp.clicked() {
            self.sel = Sel::None;
        }

        painter.text(rect.right_bottom() - Vec2::new(10.0, 8.0), Align2::RIGHT_BOTTOM, format!("{:.0}%", self.zoom * 100.0), FontId::proportional(11.5), theme::text_dim());
    }
}

// ---------------------------------------------------------------- helpers for the title bar and the panels
/// A menu button of the title bar, with a coloured underline when it is open or hovered.
fn menu_item(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    let r = ui.menu_button(title, add);
    let open = r.inner.is_some();
    if open || r.response.hovered() {
        let rc = r.response.rect;
        let y = rc.bottom() - 1.5;
        let col = if open { theme::accent() } else { Color32::from_rgb(0x9A, 0xA7, 0xB5) };
        ui.painter().line_segment([Pos2::new(rc.left() + 8.0, y), Pos2::new(rc.right() - 8.0, y)], Stroke::new(2.5, col));
    }
}

/// Flat tab with an accent bar under the selected one and a grey bar under the hovered one.
fn tab_button(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let color = if selected { theme::text() } else { theme::text_dim() };
    let galley = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.5), color);
    let size = galley.size() + Vec2::new(20.0, 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 2.0)), 4.0, theme::overlay(10));
    }
    ui.painter().galley(rect.min + Vec2::new(10.0, 6.0), galley, color);
    if selected || resp.hovered() {
        let y = rect.bottom() - 1.5;
        let col = if selected { theme::accent() } else { Color32::from_rgb(0xB4, 0xBE, 0xC9) };
        ui.painter().line_segment([Pos2::new(rect.left() + 8.0, y), Pos2::new(rect.right() - 8.0, y)], Stroke::new(2.5, col));
    }
    resp
}

/// Makes the widgets of the current region flat (no border, transparent background), for the title bar and the toolbar.
fn flat_buttons(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.bg_stroke = Stroke::NONE;
    }
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.hovered.weak_bg_fill = theme::overlay(14);
    v.widgets.hovered.bg_fill = Color32::from_black_alpha(14);
    v.widgets.open.weak_bg_fill = Color32::TRANSPARENT;
}

/// A key and a value on one row, with a separator line below. The value is shown in monospace on the right.
pub(crate) fn kv(ui: &mut egui::Ui, k: &str, v: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).color(theme::text_dim()).size(12.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(v).monospace().size(12.0));
        });
    });
    ui.separator();
}

// ---------------------------------------------------------------- entry point
/// Starts the window: undecorated, 1320 x 820 at first and at least 900 x 600, drag and drop on, maximised when --maximized is given.
fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1320.0, 820.0]).with_min_inner_size([900.0, 600.0]).with_icon(std::sync::Arc::new(app_icon())).with_title("Zartex'27").with_decorations(false).with_maximized(std::env::args().any(|a| a == "--maximized")).with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Zartex'27", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}


/// The window icon (the same picture as the program icon).
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.resize(256, 256, image::imageops::FilterType::Lanczos3).to_rgba8());
    match img {
        Ok(i) => egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() },
        Err(_) => egui::IconData::default(),
    }
}
