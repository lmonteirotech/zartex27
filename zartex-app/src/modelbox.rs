//! The optional 3D model box of the inspector: vehicles and characters.
//!
//! What it does: `ModelBox` shows one model (a vehicle, a character, or any model the caller builds) in a small square
//! picture that can be turned by dragging, zoomed with the wheel, and reset with a double click. `SkyBox` shows a sky
//! dome that can be looked around. Both draw with the software renderer of `city3d` (`render`, `render_vp`) and load
//! their data from the game folder through `vehicle3d` and `character3d`.
//! Uses: `city3d` (`Camera`, `Scene`, `render`, `render_vp`, `load_sky`), `vehicle3d` (`VehicleModel`, `load`), `character3d`
//! (`load`), `theme` (colours). Used by: `main.rs` (the inspector creates one `ModelBox` and one `SkyBox`).

use crate::character3d;
use crate::city3d::{self, Camera};
use crate::theme;
use crate::vehicle3d::{self, VehicleModel};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2};
use std::path::{Path, PathBuf};

/// A model box: the model currently shown, its orbit camera, and the picture drawn for the last camera state.
pub struct ModelBox {
    /// Game folder and a text key of the model; the model is loaded again when either changes.
    key: Option<(PathBuf, String)>,
    /// The loaded model, or the error that stopped the load.
    model: Option<Result<VehicleModel, String>>,
    cam: Camera,
    tex: Option<TextureHandle>,
    /// Picture size and camera (yaw, pitch, distance) of the last drawn image.
    last: Option<([usize; 2], [f32; 3])>,
}

impl ModelBox {
    /// An empty model box (nothing shown until one of the `vehicle`, `character` or `custom` methods is called).
    pub fn new() -> Self {
        ModelBox { key: None, model: None, cam: Camera::orbit(glam::Vec3::ZERO), tex: None, last: None }
    }

    /// Shows a vehicle of the city type with id `id`, loaded from the game folder. Loaded again only when the game,
    /// city or id changes.
    pub fn vehicle(&mut self, ui: &mut egui::Ui, game: &Path, city_type: i32, id: u32) {
        let key = (game.to_path_buf(), format!("v:{city_type}:{id}"));
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.model = Some(vehicle3d::load(game, city_type, id));
            self.reset_camera(1.0);
        }
        self.show(ui, &format!("Vehicle id {id}"), 0.68);
    }

    /// Shows a character skin (hash `skin`) of a mission's `.dam` file. Loaded again only when the mission or skin changes.
    pub fn character(&mut self, ui: &mut egui::Ui, game: &Path, mission_stem: &str, skin: u32) {
        let key = (game.to_path_buf(), format!("c:{mission_stem}:{skin}"));
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.model = Some(character3d::load(game, mission_stem, skin));
            self.reset_camera(1.0);
        }
        self.show(ui, &format!("Skin {skin:#010x}"), 1.15);
    }

    /// Any model built by the caller; `key` says when it has to be built again.
    pub fn custom(&mut self, ui: &mut egui::Ui, key: &str, label: &str, build: impl FnOnce() -> Result<VehicleModel, String>) {
        let k = (PathBuf::new(), format!("x:{key}"));
        if self.key.as_ref() != Some(&k) {
            self.key = Some(k);
            self.model = Some(build());
            self.reset_camera(1.0);
        }
        self.show(ui, label, 0.8);
    }

    /// Points the camera at the model's centre, from a distance of 2.3 times its radius, at a default angle.
    /// The argument is not used.
    fn reset_camera(&mut self, _scale: f32) {
        self.last = None;
        if let Some(Ok(m)) = &self.model {
            self.cam = Camera::orbit(m.center);
            self.cam.yaw = 0.7;
            self.cam.pitch = 0.25;
            self.cam.dist = m.radius * 2.3;
        }
    }

    /// Draws the model in a square-ish picture (height = width times `aspect`) with a caption. Drag turns the model,
    /// the wheel zooms (within 0.8 to 8 times the radius), a double click resets the view. The picture is rendered again
    /// only when the size or the camera changed.
    fn show(&mut self, ui: &mut egui::Ui, label: &str, aspect: f32) {
        match &self.model {
            Some(Err(e)) => {
                ui.label(RichText::new(e.clone()).size(12.0).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
            }
            Some(Ok(m)) => {
                let w = ui.available_width().max(120.0);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, (w * aspect).round()), Sense::click_and_drag());
                let mut changed = false;
                if resp.dragged() {
                    let d = resp.drag_delta();
                    self.cam.yaw += d.x * 0.01;
                    self.cam.pitch = (self.cam.pitch + d.y * 0.01).clamp(-0.4, 1.4);
                    changed = true;
                }
                if resp.hovered() {
                    let sc = ui.input(|i| i.smooth_scroll_delta.y);
                    if sc != 0.0 {
                        self.cam.dist = (self.cam.dist * (1.0 - sc * 0.002)).clamp(m.radius * 0.8, m.radius * 8.0);
                        changed = true;
                    }
                }
                let (radius, center) = (m.radius, m.center);
                if resp.double_clicked() {
                    self.cam = Camera::orbit(center);
                    self.cam.yaw = 0.7;
                    self.cam.pitch = 0.25;
                    self.cam.dist = radius * 2.3;
                    changed = true;
                }
                let px = [(rect.width() * 0.9) as usize, (rect.height() * 0.9) as usize];
                let key = [self.cam.yaw, self.cam.pitch, self.cam.dist];
                if changed || self.tex.is_none() || self.last != Some((px, key)) {
                    let img = city3d::render(&m.scene, &self.cam, px[0].max(32), px[1].max(24));
                    match &mut self.tex {
                        Some(t) => t.set(img, TextureOptions::LINEAR),
                        None => self.tex = Some(ui.ctx().load_texture("modelbox", img, TextureOptions::LINEAR)),
                    }
                    self.last = Some((px, key));
                }
                if let Some(t) = &self.tex {
                    ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, theme::card_stroke()), StrokeKind::Inside);
                }
                ui.label(RichText::new(format!("{label} \u{00B7} drag to turn, wheel to zoom")).size(11.0).color(theme::text_dim()));
            }
            None => {}
        }
    }
}

/// Looking around inside a sky dome (`Skies\*.d3s`).
pub struct SkyBox {
    /// Game folder and sky file name; the dome is loaded again when either changes.
    key: Option<(PathBuf, String)>,
    /// The loaded dome, or the error that stopped the load.
    sky: Option<Result<city3d::Scene, String>>,
    /// View direction: angle around Y and angle above the horizon, in radians.
    yaw: f32,
    pitch: f32,
    tex: Option<TextureHandle>,
    /// Picture size and view direction of the last drawn image.
    last: Option<([usize; 2], [f32; 2])>,
}

impl SkyBox {
    /// An empty sky box, looking slightly up (pitch 0.15).
    pub fn new() -> Self {
        SkyBox { key: None, sky: None, yaw: 0.0, pitch: 0.15, tex: None, last: None }
    }

    /// Shows the sky dome `name` of the game folder. Dragging changes the view direction; the picture is rendered
    /// again only when its size or the direction changed.
    pub fn show(&mut self, ui: &mut egui::Ui, game: &Path, name: &str) {
        let key = (game.to_path_buf(), name.to_string());
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.sky = Some(city3d::load_sky(game, name));
            self.last = None;
        }
        match &self.sky {
            Some(Err(e)) => {
                ui.label(RichText::new(e.clone()).size(12.0).color(Color32::from_rgb(0xC4, 0x2B, 0x1C)));
            }
            Some(Ok(sky)) => {
                let w = ui.available_width().max(120.0);
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, (w * 0.62).round()), Sense::click_and_drag());
                if resp.dragged() {
                    let d = resp.drag_delta();
                    self.yaw -= d.x * 0.008;
                    self.pitch = (self.pitch + d.y * 0.008).clamp(-1.2, 1.4);
                }
                let px = [(rect.width() * 0.9) as usize, (rect.height() * 0.9) as usize];
                if self.tex.is_none() || self.last != Some((px, [self.yaw, self.pitch])) {
                    let dir = glam::Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos());
                    let view = glam::Mat4::look_to_rh(glam::Vec3::ZERO, dir, glam::Vec3::Y);
                    let proj = glam::Mat4::perspective_rh(75f32.to_radians(), px[0].max(32) as f32 / px[1].max(24) as f32, 0.1, 6000.0);
                    let img = city3d::render_vp(&city3d::Scene::default(), &(proj * view), glam::Vec3::ZERO, px[0].max(32), px[1].max(24), Some(sky), None);
                    match &mut self.tex {
                        Some(t) => t.set(img, TextureOptions::LINEAR),
                        None => self.tex = Some(ui.ctx().load_texture("skybox", img, TextureOptions::LINEAR)),
                    }
                    self.last = Some((px, [self.yaw, self.pitch]));
                }
                if let Some(t) = &self.tex {
                    ui.painter().image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                    ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, theme::card_stroke()), StrokeKind::Inside);
                }
                ui.label(RichText::new("drag to look around").size(11.0).color(theme::text_dim()));
            }
            None => {}
        }
    }
}
