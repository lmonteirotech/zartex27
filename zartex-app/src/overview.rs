//! Plain-language summary of how a mission starts: place, time, weather, music, traffic.
//! Shown in the inspector when nothing is selected. For a Driv3r mission it lists the summary fields, which are editable,
//! and the mood file they point to, which is read-only. For Parallel Lines (no summary) it says so and points to the logic.
//! It also lists the logic nodes that change the environment while the mission runs, grouped by what they change.
//! Exposes show. Uses crate::model (summary, nodes, the Edit values), crate::mood (the mood file), crate::theme and crate::kv.
//! Used by main.rs only, in the inspector.

use crate::kv;
use crate::model::*;
use crate::mood::Mood;
use crate::theme;
use eframe::egui::{self, Color32, RichText, Vec2};

/// Section title: small grey text with a little space around it.
fn heading(ui: &mut egui::Ui, t: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(t).size(11.0).color(theme::text_dim()));
    ui.add_space(2.0);
}

/// A key and value row (kv from main.rs).
fn row(ui: &mut egui::Ui, k: &str, v: impl Into<String>) {
    kv(ui, k, v.into());
}

/// A key with a small box filled with an RGB colour. The components are from 0 to 1.
fn swatch(ui: &mut egui::Ui, k: &str, c: [f32; 3]) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).color(theme::text_dim()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let col = Color32::from_rgb((c[0].clamp(0.0, 1.0) * 255.0) as u8, (c[1].clamp(0.0, 1.0) * 255.0) as u8, (c[2].clamp(0.0, 1.0) * 255.0) as u8);
            let (r, _) = ui.allocate_exact_size(Vec2::new(38.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 4.0, col);
            ui.painter().rect_stroke(r, 4.0, egui::Stroke::new(1.0, theme::card_stroke()), egui::StrokeKind::Inside);
        });
    });
}

/// Turns a game value such as CLEAR_SKY into a sentence-case label such as Clear sky.
fn nice(s: &str) -> String {
    let mut c = s.to_lowercase().replace('_', " ");
    if let Some(f) = c.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    c
}

/// Last part of a path, split at a backslash or a slash.
fn file_name(p: &str) -> String {
    p.rsplit(['\\', '/']).next().unwrap_or(p).to_string()
}

/// Name of a city type number from the summary: 1 to 6, Miami, Nice and Istanbul, day or night.
fn city_name(t: i32) -> &'static str {
    match t {
        1 => "Miami, day",
        2 => "Miami, night",
        3 => "Nice, day",
        4 => "Nice, night",
        5 => "Istanbul, day",
        6 => "Istanbul, night",
        _ => "Unknown city",
    }
}

/// Draws the overview. Driv3r summary fields are editable: each change is pushed to edits as one Edit. mood_missing is the text shown when the mood file cannot be read.
pub fn show(ui: &mut egui::Ui, v: &MissionView, mood: Option<&Mood>, mood_missing: Option<String>, edits: &mut Vec<Edit>, moods: &[(i16, String)]) {
    ui.label(RichText::new("Mission overview").strong().size(15.0));
    ui.label(RichText::new("How the mission starts. Values here are editable; scripts can change most of this later.").size(11.5).color(theme::text_dim()));

    // a row with a label on the left and the control on the right
    let erow = |ui: &mut egui::Ui, k: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(k).color(theme::text_dim()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add(ui));
        });
    };

    // Driv3r: the summary fields are shown and can be edited
    if let Some(s) = &v.summary {
        heading(ui, "PLACE");
        let mut city = s.city_type;
        erow(ui, "City", &mut |ui| {
            egui::ComboBox::from_id_salt("ov_city").width(150.0).selected_text(city_name(city)).show_ui(ui, |ui| {
                for t in 1..=6 {
                    ui.selectable_value(&mut city, t, city_name(t));
                }
            });
        });
        if city != s.city_type {
            edits.push(Edit::Summary(SummaryEdit::City(city)));
        }
        // player start: x and z of the summary; a change is queued only when a value moved
        let (mut x, mut z) = (s.start_position.x, s.start_position.y);
        let mut moved = false;
        erow(ui, "Player starts at (x, z)", &mut |ui| {
            moved |= ui.add(egui::DragValue::new(&mut z).speed(0.5).max_decimals(1)).changed();
            moved |= ui.add(egui::DragValue::new(&mut x).speed(0.5).max_decimals(1)).changed();
        });
        if moved {
            edits.push(Edit::StartPos(x, z));
        }

        // the Mood picker below chooses the file; the values read from that file are shown, not edited
        heading(ui, "ENVIRONMENT");
        let mut mid = s.mission_id;
        erow(ui, "Mood", &mut |ui| {
            let cur = moods.iter().find(|m| m.0 == mid).map(|m| m.1.clone()).unwrap_or_else(|| format!("mood{mid:02}"));
            egui::ComboBox::from_id_salt("ov_mood").width(150.0).selected_text(cur).show_ui(ui, |ui| {
                for (id, label) in moods {
                    ui.selectable_value(&mut mid, *id, label);
                }
            });
        });
        if mid != s.mission_id {
            edits.push(Edit::Summary(SummaryEdit::MoodId(mid)));
        }
        // time, weather, rain, music, sky, sun, fog and light come from the mood file
        match mood {
            Some(m) => {
                if let Some(t) = m.get("TIME_OF_DAY") {
                    row(ui, "Time of day", nice(t));
                }
                if let Some(t) = m.get("WEATHER") {
                    row(ui, "Weather", nice(t));
                }
                if let Some(r) = m.num("RAIN") {
                    row(ui, "Rain", if r > 0.0 { format!("yes ({r})") } else { "none".into() });
                }
                if let Some(t) = m.get("MUSIC_NAME") {
                    row(ui, "Music", file_name(t));
                }
                if let Some(t) = m.get("SKYDOME_NAME") {
                    row(ui, "Sky", file_name(t));
                }
                if let Some(e) = m.num("SUN_ELEVATION") {
                    row(ui, "Sun height", format!("{e:.2}"));
                }
                if let (Some(a), Some(b)) = (m.num("FOG_NEARDISTANCE"), m.num("FOG_FARDISTANCE")) {
                    row(ui, "Fog", format!("{a:.0} to {b:.0} m"));
                }
                let on = |k: &str| m.num(k).map(|x| if x > 0.0 { "on" } else { "off" }).unwrap_or("-");
                row(ui, "Street lights", on("STREET_LIGHTS"));
                row(ui, "Headlights", on("HEAD_LIGHTS"));
                heading(ui, "LIGHT");
                if let Some(c) = m.rgb("LIGHT_AMBIENT_COLOUR") {
                    swatch(ui, "Ambient", c);
                }
                if let Some(c) = m.rgb("LIGHT_DIRECTIONAL_COLOUR") {
                    swatch(ui, "Sun", c);
                }
                if let Some(c) = m.rgb("FOG_COLOUR") {
                    swatch(ui, "Fog", c);
                }
                ui.label(RichText::new("Time, weather, music and sky belong to the mood file; pick another mood above to change them.").size(11.0).color(theme::text_dim()));
            }
            None => {
                ui.label(RichText::new(mood_missing.unwrap_or_else(|| "Mood file not found in the game folder.".into())).size(12.0).color(theme::text_dim()));
            }
        }

        // traffic numbers: each changed value is queued as its own Edit
        heading(ui, "TRAFFIC");
        let (mut pk, mut at, mut pin, mut pout, mut ov) = (s.parked_car_density, s.attractor_parked_car_density, s.ping_in_radius, s.ping_out_radius, s.density_override);
        erow(ui, "Parked cars", &mut |ui| {
            ui.add(egui::DragValue::new(&mut pk).speed(0.2));
        });
        erow(ui, "Attractor parked cars", &mut |ui| {
            ui.add(egui::DragValue::new(&mut at).speed(0.2));
        });
        erow(ui, "Ping in radius (m)", &mut |ui| {
            ui.add(egui::DragValue::new(&mut pin).speed(1.0).max_decimals(0));
        });
        erow(ui, "Ping out radius (m)", &mut |ui| {
            ui.add(egui::DragValue::new(&mut pout).speed(1.0).max_decimals(0));
        });
        erow(ui, "Density override", &mut |ui| {
            ui.checkbox(&mut ov, "");
        });
        if pk != s.parked_car_density {
            edits.push(Edit::Summary(SummaryEdit::ParkedDensity(pk)));
        }
        if at != s.attractor_parked_car_density {
            edits.push(Edit::Summary(SummaryEdit::AttractorDensity(at)));
        }
        if pin != s.ping_in_radius {
            edits.push(Edit::Summary(SummaryEdit::PingIn(pin)));
        }
        if pout != s.ping_out_radius {
            edits.push(Edit::Summary(SummaryEdit::PingOut(pout)));
        }
        if ov != s.density_override {
            edits.push(Edit::Summary(SummaryEdit::DensityOverride(ov)));
        }
    } else {
        heading(ui, "PLACE");
        row(ui, "Game", "Driver: Parallel Lines");
        ui.label(RichText::new("Parallel Lines sets weather, time and music from the mission's own logic nodes (see below).").size(12.0).color(theme::text_dim()));
    }
    // what the script changes while the mission runs
    let l = &v.mission.logic;
    // logic nodes that change the environment, grouped by what they change
    let mut groups: Vec<(&str, Vec<String>)> = vec![("Music", vec![]), ("Ambient sound", vec![]), ("Time of day", vec![]), ("Weather", vec![]), ("Traffic density", vec![]), ("Pedestrian density", vec![])];
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        let name = node_type_name(v, n.type_id);
        let k = if name.contains("Music") {
            0
        } else if name.contains("Ambient") {
            1
        } else if name.contains("TimeControl") || name.contains("TimeWatch") {
            2
        } else if name.contains("Weather") {
            3
        } else if name.contains("TrafficDensity") {
            4
        } else if name.contains("PedestrianDensity") {
            5
        } else {
            continue;
        };
        groups[k].1.push(format!("{} (node {i})", node_label(v, i)));
    }
    let any = groups.iter().any(|g| !g.1.is_empty());
    heading(ui, "CHANGED BY THE SCRIPT");
    if !any {
        ui.label(RichText::new("Nothing: the mission keeps its starting environment.").size(12.0).color(theme::text_dim()));
    }
    for (name, items) in groups.iter().filter(|g| !g.1.is_empty()) {
        row(ui, name, format!("{} node(s)", items.len()));
        for it in items.iter().take(4) {
            ui.label(RichText::new(format!("   {it}")).size(11.5).color(theme::text_dim()));
        }
    }
    if !l.sound_banks.items.is_empty() {
        heading(ui, "SOUND");
        row(ui, "Sound banks", format!("{}", l.sound_banks.items.len()));
    }
}
