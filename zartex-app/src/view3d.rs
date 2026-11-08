//! "City 3D" tab: real city geometry around the mission, with the mission's actors, objects and cameras placed on it.
//!
//! What it does: `CityView` draws the 3D view (orbit, pan, zoom), the mission objects as markers and camera frustums,
//! the player start, the cutscene camera path, the camera preview in a corner, and the minimap. It handles the mouse:
//! click to select, drag an object or the start point on the ground (Shift moves it up and down), drag a camera's aim
//! tip, drag on empty space to orbit. The result of a click or drag is returned as a `CityEvent` to the caller.
//! For Parallel Lines the same view shows the terrain tiles of the city file (`plassets`).
//! Uses: `city3d` (geometry, software renderer, city plan), `gpu` (graphics card renderer switch and its screen size),
//! `plassets` (Parallel Lines world), `camera` (`Pose` of the cutscene camera), `theme` (colours), `zartex_core` (objects).
//! Used by: `main.rs` (creates one `CityView` and calls `ui` every frame; it also builds the `CamExtras`).

use crate::camera::Pose;
use crate::city3d::{self, Camera, CityStore, Scene};
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use glam::Vec3;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use zartex_core::objects::MissionObject;

/// What the user did in the 3D view. Returned by `CityView::ui` to the caller.
#[derive(Clone, Copy, Debug)]
pub enum CityEvent {
    /// A mission object was clicked (index in the mission's object list).
    Click(usize),
    /// Drag of a camera's aim handle: the new direction from the camera
    AimCamera(usize, Vec3),
    /// A mission object was dragged to a new position.
    MoveObject(usize, Vec3),
    /// The player start was dragged to a new position.
    MoveStart(Vec3),
}

/// The things a mouse press in the view can grab (the hit targets of the last frame).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    /// A mission object, by index.
    Object(usize),
    /// The aim tip of a camera object, by index.
    CamTip(usize),
    /// The player start.
    Start,
}

/// Cutscene-camera overlays for the view.
pub struct CamExtras<'a> {
    /// Camera to preview in the corner (Driv3r city only)
    pub preview: Option<Pose>,
    /// Camera path: shot positions in order
    pub path: &'a [(Vec3, usize)],
    /// Sky dome file of the mission's mood, drawn behind the preview only
    pub sky: Option<&'a str>,
    /// Object labels and the info line over the map
    pub labels: bool,
    /// Road data of Parallel Lines drawn over the map
    pub roads: bool,
    /// Mission lighting of the camera preview (always used when the mood has it)
    pub preview_light: Option<crate::city3d::Lighting>,
    /// Mission lighting of the main map (only when the user turned it on)
    pub map_light: Option<crate::city3d::Lighting>,
    /// Show the minimap over the 3D view
    pub minimap: bool,
}

/// Size of the minimap square, in pixels.
const MM: f32 = 270.0;

/// The City 3D tab. It keeps the loaded city (or Parallel Lines world), the geometry around the camera, the minimap
/// picture and the camera preview, and redraws them when something changes.
pub struct CityView {
    /// Top-down picture of the city for the minimap, once built.
    plan: Option<crate::city3d::Plan>,
    plan_tex: Option<TextureHandle>,
    /// Receives the plan from the thread that builds it.
    plan_rx: Option<Receiver<Result<crate::city3d::Plan, String>>>,
    /// The city the plan belongs to (game folder and city type).
    plan_key: Option<(PathBuf, i32)>,
    plan_err: Option<String>,
    /// Centre of the minimap in world (x, z), and half of its width in world units.
    mm_center: [f32; 2],
    mm_half: f32,
    /// Target of the 3D view the last time the minimap looked at it.
    mm_last_target: Vec3,
    /// True while the mouse button is held down inside the minimap.
    mm_active: bool,
    last_radius: f32,
    map_light_on: bool,
    map_light_key: Option<crate::city3d::Lighting>,
    preview_light_key: Option<crate::city3d::Lighting>,
    /// Name of the sky file that `sky` was loaded from.
    sky_key: Option<String>,
    sky: Option<Scene>,
    sky_err: Option<String>,
    /// Geometry around the camera preview, and the point it was built around.
    prev_scene: Scene,
    prev_center: Vec3,
    tip_hits: Vec<(Target, Pos2)>,
    prev_tex: Option<TextureHandle>,
    /// Camera pose the preview image was last drawn for.
    last_pose: Option<[f32; 10]>,
    /// What the current mouse drag grabbed, if anything.
    drag: Option<Target>,
    /// Hit targets of the last frame, with their screen positions.
    last_hits: Vec<(Target, Pos2)>,
    /// The opened Driv3r city files (None until loaded).
    store: Option<Arc<CityStore>>,
    /// Receives the opened city files from the thread that reads them.
    loading: Option<Receiver<Result<CityStore, String>>>,
    /// Receives the Parallel Lines world from the thread that opens it.
    pl_loading: Option<Receiver<Result<crate::plassets::PlWorld, String>>>,
    pl_world: Option<Arc<crate::plassets::PlWorld>>,
    /// True once the test variables of the Parallel Lines view have been applied.
    pl_env_done: bool,
    /// The Parallel Lines city file of the current world.
    pl_key: Option<PathBuf>,
    /// The game folder and city type of the Driv3r city in `store`.
    key: Option<(PathBuf, i32)>,
    /// Geometry around `scene_center` that is drawn in the main view.
    scene: Scene,
    scene_center: Vec3,
    /// The orbit camera of the main view.
    pub cam: Camera,
    tex: Option<TextureHandle>,
    /// True when the main view must be drawn again.
    dirty: bool,
    /// Size in pixels of the last main-view image.
    last_px: [usize; 2],
    /// Error to show in place of the view (for example a city that cannot be read).
    pub error: Option<String>,
    /// Half-width of the area loaded around the view, in world units (260 without the GPU, 650 with it).
    pub radius: f32,
    /// When set, the next frame moves the camera target here.
    pub focus: Option<Vec3>,
    /// Line of information shown under the view (counts, load time, and so on).
    pub status: String,
}

/// Position of a mission object in the world, or `None` for objects that have no position (props) and for a path with
/// no points. A path uses its first point.
pub fn object_pos(o: &MissionObject) -> Option<Vec3> {
    use MissionObject::*;
    let v3 = |p: &zartex_core::binio::Vec3| Vec3::new(p.x, p.y, p.z);
    Some(match o {
        Vehicle { position, .. } | Character { position, .. } | Volume { position, .. } | ObjectiveIcon { position, .. } => v3(position),
        Camera { position, .. } | Switch { position, .. } | Collectable { position, .. } | AnimProp { position } | Marker { position, .. } => v3(position),
        Area { position, .. } => Vec3::new(position.x, position.y, position.z),
        Path { path, .. } => path.first().map(|p| Vec3::new(p.x, p.y, p.z))?,
        Prop { .. } => return None,
    })
}

/// Marker colour and display name of a mission object type, used by the 3D view and the minimap.
fn style(o: &MissionObject) -> (Color32, &'static str) {
    use MissionObject::*;
    match o {
        Vehicle { .. } => (Color32::from_rgb(0x00, 0x67, 0xC0), "Vehicle"),
        Character { .. } => (Color32::from_rgb(0x10, 0x7C, 0x10), "Character"),
        Camera { .. } => (Color32::from_rgb(0x75, 0x4A, 0xB8), "Camera"),
        Area { .. } => (Color32::from_rgb(0xC1, 0x7A, 0x00), "Area"),
        Volume { .. } => (Color32::from_rgb(0xC1, 0x7A, 0x00), "Volume"),
        Path { .. } => (Color32::from_rgb(0x0F, 0x7B, 0x6C), "Path"),
        ObjectiveIcon { .. } => (Color32::from_rgb(0xC4, 0x2B, 0x1C), "Objective"),
        AnimProp { .. } => (Color32::from_rgb(0xB1, 0x46, 0x9B), "Animation"),
        _ => (Color32::from_rgb(0x5F, 0x5F, 0x5F), "Object"),
    }
}

impl CityView {
    /// Forget the current view so that the next frame centres on the mission.
    pub fn recenter(&mut self) {
        self.cam.target = Vec3::ZERO;
        self.cam.dist = 90.0;
        self.scene_center = Vec3::splat(f32::MAX);
        self.prev_center = Vec3::splat(f32::MAX);
        self.mm_last_target = Vec3::splat(f32::MAX);
        self.dirty = true;
    }

    /// Creates an empty view: nothing loaded, the camera at the origin.
    pub fn new() -> Self {
        CityView {
            plan: None,
            plan_tex: None,
            plan_rx: None,
            plan_key: None,
            plan_err: None,
            mm_center: [0.0, 0.0],
            mm_half: 700.0,
            mm_last_target: Vec3::splat(f32::MAX),
            mm_active: false,
            last_radius: 0.0,
            map_light_on: false,
            map_light_key: None,
            preview_light_key: None,
            sky_key: None,
            sky: None,
            sky_err: None,
            prev_scene: Scene::default(),
            prev_center: Vec3::splat(f32::MAX),
            tip_hits: Vec::new(),
            prev_tex: None,
            last_pose: None,
            drag: None,
            last_hits: Vec::new(),
            store: None,
            loading: None,
            pl_loading: None,
            pl_world: None,
            pl_env_done: false,
            pl_key: None,
            key: None,
            scene: Scene::default(),
            scene_center: Vec3::splat(f32::MAX),
            cam: Camera::orbit(Vec3::ZERO),
            tex: None,
            dirty: true,
            last_px: [0, 0],
            error: None,
            radius: 260.0,
            focus: None,
            status: String::new(),
        }
    }

    /// Starts reading the Driv3r city files in a thread when the game folder or city type changes, and keeps the result
    /// when it arrives. The view shows "Loading city..." until then.
    fn ensure_store(&mut self, game: &Path, city: i32) {
        let key = (game.to_path_buf(), city);
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.store = None;
            self.scene = Scene::default();
            self.scene_center = Vec3::splat(f32::MAX);
            self.error = None;
            let (tx, rx) = channel();
            let g = game.to_path_buf();
            std::thread::spawn(move || {
                let _ = tx.send(CityStore::open(&g, city));
            });
            self.loading = Some(rx);
        }
        if let Some(rx) = &self.loading {
            if let Ok(r) = rx.try_recv() {
                self.loading = None;
                match r {
                    Ok(s) => {
                        self.store = Some(Arc::new(s));
                        self.dirty = true;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    /// Draws the 3D view for one frame and returns what the user did in it, if anything.
    /// `game` is the game folder (`None` when not set), `city` the mission's city type, `objs` the mission objects,
    /// `start` the player start, `selected` the selected object. `landmarks` is `Some` for Parallel Lines (its named points)
    /// and `None` for Driv3r. `extras` holds the cutscene overlays.
    pub fn ui(&mut self, ui: &mut egui::Ui, game: Option<&Path>, city: i32, objs: &[MissionObject], start: Option<Vec3>, selected: Option<usize>, landmarks: Option<&[(String, Vec3)]>, extras: &CamExtras) -> Option<CityEvent> {
        // ---- Driv3r: the city files must be open before anything can be drawn
        if landmarks.is_none() {
            let Some(game) = game else {
                ui.centered_and_justified(|ui| ui.label("Set the Driv3r game folder in File > Settings to see the city."));
                return None;
            };
            if city3d::city_dir(city).is_none() {
                ui.centered_and_justified(|ui| ui.label("This mission has no known Driv3r city."));
                return None;
            }
            self.ensure_store(game, city);
            if let Some(e) = &self.error {
                ui.colored_label(Color32::from_rgb(0xC4, 0x2B, 0x1C), e.clone());
                return None;
            }
            if self.store.is_none() {
                ui.ctx().request_repaint();
                ui.centered_and_justified(|ui| ui.label("Loading city..."));
                return None;
            }
        } else {
            // ---- Parallel Lines: the city file (the `*_pc.d4c` of the city type) is opened in a thread; its terrain
            // tiles, props and roads make up the world
            if let Some(root) = game {
                let files = crate::plassets::city_files(root);
                let file = files.get((city.max(0) as usize).min(files.len().saturating_sub(1))).cloned();
                if file != self.pl_key {
                    self.pl_key = file.clone();
                    self.scene = Scene::default();
                    self.pl_world = None;
                    self.plan = None;
                    self.plan_tex = None;
                    self.plan_key = None;
                    self.plan_rx = None;
                    self.scene_center = Vec3::splat(f32::MAX);
                    self.dirty = true;
                    if let Some(f) = file {
                        let (tx, rx) = channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::plassets::PlWorld::open(&f));
                        });
                        self.pl_loading = Some(rx);
                        self.status = "Loading the Parallel Lines city...".into();
                    }
                }
                if let Some(rx) = &self.pl_loading {
                    ui.ctx().request_repaint();
                    if let Ok(r) = rx.try_recv() {
                        self.pl_loading = None;
                        match r {
                            Ok(w) => {
                                self.pl_world = Some(Arc::new(w));
                                self.scene_center = Vec3::splat(f32::MAX);
                                self.dirty = true;
                            }
                            Err(e) => self.status = format!("Parallel Lines city: {e}"),
                        }
                    }
                }
            }
            // until the world is loaded: start at the origin, 800 units away (test variables may change this)
            if self.scene_center.x == f32::MAX && self.pl_world.is_none() {
                self.scene_center = Vec3::ZERO;
                self.cam.dist = std::env::var("ZM_CITY_DIST").ok().and_then(|v| v.parse().ok()).unwrap_or(800.0);
                if let Some(t) = std::env::var("ZM_CITY_TARGET").ok().and_then(|v| { let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect(); (p.len() == 3).then(|| Vec3::new(p[0], p[1], p[2])) }) {
                    self.cam.target = t;
                }
            }
        }
        // a new target asked for by the rest of the app (for example the selected object)
        if let Some(f) = self.focus.take() {
            self.cam.target = f;
            self.dirty = true;
        }
        // the graphics card path covers a larger area around the camera, so more geometry is loaded
        let gpu_on = crate::gpu::enabled();
        let want_radius = if gpu_on { 650.0 } else { 260.0 };
        if (want_radius - self.radius).abs() > 1.0 {
            self.radius = want_radius;
            self.scene_center = Vec3::splat(f32::MAX);
        }
        // ---- mouse input: the view takes the whole area; the minimap takes the mouse while it is over it
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let mm_rect = Rect::from_min_size(rect.right_top() + Vec2::new(-(MM + 10.0), 10.0), Vec2::splat(MM));
        let ptr = ui.input(|i| i.pointer.hover_pos());
        let over_mm = extras.minimap && ptr.map(|p| mm_rect.contains(p)).unwrap_or(false);
        if ui.input(|i| i.pointer.any_pressed()) && over_mm {
            self.mm_active = true;
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.mm_active = false;
        }
        let block = over_mm || self.mm_active;
        let mut event = None;
        // a press starts a drag on the nearest hit target within 16 pixels, if there is one
        if resp.drag_started_by(egui::PointerButton::Primary) && !block {
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                self.drag = self.last_hits.iter().map(|(t, s)| (*t, s.distance(o))).filter(|(_, d)| *d < 16.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(t, _)| t);
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if resp.dragged_by(egui::PointerButton::Primary) && !block {
            if let (Some(t), Some(m)) = (self.drag, resp.interact_pointer_pos()) {
                let cur = match t {
                    Target::Object(i) => objs.get(i).and_then(object_pos),
                    Target::Start => start,
                    Target::CamTip(i) => match objs.get(i) {
                        Some(MissionObject::Camera { position, forward, .. }) => Some(Vec3::new(position.x - forward.x * 6.0, position.y - forward.y * 6.0, position.z - forward.z * 6.0)),
                        _ => None,
                    },
                };
                if let Some(cur) = cur {
                    // the dragged point moves on the ground plane at its current height; with Shift it moves up and down
                    let vp = self.cam.view_proj(rect.width() / rect.height());
                    let shift = ui.input(|i| i.modifiers.shift);
                    let new = if shift {
                        // vertical move: screen-up raises the point
                        let d = resp.drag_delta();
                        Some(Vec3::new(cur.x, cur.y - d.y * self.cam.dist * 0.002, cur.z))
                    } else {
                        ground_hit(&vp, rect, m, cur.y).map(|g| Vec3::new(g.x, cur.y, g.z))
                    };
                    if let Some(n) = new {
                        event = Some(match t {
                            Target::Object(i) => CityEvent::MoveObject(i, n),
                            Target::Start => CityEvent::MoveStart(n),
                            Target::CamTip(i) => {
                                let p0 = objs.get(i).and_then(object_pos).unwrap_or(n);
                                CityEvent::AimCamera(i, n - p0)
                            }
                        });
                    }
                }
            } else {
                // drag on empty space: orbit (yaw and pitch)
                let d = resp.drag_delta();
                self.cam.yaw -= d.x * 0.008;
                self.cam.pitch = (self.cam.pitch + d.y * 0.008).clamp(-0.1, 1.5);
                self.dirty = true;
            }
        }
        // right or middle drag: pan the target across the ground
        if (resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle)) && !block {
            let d = resp.drag_delta();
            let (sy, cy) = (self.cam.yaw.sin(), self.cam.yaw.cos());
            let k = self.cam.dist * 0.0016;
            self.cam.target += Vec3::new(-cy * d.x + -sy * d.y, 0.0, sy * d.x + -cy * d.y) * k;
            self.dirty = true;
        }
        if resp.hovered() && !block {
            let sc = ui.input(|i| i.smooth_scroll_delta.y);
            if sc != 0.0 {
                self.cam.dist = (self.cam.dist * (1.0 - sc * 0.0015)).clamp(4.0, if landmarks.is_some() { 5000.0 } else { 900.0 });
                self.dirty = true;
            }
        }
        // ---- geometry: reload when the target has moved more than 45% of the loaded radius from the last loaded centre
        // (Driv3r; Parallel Lines below)
        if landmarks.is_none() && (self.scene_center.x == f32::MAX || (self.cam.target - self.scene_center).length() > self.radius * 0.45) {
            if let Some(store) = &self.store {
                match store.region(self.cam.target.to_array(), self.radius) {
                    Ok(s) => {
                        self.status = format!("{} instances, {} triangles ({} ms)", s.instances, s.verts.len() / 3, s.load_ms);
                        self.scene = s;
                        self.scene_center = self.cam.target;
                        self.dirty = true;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        // once per world: the test variables ZM_CITY_DIST, ZM_CITY_TARGET and ZM_CITY_PITCH set the start view
        if self.pl_world.is_some() && !self.pl_env_done {
            self.pl_env_done = true;
            if let Some(d) = std::env::var("ZM_CITY_DIST").ok().and_then(|v| v.parse::<f32>().ok()) {
                self.cam.dist = d;
            }
            if let Some(t) = std::env::var("ZM_CITY_TARGET").ok().and_then(|v| { let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect(); (p.len() == 3).then(|| Vec3::new(p[0], p[1], p[2])) }) {
                self.cam.target = t;
                self.focus = None;
            }
            if let Some(p) = std::env::var("ZM_CITY_PITCH").ok().and_then(|v| v.parse::<f32>().ok()) {
                self.cam.pitch = p;
            }
        }
        // Parallel Lines: the terrain tiles and the props near the target (props only when zoomed in)
        if let (Some(w), true) = (&self.pl_world, landmarks.is_some()) {
            if self.scene_center.x == f32::MAX || (self.cam.target - self.scene_center).length() > self.radius * 0.45 {
                let near = self.cam.dist < 700.0;
                let s = w.scene(self.cam.target.to_array(), self.radius, near);
                self.status = format!("Parallel Lines: {} tiles and props, {} triangles ({} ms){}", s.instances, s.verts.len() / 3, s.load_ms, if near { "" } else { " - zoom in to see the props" });
                self.scene = s;
                self.scene_center = self.cam.target;
                self.dirty = true;
            }
        }
        // with the graphics card: render at 1.5x and let the display filter it down (smoother edges)
        let scale = if gpu_on { 1.5f32 } else { 0.8f32 };
        let px = [((rect.width() * scale) as usize).clamp(64, 3000), ((rect.height() * scale) as usize).clamp(48, 2000)];
        // ---- drawing the main view: the image is rendered again only when something changed
        if self.map_light_on != extras.map_light.is_some() || self.map_light_key != extras.map_light {
            self.map_light_on = extras.map_light.is_some();
            self.map_light_key = extras.map_light.clone();
            self.dirty = true;
        }
        if self.dirty || px != self.last_px || self.tex.is_none() {
            let img = city3d::render_lit(&self.scene, &self.cam, px[0], px[1], extras.map_light.as_ref());
            match &mut self.tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.tex = Some(ui.ctx().load_texture("city3d", img, TextureOptions::LINEAR)),
            }
            self.last_px = px;
            self.dirty = false;
        }
        let painter = ui.painter_at(rect);
        if let Some(t) = &self.tex {
            painter.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let vp = self.cam.view_proj(rect.width() / rect.height());
        // world point to a screen position inside `rect` (None when behind the camera)
        let to_screen = |p: Vec3| city3d::project(&vp, p, rect.width(), rect.height()).map(|(x, y, _)| rect.min + Vec2::new(x, y));
        // Parallel Lines roads, drawn on the ground; a road is skipped when its first point is far from the target
        if let (true, Some(w)) = (extras.roads, &self.pl_world) {
            let t = self.cam.target;
            let near = (self.cam.dist * 2.0).clamp(300.0, 1500.0);
            for line in &w.roads {
                if let Some(p0) = line.first() {
                    if (p0[0] - t.x).abs() > near + 400.0 || (p0[1] - t.z).abs() > near + 400.0 {
                        continue;
                    }
                }
                let mut prev: Option<Pos2> = None;
                for q in line {
                    let sp = to_screen(Vec3::new(q[0], w.ground_height(q[0], q[1]) + 0.6, q[1]));
                    if let (Some(a), Some(b)) = (prev, sp) {
                        painter.line_segment([a, b], Stroke::new(2.0, Color32::from_rgb(0x00, 0xC8, 0xE6)));
                    }
                    prev = sp;
                }
            }
        }
        // Parallel Lines: a ground grid around the target and the named points; the grid spacing grows with the distance
        if let Some(lm) = landmarks {
            let step = if self.cam.dist > 600.0 { 500.0 } else if self.cam.dist > 150.0 { 100.0 } else { 20.0 };
            let (cx, cz) = ((self.cam.target.x / step).round() * step, (self.cam.target.z / step).round() * step);
            // the grid is drawn only while no geometry is loaded (with n = -1 the range below is empty)
            let n = if self.scene.verts.is_empty() { 24 } else { -1 };
            let gc = Color32::from_rgb(0xB8, 0xC4, 0xD0);
            for k in -n..=n {
                let d = k as f32 * step;
                for (a, b) in [(Vec3::new(cx + d, 0.0, cz - n as f32 * step), Vec3::new(cx + d, 0.0, cz + n as f32 * step)), (Vec3::new(cx - n as f32 * step, 0.0, cz + d), Vec3::new(cx + n as f32 * step, 0.0, cz + d))] {
                    // clip against the near plane by sampling the line
                    let mut prev: Option<Pos2> = None;
                    for t in 0..=16 {
                        let q = a.lerp(b, t as f32 / 16.0);
                        let sp = to_screen(q);
                        if let (Some(p0), Some(p1)) = (prev, sp) {
                            painter.line_segment([p0, p1], Stroke::new(1.0, gc));
                        }
                        prev = sp;
                    }
                }
            }
            for (name, p) in lm {
                if let Some(sp) = to_screen(*p) {
                    if rect.contains(sp) {
                        painter.circle(sp, 4.0, Color32::from_rgb(0x60, 0x6B, 0x78), Stroke::new(1.5, Color32::WHITE));
                        painter.text(sp + Vec2::new(7.0, -2.0), Align2::LEFT_BOTTOM, name, FontId::proportional(11.5), Color32::from_rgb(0x45, 0x4F, 0x5C));
                    }
                }
            }
        }
        // ---- mission objects: a marker per object, a pole down to the ground, and extras for cameras and paths
        self.tip_hits.clear();
        let mut hits: Vec<(Target, Pos2)> = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            let Some(p) = object_pos(o) else { continue };
            let Some(s) = to_screen(p) else { continue };
            if !rect.contains(s) {
                continue;
            }
            let (col, name) = style(o);
            let sel = selected == Some(i);
            let r = if sel { 8.0 } else { 5.5 };
            // pole from the ground so the height is readable
            if let Some(g) = to_screen(Vec3::new(p.x, p.y - 2.0, p.z)) {
                painter.line_segment([s, g], Stroke::new(1.5, col.gamma_multiply(0.6)));
            }
            painter.circle(s, r, col, Stroke::new(if sel { 2.5 } else { 1.5 }, Color32::WHITE));
            if let MissionObject::Camera { right, up, forward, .. } = o {
                // the camera's view: a 16:9 rectangle 6 units in front of it (along -forward, as the stored forward points
                // backwards); its half height is 6 * tan(27.5 degrees), so the field of view is 55 degrees
                let (f, r, u) = (-Vec3::new(forward.x, forward.y, forward.z), Vec3::new(right.x, right.y, right.z), Vec3::new(up.x, up.y, up.z));
                let d = 6.0;
                let hh = d * 0.5206;
                let hw = hh * 16.0 / 9.0;
                // centre of the rectangle: the aim tip, which can be dragged to change the direction
                let c = p + f * d;
                let corners: Vec<Option<Pos2>> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(a, b)| to_screen(c + r * (hw * a) + u * (hh * b))).collect();
                let st = Stroke::new(if sel { 2.0 } else { 1.3 }, col);
                for k in 0..4 {
                    if let (Some(a), Some(b)) = (corners[k], corners[(k + 1) % 4]) {
                        painter.line_segment([a, b], st);
                    }
                    if let Some(a) = corners[k] {
                        painter.line_segment([s, a], Stroke::new(1.0, col.gamma_multiply(0.7)));
                    }
                }
                if let Some(t) = to_screen(c) {
                    painter.line_segment([s, t], Stroke::new(2.0, col));
                    painter.circle(t, 4.5, Color32::WHITE, Stroke::new(2.0, col));
                    self.tip_hits.push((Target::CamTip(i), t));
                }
            }
            // a path object: its points joined by a line
            if let MissionObject::Path { path, .. } = o {
                let pts: Vec<Pos2> = path.iter().filter_map(|q| to_screen(Vec3::new(q.x, q.y, q.z))).collect();
                if pts.len() > 1 {
                    painter.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
                }
            }
            // labels: for the selected object, and for all of them when zoomed in
            if extras.labels && (sel || self.cam.dist < 120.0) {
                painter.text(s + Vec2::new(10.0, -10.0), Align2::LEFT_BOTTOM, format!("{name} {i}"), FontId::proportional(12.0), theme::text());
            }
            hits.push((Target::Object(i), s));
        }
        // ---- player start: a green ring with its label, a hit target for dragging
        if let Some(sp) = start {
            if let Some(s) = to_screen(sp) {
                painter.circle_stroke(s, 12.0, Stroke::new(2.5, Color32::from_rgb(0x10, 0x7C, 0x10)));
                painter.text(s + Vec2::new(14.0, 0.0), Align2::LEFT_CENTER, "Player start", FontId::proportional(12.0), Color32::from_rgb(0x10, 0x7C, 0x10));
                hits.push((Target::Start, s));
            }
        }
        // ---- cutscene camera path: a line through the shot positions, each numbered
        if extras.path.len() > 1 {
            let pts: Vec<Pos2> = extras.path.iter().filter_map(|(q, _)| to_screen(*q)).collect();
            if pts.len() > 1 {
                painter.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(0x75, 0x4A, 0xB8).gamma_multiply(0.85))));
            }
            for (q, n) in extras.path {
                if let Some(sp) = to_screen(*q) {
                    painter.circle(sp, 9.0, Color32::from_rgb(0x75, 0x4A, 0xB8), Stroke::new(1.5, Color32::WHITE));
                    painter.text(sp, Align2::CENTER_CENTER, n.to_string(), FontId::proportional(11.0), Color32::WHITE);
                }
            }
        }
        // ---- camera preview (Driv3r only): what the selected cutscene camera sees, in a corner of the view
        if let (Some(pose), true) = (extras.preview, landmarks.is_none()) {
            // the pose as a plain array: the preview is drawn again only when this changes
            let key = [pose.pos[0], pose.pos[1], pose.pos[2], pose.fwd[0], pose.fwd[1], pose.fwd[2], pose.up[0], pose.up[1], pose.up[2], pose.fov];
            // the mission's sky dome (only for the preview); reloaded only when its name changes
            match (extras.sky, game) {
                (Some(name), Some(g)) => {
                    if self.sky_key.as_deref() != Some(name) {
                        self.sky_key = Some(name.to_string());
                        match city3d::load_sky(g, name) {
                            Ok(sc) => {
                                self.sky = Some(sc);
                                self.sky_err = None;
                            }
                            Err(e) => {
                                self.sky = None;
                                self.sky_err = Some(e);
                            }
                        }
                        self.last_pose = None;
                    }
                }
                _ => {
                    if self.sky.is_some() || self.sky_key.is_some() {
                        self.sky = None;
                        self.sky_key = None;
                        self.last_pose = None;
                    }
                }
            }
            // the preview camera can be far from the main view: load the city around it
            let eye0 = Vec3::from(pose.pos);
            if self.prev_center.x == f32::MAX || (eye0 - self.prev_center).length() > 150.0 {
                if let Some(store) = &self.store {
                    if let Ok(sc) = store.region(pose.pos, 450.0) {
                        self.prev_scene = sc;
                        self.prev_center = eye0;
                        self.last_pose = None;
                    }
                }
            }
            if self.preview_light_key != extras.preview_light {
                self.preview_light_key = extras.preview_light.clone();
                self.last_pose = None;
            }
            if self.last_pose != Some(key) || self.prev_tex.is_none() {
                // the preview is 384 by 216 pixels; its view and projection come from the camera's pose
                let (w, h) = (384usize, 216usize);
                let eye = Vec3::from(pose.pos);
                let view = glam::Mat4::look_to_rh(eye, Vec3::from(pose.fwd), Vec3::from(pose.up));
                let proj = glam::Mat4::perspective_rh(pose.fov.to_radians(), w as f32 / h as f32, 0.3, 6000.0);
                let img = city3d::render_vp(&self.prev_scene, &(proj * view), eye, w, h, self.sky.as_ref(), extras.preview_light.as_ref());
                match &mut self.prev_tex {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => self.prev_tex = Some(ui.ctx().load_texture("city3d_cam", img, TextureOptions::LINEAR)),
                }
                self.last_pose = Some(key);
            }
            if let Some(t) = &self.prev_tex {
                let r = Rect::from_min_size(rect.right_bottom() - Vec2::new(384.0 + 12.0, 216.0 + 28.0), Vec2::new(384.0, 216.0));
                painter.rect_filled(r.expand(3.0), 6.0, Color32::WHITE);
                painter.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                painter.rect_stroke(r.expand(3.0), 6.0, Stroke::new(1.5, Color32::from_rgb(0x75, 0x4A, 0xB8)), egui::StrokeKind::Outside);
                painter.text(r.left_bottom() + Vec2::new(2.0, 14.0), Align2::LEFT_CENTER, format!("Camera preview  (fov {:.0})", pose.fov), FontId::proportional(11.5), theme::text_dim());
            }
        }
        // ---- minimap (drawn last, on top); a click on it moves the 3D view there
        let jump = self.minimap(ui, mm_rect, rect, over_mm, objs, start, selected, landmarks, extras, game, city);
        if let Some((x, z)) = jump {
            self.cam.target.x = x;
            self.cam.target.z = z;
            self.dirty = true;
            ui.ctx().request_repaint();
        }
        // ---- a click (not a drag) selects the nearest object within 14 pixels
        if resp.clicked() && event.is_none() && !block {
            if let Some(m) = resp.interact_pointer_pos() {
                let t = hits.iter().map(|(t, s)| (*t, s.distance(m))).filter(|(_, d)| *d < 14.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(t, _)| t);
                if let Some(Target::Object(i)) = t {
                    event = Some(CityEvent::Click(i));
                }
            }
        }
        // aim tips are hit targets too; the hits of this frame are used for dragging in the next one
        hits.extend(self.tip_hits.drain(..));
        self.last_hits = hits;
        if event.is_some() {
            ui.ctx().request_repaint();
        }
        // ---- status line under the view (Driv3r main view only, when labels are on)
        if extras.labels && extras.preview.is_none() {
            painter.text(rect.left_bottom() + Vec2::new(8.0, -6.0), Align2::LEFT_BOTTOM, format!("{}  -  drag: orbit, right-drag: pan, wheel: zoom", self.status), FontId::proportional(11.5), theme::text_dim());
        }
        event
    }
}

impl CityView {
    /// The minimap: city plan, mission elements, the area the 3D view shows; scroll zooms, a click or drag moves the 3D
    /// view there, right-drag pans the minimap. Returns the new view centre (x, z) when the user asked for one.
    #[allow(clippy::too_many_arguments)]
    fn minimap(&mut self, ui: &mut egui::Ui, mm: Rect, view: Rect, over: bool, objs: &[MissionObject], start: Option<Vec3>, selected: Option<usize>, landmarks: Option<&[(String, Vec3)]>, extras: &CamExtras, game: Option<&Path>, city: i32) -> Option<(f32, f32)> {
        if !extras.minimap {
            return None;
        }
        // the city plan is built once per city, in a background thread (Driv3r)
        if landmarks.is_none() {
            if let (Some(g), Some(store)) = (game, self.store.clone()) {
                let key = (g.to_path_buf(), city);
                if self.plan_key.as_ref() != Some(&key) {
                    self.plan_key = Some(key);
                    self.plan = None;
                    self.plan_tex = None;
                    self.plan_err = None;
                    let (tx, rx) = channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(store.plan());
                    });
                    self.plan_rx = Some(rx);
                }
            }
            if let Some(rx) = &self.plan_rx {
                if let Ok(r) = rx.try_recv() {
                    self.plan_rx = None;
                    match r {
                        Ok(p) => {
                            self.plan_tex = Some(ui.ctx().load_texture("cityplan", p.img.clone(), TextureOptions::LINEAR));
                            self.plan = Some(p);
                        }
                        Err(e) => self.plan_err = Some(e),
                    }
                } else {
                    ui.ctx().request_repaint();
                }
            }
        }
        // Parallel Lines: the plan is built from the world once, in a background thread
        if landmarks.is_some() {
            if let Some(w) = self.pl_world.clone() {
                if self.plan_key.is_none() {
                    self.plan_key = Some((PathBuf::from("pl"), 0));
                    let (tx, rx) = channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(Ok(w.plan()));
                    });
                    self.plan_rx = Some(rx);
                }
            }
            if let Some(rx) = &self.plan_rx {
                if let Ok(r) = rx.try_recv() {
                    self.plan_rx = None;
                    if let Ok(p) = r {
                        self.plan_tex = Some(ui.ctx().load_texture("cityplan", p.img.clone(), TextureOptions::LINEAR));
                        self.plan = Some(p);
                    }
                } else {
                    ui.ctx().request_repaint();
                }
            }
        }
        // follow the 3D view when it was moved by something else
        let t = self.cam.target;
        if self.mm_last_target.x == f32::MAX || (t - self.mm_last_target).length() > 0.01 {
            self.mm_center = [t.x, t.z];
        }
        // scale: pixels per world unit, for a minimap that shows `half` units from its centre to its edge
        let k = |half: f32| MM / (2.0 * half);
        // world (x, z) to minimap pixel, and back
        let to_screen = |c: [f32; 2], half: f32, x: f32, z: f32| Pos2::new(mm.center().x + (x - c[0]) * k(half), mm.center().y + (z - c[1]) * k(half));
        let to_world = |c: [f32; 2], half: f32, p: Pos2| ((p.x - mm.center().x) / k(half) + c[0], (p.y - mm.center().y) / k(half) + c[1]);
        // ---- input: wheel zooms around the pointer; a press or drag inside moves the 3D view, right-drag pans
        let mut jump = None;
        let ptr = ui.input(|i| i.pointer.hover_pos());
        if over {
            let sc = ui.input(|i| i.smooth_scroll_delta.y);
            if sc != 0.0 {
                if let Some(p) = ptr {
                    let before = to_world(self.mm_center, self.mm_half, p);
                    self.mm_half = (self.mm_half * (1.0 - sc * 0.002)).clamp(50.0, 6000.0);
                    let after = to_world(self.mm_center, self.mm_half, p);
                    self.mm_center[0] += before.0 - after.0;
                    self.mm_center[1] += before.1 - after.1;
                }
            }
        }
        if self.mm_active {
            let (prim, pan, delta) = ui.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down() || i.pointer.middle_down(), i.pointer.delta()));
            if prim {
                if let Some(p) = ptr {
                    let p = Pos2::new(p.x.clamp(mm.min.x, mm.max.x), p.y.clamp(mm.min.y, mm.max.y));
                    let w = to_world(self.mm_center, self.mm_half, p);
                    jump = Some((w.0, w.1));
                }
            } else if pan {
                self.mm_center[0] -= delta.x / k(self.mm_half);
                self.mm_center[1] -= delta.y / k(self.mm_half);
            }
        }
        if let Some((x, z)) = jump {
            // the minimap stays where it is while the user drags inside it
            self.mm_last_target = Vec3::new(x, t.y, z);
        } else {
            self.mm_last_target = t;
        }
        // ---- drawing: background, city plan, Parallel Lines roads and names, mission elements, the 3D view outline
        let painter = ui.painter_at(mm.expand(2.0));
        painter.rect_filled(mm.expand(2.0), 8.0, Color32::from_rgb(0x2B, 0x2F, 0x36));
        let inner = painter.with_clip_rect(mm);
        inner.rect_filled(mm, 6.0, Color32::from_rgb(0xE4, 0xE8, 0xEC));
        let (c, half) = (self.mm_center, self.mm_half);
        if let (Some(p), Some(tex)) = (&self.plan, &self.plan_tex) {
            let (vx0, vz0, vx1, vz1) = (c[0] - half, c[1] - half, c[0] + half, c[1] + half);
            let (ix0, iz0, ix1, iz1) = (vx0.max(p.min[0]), vz0.max(p.min[1]), vx1.min(p.max[0]), vz1.min(p.max[1]));
            if ix0 < ix1 && iz0 < iz1 {
                let uv = Rect::from_min_max(Pos2::new((ix0 - p.min[0]) / (p.max[0] - p.min[0]), (iz0 - p.min[1]) / (p.max[1] - p.min[1])), Pos2::new((ix1 - p.min[0]) / (p.max[0] - p.min[0]), (iz1 - p.min[1]) / (p.max[1] - p.min[1])));
                let dst = Rect::from_min_max(to_screen(c, half, ix0, iz0), to_screen(c, half, ix1, iz1));
                inner.image(tex.id(), dst, uv, Color32::WHITE);
            }
        } else {
            let msg = match (&self.plan_err, landmarks) {
                (Some(e), _) => e.clone(),
                (None, Some(_)) => "Building the city plan...".to_string(),
                _ => "Building the city plan...".to_string(),
            };
            inner.text(mm.center(), Align2::CENTER_CENTER, msg, FontId::proportional(11.5), theme::text_dim());
        }
        if let (true, Some(w)) = (extras.roads, &self.pl_world) {
            for line in &w.roads {
                let mut prev: Option<Pos2> = None;
                for q in line {
                    let s = to_screen(c, half, q[0], q[1]);
                    if let Some(a) = prev {
                        if mm.expand(40.0).contains(s) || mm.expand(40.0).contains(a) {
                            inner.line_segment([a, s], Stroke::new(1.0, Color32::from_rgb(0xD0, 0x40, 0x40)));
                        }
                    }
                    prev = Some(s);
                }
            }
        }
        if let Some(lm) = landmarks {
            for (name, p) in lm {
                let s = to_screen(c, half, p.x, p.z);
                if mm.contains(s) {
                    inner.circle_filled(s, 2.5, Color32::from_rgb(0x60, 0x6B, 0x78));
                    if half < 2500.0 {
                        inner.text(s + Vec2::new(4.0, -2.0), Align2::LEFT_BOTTOM, name, FontId::proportional(9.5), Color32::from_rgb(0x45, 0x4F, 0x5C));
                    }
                }
            }
        }
        // camera path of the open cutscene
        if extras.path.len() > 1 {
            let pts: Vec<Pos2> = extras.path.iter().map(|(q, _)| to_screen(c, half, q.x, q.z)).collect();
            inner.add(egui::Shape::line(pts, Stroke::new(1.5, Color32::from_rgb(0x75, 0x4A, 0xB8))));
        }
        // mission elements: a dot per object (a line for a camera's direction, and paths as lines)
        for (i, o) in objs.iter().enumerate() {
            let Some(p) = object_pos(o) else { continue };
            let s = to_screen(c, half, p.x, p.z);
            if !mm.expand(30.0).contains(s) {
                continue;
            }
            let (col, _) = style(o);
            if let MissionObject::Path { path, .. } = o {
                let pts: Vec<Pos2> = path.iter().map(|q| to_screen(c, half, q.x, q.z)).collect();
                inner.add(egui::Shape::line(pts, Stroke::new(1.5, col)));
            }
            let sel = selected == Some(i);
            if let MissionObject::Camera { forward, .. } = o {
                let tip = s + Vec2::new(-forward.x, -forward.z).normalized() * 11.0;
                inner.line_segment([s, tip], Stroke::new(1.5, col));
            }
            inner.circle(s, if sel { 5.0 } else { 3.2 }, col, Stroke::new(if sel { 2.0 } else { 1.0 }, Color32::WHITE));
        }
        if let Some(sp) = start {
            let s = to_screen(c, half, sp.x, sp.z);
            inner.circle_stroke(s, 6.0, Stroke::new(2.0, Color32::from_rgb(0x10, 0x7C, 0x10)));
        }
        // the area the 3D view shows (corners of the view hitting the ground at the view's height)
        // the outline is drawn as a polygon with its far corners limited to 500 units from the target
        let vp = self.cam.view_proj(view.width() / view.height());
        let corners = [Pos2::new(view.min.x, view.min.y), Pos2::new(view.max.x, view.min.y), Pos2::new(view.max.x, view.max.y), Pos2::new(view.min.x, view.max.y)];
        let hits: Vec<Option<Vec3>> = corners.iter().map(|q| ground_hit(&vp, view, *q, t.y)).collect();
        if hits.iter().all(|h| h.is_some()) {
            // a tilted view sees very far: keep the outline to 500 m around the centre so that it stays readable
            let pts: Vec<Pos2> = hits
                .iter()
                .map(|h| {
                    let g = h.unwrap();
                    let d = Vec3::new(g.x - t.x, 0.0, g.z - t.z);
                    let g = if d.length() > 500.0 { t + d.normalize() * 500.0 } else { g };
                    to_screen(c, half, g.x, g.z)
                })
                .collect();
            inner.add(egui::Shape::convex_polygon(pts.clone(), Color32::from_rgba_unmultiplied(0x00, 0x67, 0xC0, 30), Stroke::new(2.0, theme::accent())));
        }
        let ts = to_screen(c, half, t.x, t.z);
        inner.circle_filled(ts, 3.0, theme::accent());
        painter.rect_stroke(mm, 6.0, Stroke::new(1.5, Color32::from_rgb(0x9A, 0xA4, 0xB0)), egui::StrokeKind::Outside);
        painter.text(mm.left_bottom() + Vec2::new(6.0, -4.0), Align2::LEFT_BOTTOM, format!("scroll: zoom \u{00B7} click: go there \u{00B7} {:.0} m", half * 2.0), FontId::proportional(9.5), theme::text_dim());
        jump
    }
}

/// World point where the mouse ray meets the horizontal plane at height `y`.
fn ground_hit(vp: &glam::Mat4, rect: Rect, m: Pos2, y: f32) -> Option<Vec3> {
    let inv = vp.inverse();
    let (nx, ny) = ((m.x - rect.min.x) / rect.width() * 2.0 - 1.0, 1.0 - (m.y - rect.min.y) / rect.height() * 2.0);
    let un = |z: f32| {
        let p = inv * glam::Vec4::new(nx, ny, z, 1.0);
        p.truncate() / p.w
    };
    let (a, b) = (un(0.0), un(1.0));
    let d = b - a;
    if d.y.abs() < 1e-5 {
        return None;
    }
    let t = (y - a.y) / d.y;
    (t > 0.0).then(|| a + d * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Projects three ground points to the screen, then checks that the mouse ray through that pixel meets the same
    /// point again (`ground_hit` inverts the projection).
    #[test]
    fn mouse_ray_hits_the_projected_point() {
        let cam = Camera { target: Vec3::new(1293.0, 7.0, 4.0), yaw: 0.8, pitch: 0.55, dist: 90.0, fov_deg: 60.0 };
        let rect = Rect::from_min_size(Pos2::new(250.0, 90.0), Vec2::new(780.0, 520.0));
        let vp = cam.view_proj(rect.width() / rect.height());
        for p in [Vec3::new(1300.0, 7.5, 10.0), Vec3::new(1280.0, 7.5, -3.0), Vec3::new(1293.0, 2.0, 4.0)] {
            let (x, y, _) = city3d::project(&vp, p, rect.width(), rect.height()).unwrap();
            let hit = ground_hit(&vp, rect, rect.min + Vec2::new(x, y), p.y).unwrap();
            assert!((hit - p).length() < 0.05, "{p:?} -> {hit:?}");
        }
    }
}
