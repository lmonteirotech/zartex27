//! Editable property rows for logic nodes and actors.
//! Draws each property of the selected node or actor in the inspector as an editable row: whole numbers, floats,
//! booleans, text, actor references, positions and vectors. Other value kinds are shown as read-only text.
//! Exposes show, which pushes the edits into a list; the list is applied by App::apply_pending (see model.rs).
//! Uses crate::kv and crate::theme (main.rs and theme.rs), crate::model (MissionView, Owner, Edit, string_of, actor_label)
//! and zartex_core::logic (the property types). Used by main.rs, in the inspector.

use crate::kv;
use crate::model::*;
use crate::theme;
use eframe::egui::{self, RichText};
use zartex_core::logic::{PropValue, Property};

/// One row: the property name on the left, the control on the right.
fn row(ui: &mut egui::Ui, name: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).color(theme::text_dim()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

/// Text field that only commits when it loses focus (each commit may add a string to the mission's string table).
fn text_row(ui: &mut egui::Ui, id: egui::Id, name: &str, current: &str) -> Option<String> {
    let mut out = None;
    row(ui, name, |ui| {
        // the text is kept in egui memory while it is typed; it is committed on losing focus, and only when it changed
        let mut buf: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
        let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(150.0));
        if r.changed() {
            ui.data_mut(|d| d.insert_temp(id, buf.clone()));
        }
        if r.lost_focus() {
            ui.data_mut(|d| d.remove_temp::<String>(id));
            if buf != current {
                out = Some(buf);
            }
        }
    });
    out
}

/// Draws the property rows of a node or actor. skip is the number of leading properties that are not shown (a node's first property is its wire collection index). Each change is pushed to pending.
pub fn show(ui: &mut egui::Ui, v: &MissionView, props: &[Property], skip: usize, owner: Owner, pending: &mut Vec<Edit>) {
    for (k, pr) in props.iter().enumerate().skip(skip) {
        let name = string_of(v, pr.string_id);
        let id = egui::Id::new(("prop", match owner { Owner::Node(n) => (0, n), Owner::Actor(a) => (1, a) }, k));
        // at most one edit per row
        let mut edit: Option<PropEdit> = None;
        match &pr.value {
            // property type 7 is a reference to an actor: a list of the actors, or none (-1)
            PropValue::Int(x) if pr.type_id == 7 => {
                // reference to an actor
                let mut cur = *x;
                row(ui, &name, |ui| {
                    let label = if cur < 0 { "none".to_string() } else { format!("{cur}: {}", actor_label(v, cur as usize)) };
                    egui::ComboBox::from_id_salt(id).width(150.0).selected_text(label).show_ui(ui, |ui| {
                        ui.selectable_value(&mut cur, -1, "none");
                        for i in 0..v.mission.logic.actors.definitions.len() {
                            ui.selectable_value(&mut cur, i as i32, format!("{i}: {}", actor_label(v, i)));
                        }
                    });
                });
                if cur != *x {
                    edit = Some(PropEdit::Int(cur));
                }
            }
            // whole numbers; property type 9 is shown in hexadecimal
            PropValue::Int(x) => {
                let mut cur = *x;
                row(ui, &name, |ui| {
                    let mut dv = egui::DragValue::new(&mut cur).speed(0.2);
                    if pr.type_id == 9 {
                        dv = dv.hexadecimal(4, false, true);
                    }
                    ui.add(dv);
                });
                if cur != *x {
                    edit = Some(PropEdit::Int(cur));
                }
            }
            PropValue::Float(x) => {
                let mut cur = *x;
                row(ui, &name, |ui| {
                    ui.add(egui::DragValue::new(&mut cur).speed(0.01).max_decimals(4));
                });
                if cur != *x {
                    edit = Some(PropEdit::Float(cur));
                }
            }
            PropValue::Bool(x) => {
                let mut cur = *x;
                row(ui, &name, |ui| {
                    ui.checkbox(&mut cur, "");
                });
                if cur != *x {
                    edit = Some(PropEdit::Bool(cur));
                }
            }
            PropValue::Str(s) => {
                if let Some(t) = text_row(ui, id, &name, &string_of(v, *s)) {
                    edit = Some(PropEdit::Text(t));
                }
            }
            PropValue::TextFile { value, .. } => {
                if let Some(t) = text_row(ui, id, &name, &string_of(v, *value)) {
                    edit = Some(PropEdit::Text(t));
                }
            }
            // a vector: x, y and z are edited; the fourth component is not shown
            PropValue::Vec4(p) => {
                let mut c = [p.x, p.y, p.z];
                let mut changed = false;
                row(ui, &name, |ui| {
                    for k in (0..3).rev() {
                        changed |= ui.add(egui::DragValue::new(&mut c[k]).speed(0.1).max_decimals(2)).changed();
                    }
                });
                if changed {
                    edit = Some(PropEdit::Vec3(c));
                }
            }
            // a transform: only its position is edited; the rotation part is not shown
            PropValue::Matrix { pos, .. } => {
                let mut c = [pos.x, pos.y, pos.z];
                let mut changed = false;
                row(ui, &format!("{name} (position)"), |ui| {
                    for k in (0..3).rev() {
                        changed |= ui.add(egui::DragValue::new(&mut c[k]).speed(0.1).max_decimals(2)).changed();
                    }
                });
                if changed {
                    edit = Some(PropEdit::Vec3(c));
                }
            }
            // every other kind of value is shown as read-only text
            _ => kv(ui, &name, prop_value_text(v, pr)),
        }
        // the change is queued; it is applied at the start of the next frame
        if let Some(e) = edit {
            pending.push(Edit::Prop(owner, k, e));
        }
    }
}
