//! View-model: a flat, UI-friendly snapshot of a mission loaded from .mpc / .sp / Lua.
//! Holds the open file (Project, with its container in Source and one MissionView per mission), the mission data the
//! panels read (MissionView), the reading helpers (labels, wires, the automatic graph layout, warnings), and the editing
//! model: the Edit values, Project::apply, Project::sync_to_source, and the delete operations.
//! Exposes the types and functions of this file. Uses zartex_core (mission, DPL package, logic, object, binary and node
//! name data) and crate::camera (set_camera_forward and set_float_prop, for the camera edits).
//! Used by main.rs (through use model::*), propedit.rs, overview.rs and camera.rs, and by the test programs
//! bin/edittest.rs and bin/camtest.rs, which compile this file with #[path].

use std::collections::HashMap;
use zartex_core::dpl::DplPackage;
use zartex_core::logic::{LogicCollection, NodeDef, PropValue, Property};
use zartex_core::mission::{ExportedMission, MissionFile, MissionSummary};
use zartex_core::nodenames;
use zartex_core::objects::MissionObject;

/// Which game a mission belongs to. It selects the node-name table and some labels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameKind {
    Driv3r,
    ParallelLines,
}

/// One mission ready for the UI: its title, its decoded data, its summary (Driv3r only) and its game.
#[derive(Clone, Debug)]
pub struct MissionView {
    pub title: String,
    pub mission: ExportedMission,
    pub summary: Option<MissionSummary>,
    pub kind: GameKind,
}

/// The container a project was read from. It is written back when the project is saved.
#[derive(Clone, Debug)]
pub enum Source {
    /// A Driv3r .mpc file: one mission.
    Mpc(Box<MissionFile>),
    /// A Parallel Lines .sp package: an init script and its sub-missions.
    Sp(Box<DplPackage>),
}

/// An open file: its path, its container, one view per mission, the index of the mission shown, and whether it has unsaved changes.
pub struct Project {
    pub path: String,
    pub source: Source,
    pub views: Vec<MissionView>,
    pub current: usize,
    pub dirty: bool,
}

impl Project {
    /// Reads a file from its bytes. A path ending in .sp is a Parallel Lines package (view 0 is the init script, then one view per sub-mission); any other path is a Driv3r mission.
    pub fn open_bytes(path: &str, bytes: &[u8]) -> Result<Project, String> {
        let name = std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if path.to_lowercase().ends_with(".sp") {
            let pkg = DplPackage::from_bytes(bytes).map_err(|e| e.to_string())?;
            let mut views = Vec::new();
            views.push(MissionView {
                title: "Init (auto-exec)".into(),
                mission: exported(&pkg.init),
                summary: None,
                kind: GameKind::ParallelLines,
            });
            for (sub, m) in &pkg.missions {
                views.push(MissionView {
                    title: format!("{} (sub {})", m.name.clone().unwrap_or_else(|| "Mission".into()), sub),
                    mission: exported(m),
                    summary: None,
                    kind: GameKind::ParallelLines,
                });
            }
            // a package opens on its first sub-mission when it has one
            let current = if views.len() > 1 { 1 } else { 0 };
            Ok(Project { path: path.into(), source: Source::Sp(Box::new(pkg)), views, current, dirty: false })
        } else {
            let mf = MissionFile::from_bytes(bytes).map_err(|e| e.to_string())?;
            let view = MissionView { title: name, mission: mf.mission.clone(), summary: mf.summary.clone(), kind: GameKind::Driv3r };
            Ok(Project { path: path.into(), source: Source::Mpc(Box::new(mf)), views: vec![view], current: 0, dirty: false })
        }
    }

    /// Builds a project from a Driv3r script compiled in memory. It is marked unsaved, because nothing has been written yet.
    pub fn from_compiled_mpc(label: &str, bytes: &[u8]) -> Result<Project, String> {
        let mut p = Project::open_bytes(&format!("{label}.mpc"), bytes)?;
        p.dirty = true;
        Ok(p)
    }

    /// Builds a project from a Parallel Lines script compiled in memory. It is marked unsaved.
    pub fn from_compiled_sp(label: &str, bytes: &[u8]) -> Result<Project, String> {
        let mut p = Project::open_bytes(&format!("{label}.sp"), bytes)?;
        p.dirty = true;
        Ok(p)
    }

    /// The mission shown now. The index is kept inside the range of the views.
    pub fn view(&self) -> &MissionView {
        &self.views[self.current.min(self.views.len() - 1)]
    }
}

/// Copies a Parallel Lines mission into the ExportedMission shape used by the UI (no instance list).
fn exported(m: &zartex_core::dpl::DplMission) -> ExportedMission {
    ExportedMission { objects: m.objects.clone(), instances: None, logic: m.logic.clone() }
}

// ---------------------------------------------------------------- derived helpers

/// Text of a string-table entry. A negative or unknown id gives #id.
pub fn string_of(mv: &MissionView, id: i16) -> String {
    mv.mission.logic.strings.strings.get(id.max(0) as usize).cloned().unwrap_or_else(|| format!("#{id}"))
}

/// Name of a logic node type. Parallel Lines missions use their own table. An unknown id gives Type N.
pub fn node_type_name(mv: &MissionView, type_id: u8) -> String {
    let table = if mv.kind == GameKind::ParallelLines { nodenames::LOGIC_NODE_TYPES_DPL } else { nodenames::LOGIC_NODE_TYPES };
    nodenames::lookup(table, type_id).map(|s| s.to_string()).unwrap_or_else(|| format!("Type {type_id}"))
}

/// Name of an actor type. An unknown id gives Actor N.
pub fn actor_type_name(type_id: u8) -> String {
    nodenames::lookup(nodenames::ACTOR_NODE_TYPES, type_id).map(|s| s.to_string()).unwrap_or_else(|| format!("Actor {type_id}"))
}

/// Label of a logic node: its note when it has one, otherwise its type name.
pub fn node_label(mv: &MissionView, idx: usize) -> String {
    let n = &mv.mission.logic.nodes.definitions[idx];
    let note = string_of(mv, n.string_id);
    if note.is_empty() || note == "Unknown" {
        node_type_name(mv, n.type_id)
    } else {
        note
    }
}

/// Label of an actor: its note when it has one, otherwise its type name.
pub fn actor_label(mv: &MissionView, idx: usize) -> String {
    let a = &mv.mission.logic.actors.definitions[idx];
    let note = string_of(mv, a.string_id);
    if note.is_empty() || note == "Unknown" {
        actor_type_name(a.type_id)
    } else {
        note
    }
}

/// Wire collection index of a node (its first property, `pWireCollection`).
pub fn wire_index(n: &NodeDef) -> Option<usize> {
    match n.properties.first() {
        Some(Property { type_id: 19, value: PropValue::Int(v), .. }) if *v >= 0 => Some(*v as usize),
        _ => None,
    }
}

/// A wire from one logic node to another, with its wire type number.
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub wire_type: u8,
}

/// All wires of a mission as Edge values. Each node names its wire collection in its first property. Wires that point past the last node are skipped.
pub fn edges(mv: &MissionView) -> Vec<Edge> {
    let l = &mv.mission.logic;
    let mut out = Vec::new();
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        if let Some(w) = wire_index(n) {
            if let Some(c) = l.wires.collections.get(w) {
                for x in &c.wires {
                    let to = x.node_id as usize;
                    if to < l.nodes.definitions.len() {
                        out.push(Edge { from: i, to, wire_type: x.wire_type });
                    }
                }
            }
        }
    }
    out
}

/// Left-to-right layered layout: BFS depth from LogicStart nodes (type 1), unreached nodes appended.
pub fn layout(mv: &MissionView) -> Vec<(f32, f32)> {
    let l = &mv.mission.logic;
    let n = l.nodes.definitions.len();
    let es = edges(mv);
    let mut depth: Vec<Option<usize>> = vec![None; n];
    let mut queue = std::collections::VecDeque::new();
    for (i, d) in l.nodes.definitions.iter().enumerate() {
        if d.type_id == 1 {
            depth[i] = Some(0);
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        for e in es.iter().filter(|e| e.from == i) {
            if depth[e.to].is_none() {
                depth[e.to] = Some(depth[i].unwrap() + 1);
                queue.push_back(e.to);
            }
        }
    }
    // nodes that no start node reaches go into one column after the last reached one
    let max_depth = depth.iter().flatten().max().copied().unwrap_or(0);
    for d in depth.iter_mut() {
        if d.is_none() {
            *d = Some(max_depth + 1);
        }
    }
    // nodes in the same column are stacked: x = depth * 212 px, y = row index * 74 px
    let mut rows: HashMap<usize, usize> = HashMap::new();
    let mut pos = vec![(0.0, 0.0); n];
    for i in 0..n {
        let d = depth[i].unwrap();
        let r = rows.entry(d).or_insert(0);
        pos[i] = (d as f32 * 212.0, *r as f32 * 74.0);
        *r += 1;
    }
    pos
}

/// Short text of a property value for the inspector.
pub fn prop_value_text(mv: &MissionView, p: &Property) -> String {
    match &p.value {
        PropValue::Int(v) => v.to_string(),
        PropValue::Float(v) => format!("{v:.3}"),
        PropValue::Str(s) => format!("\"{}\"", string_of(mv, *s)),
        PropValue::Bool(b) => b.to_string(),
        PropValue::TextFile { value, index } => format!("\"{}\" / {index}", string_of(mv, *value)),
        PropValue::Uid(u) => format!("{u:#x}"),
        PropValue::Audio { bank, sample } => format!("bank {bank}, sample {sample}"),
        PropValue::Vec4(v) => format!("({:.2}, {:.2}, {:.2}, {:.2})", v.x, v.y, v.z, v.w),
        PropValue::Matrix { pos, .. } => format!("pos ({:.2}, {:.2}, {:.2})", pos.x, pos.y, pos.z),
        PropValue::Path(p) => format!("{} points", p.len()),
        PropValue::Unicode(u) => String::from_utf16_lossy(u),
        PropValue::Raw(b) => format!("{} bytes", b.len()),
        PropValue::Assets { assets, .. } => format!("{} assets", assets.len()),
    }
}

/// Short label of a map object for the object list.
pub fn object_label(o: &MissionObject) -> String {
    match o {
        MissionObject::Vehicle { position, .. } => format!("Vehicle ({:.0}, {:.0}, {:.0})", position.x, position.y, position.z),
        MissionObject::Character { position, .. } => format!("Character ({:.0}, {:.0}, {:.0})", position.x, position.y, position.z),
        MissionObject::Volume { .. } => "Volume".into(),
        MissionObject::Area { .. } => "Area".into(),
        MissionObject::Path { path, .. } => format!("Path ({} points)", path.len()),
        MissionObject::ObjectiveIcon { .. } => "Objective icon".into(),
        MissionObject::Camera { .. } => "Camera".into(),
        MissionObject::Prop { .. } => "Prop".into(),
        MissionObject::Switch { .. } => "Switch".into(),
        MissionObject::Collectable { .. } => "Collectable".into(),
        MissionObject::AnimProp { .. } => "Animated prop".into(),
        MissionObject::Marker { .. } => "Marker".into(),
    }
}

/// Warnings for the Warnings tab: every logic node that is not a start node (type 1) and has no incoming wire.
pub fn warnings(mv: &MissionView) -> Vec<String> {
    let l = &mv.mission.logic;
    let mut w = Vec::new();
    for (i, n) in l.nodes.definitions.iter().enumerate() {
        if n.type_id != 1 {
            let has_out = wire_index(n).and_then(|x| l.wires.collections.get(x)).map(|c| !c.wires.is_empty()).unwrap_or(false);
            let has_in = edges(mv).iter().any(|e| e.to == i);
            if !has_in {
                w.push(format!("{} has no incoming wire", node_label(mv, i)));
            }
            let _ = has_out;
        }
    }
    w
}

// keeps the LogicCollection import in use; does nothing
#[allow(dead_code)]
fn _lc(_: &LogicCollection) {}

// How editing works. The panels never change the mission while they draw. Each change is pushed as an Edit into the
// pending list (App::pending). At the start of the next frame the list is applied with Project::apply, which changes the
// current mission and sets dirty only when something changed. Project::sync_to_source copies the edited views back into
// the container before a save. Edits refer to positions in the lists, so deleting a node or an actor renumbers the rest;
// the app then clears the selection (see apply_pending in main.rs).
// ---------------------------------------------------------------- editing

/// Which list a property belongs to: a logic node or an actor, with its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// A logic node.
    Node(usize),
    /// An actor.
    Actor(usize),
}

/// A new value for one property. Its kind must match the property's kind, or the edit is refused.
#[derive(Clone, Debug)]
pub enum PropEdit {
    Int(i32),
    Float(f32),
    Bool(bool),
    /// New text. It is appended to the string table and the property points at it; the old string stays.
    Text(String),
    Vec3([f32; 3]),
}

/// A new value for one field of the mission summary (Driv3r). Each variant sets one field.
#[derive(Clone, Debug)]
pub enum SummaryEdit {
    MoodId(i16),
    City(i32),
    ParkedDensity(i32),
    AttractorDensity(i32),
    PingIn(f64),
    PingOut(f64),
    DensityOverride(bool),
}

/// One requested change to the open mission. Panels create these; Project::apply carries them out.
#[derive(Clone, Debug)]
pub enum Edit {
    /// Removes a logic node (delete_node).
    DeleteNode(usize),
    /// Removes an actor (delete_actor).
    DeleteActor(usize),
    /// Adds a wire from the first node to the second node, of the given wire type.
    WireAdd(usize, usize, u8),
    /// Removes wire number k from the wires of a node.
    WireRemove(usize, usize),
    /// Changes the wire type of wire k of a node.
    WireType(usize, usize, u8),
    /// Changes property k of a node or an actor.
    Prop(Owner, usize, PropEdit),
    /// Changes one field of the mission summary.
    Summary(SummaryEdit),
    /// Moves a map object so that its anchor point is at the position.
    ObjectPos(usize, [f32; 3]),
    /// Moves an actor (the position of its Matrix property).
    ActorPos(usize, [f32; 3]),
    /// Aims a Camera object along a direction.
    CameraForward(usize, [f32; 3]),
    /// Sets a float property of a logic node, found by its name.
    NodeFloat(usize, &'static str, f32),
    /// Sets a float property of an actor, found by its name.
    ActorFloat(usize, &'static str, f32),
    /// Moves the player start: x and z of the summary.
    StartPos(f32, f32),
}

/// Anchor point of a map object, or None when it has no position (a Prop).
pub fn object_position(o: &MissionObject) -> Option<[f32; 3]> {
    use MissionObject::*;
    Some(match o {
        Vehicle { position, .. } | Character { position, .. } | Volume { position, .. } | ObjectiveIcon { position, .. } => [position.x, position.y, position.z],
        Camera { position, .. } | Switch { position, .. } | Collectable { position, .. } | AnimProp { position } | Marker { position, .. } => [position.x, position.y, position.z],
        Area { position, .. } => [position.x, position.y, position.z],
        Path { path, .. } => path.first().map(|p| [p.x, p.y, p.z])?,
        Prop { .. } => return None,
    })
}

/// Moves an object so that its anchor point is at `p`. Paths move as a whole.
pub fn set_object_position(o: &mut MissionObject, p: [f32; 3]) -> bool {
    use MissionObject::*;
    let set3 = |v: &mut zartex_core::binio::Vec3| {
        v.x = p[0];
        v.y = p[1];
        v.z = p[2];
    };
    match o {
        Vehicle { position, .. } | Character { position, .. } | Volume { position, .. } | ObjectiveIcon { position, .. } => set3(position),
        Camera { position, .. } | Switch { position, .. } | Collectable { position, .. } | AnimProp { position } | Marker { position, .. } => set3(position),
        Area { position, .. } => {
            position.x = p[0];
            position.y = p[1];
            position.z = p[2];
        }
        Path { path, .. } => {
            let Some(f) = path.first().copied() else { return false };
            let d = [p[0] - f.x, p[1] - f.y, p[2] - f.z];
            for q in path.iter_mut() {
                q.x += d[0];
                q.y += d[1];
                q.z += d[2];
            }
        }
        Prop { .. } => return false,
    }
    true
}

impl Project {
    /// Carries out one edit on the current mission. Sets dirty when the mission changed. An edit whose index or value kind does not fit changes nothing.
    pub fn apply(&mut self, e: Edit) {
        let cur = self.current.min(self.views.len() - 1);
        let v = &mut self.views[cur];
        // each arm gives true when the mission changed, false when the index or the value kind did not fit
        let changed = match e {
            Edit::ObjectPos(i, p) => v.mission.objects.objects.get_mut(i).map(|o| set_object_position(o, p)).unwrap_or(false),
            Edit::ActorPos(i, p) => match v.mission.logic.actors.definitions.get_mut(i).and_then(|a| a.properties.iter_mut().find_map(|pr| if let PropValue::Matrix { pos, .. } = &mut pr.value { Some(pos) } else { None })) {
                Some(pos) => {
                    pos.x = p[0];
                    pos.y = p[1];
                    pos.z = p[2];
                    true
                }
                None => false,
            },
            Edit::CameraForward(i, f) => v.mission.objects.objects.get_mut(i).map(|o| crate::camera::set_camera_forward(o, f)).unwrap_or(false),
            Edit::NodeFloat(n, name, val) => crate::camera::set_float_prop(v, Some(n), None, name, val),
            Edit::ActorFloat(a, name, val) => crate::camera::set_float_prop(v, None, Some(a), name, val),
            Edit::DeleteNode(n) => delete_node(v, n),
            Edit::DeleteActor(a) => delete_actor(v, a),
            // the wire is added to the source node's wire collection; its op code is the target node's type
            Edit::WireAdd(node, target, wtype) => {
                let l = &mut v.mission.logic;
                let op = l.nodes.definitions.get(target).map(|d| d.type_id);
                let w = l.nodes.definitions.get(node).and_then(wire_index);
                match (op, w.and_then(|w| l.wires.collections.get_mut(w))) {
                    (Some(op), Some(c)) => {
                        c.wires.push(zartex_core::logic::WireNode { wire_type: wtype, op_code: op, node_id: target as i16 });
                        true
                    }
                    _ => false,
                }
            }
            Edit::WireRemove(node, k) => {
                let l = &mut v.mission.logic;
                let w = l.nodes.definitions.get(node).and_then(wire_index);
                match w.and_then(|w| l.wires.collections.get_mut(w)) {
                    Some(c) if k < c.wires.len() => {
                        c.wires.remove(k);
                        true
                    }
                    _ => false,
                }
            }
            Edit::WireType(node, k, t) => {
                let l = &mut v.mission.logic;
                let w = l.nodes.definitions.get(node).and_then(wire_index);
                match w.and_then(|w| l.wires.collections.get_mut(w)).and_then(|c| c.wires.get_mut(k)) {
                    Some(x) => {
                        x.wire_type = t;
                        true
                    }
                    None => false,
                }
            }
            // a value is stored only when its new kind is the same as the property's kind
            Edit::Prop(owner, k, pe) => {
                let l = &mut v.mission.logic;
                let props = match owner {
                    Owner::Node(n) => l.nodes.definitions.get_mut(n).map(|d| &mut d.properties),
                    Owner::Actor(a) => l.actors.definitions.get_mut(a).map(|d| &mut d.properties),
                };
                match props.and_then(|p| p.get_mut(k)) {
                    Some(pr) => match (&mut pr.value, pe) {
                        (PropValue::Int(x), PropEdit::Int(n)) => {
                            *x = n;
                            true
                        }
                        (PropValue::Float(x), PropEdit::Float(n)) => {
                            *x = n;
                            true
                        }
                        (PropValue::Bool(x), PropEdit::Bool(n)) => {
                            *x = n;
                            true
                        }
                        (PropValue::Str(id), PropEdit::Text(t)) => {
                            *id = l.strings.append_string(&t) as i16;
                            true
                        }
                        (PropValue::TextFile { value, .. }, PropEdit::Text(t)) => {
                            *value = l.strings.append_string(&t) as i16;
                            true
                        }
                        (PropValue::Vec4(p), PropEdit::Vec3(c)) => {
                            p.x = c[0];
                            p.y = c[1];
                            p.z = c[2];
                            true
                        }
                        (PropValue::Matrix { pos, .. }, PropEdit::Vec3(c)) => {
                            pos.x = c[0];
                            pos.y = c[1];
                            pos.z = c[2];
                            true
                        }
                        _ => false,
                    },
                    None => false,
                }
            }
            // summary fields exist for Driv3r missions only
            Edit::Summary(se) => match &mut v.summary {
                Some(s) => {
                    match se {
                        SummaryEdit::MoodId(x) => s.mission_id = x,
                        SummaryEdit::City(x) => s.city_type = x,
                        SummaryEdit::ParkedDensity(x) => s.parked_car_density = x,
                        SummaryEdit::AttractorDensity(x) => s.attractor_parked_car_density = x,
                        SummaryEdit::PingIn(x) => s.ping_in_radius = x,
                        SummaryEdit::PingOut(x) => s.ping_out_radius = x,
                        SummaryEdit::DensityOverride(x) => s.density_override = x,
                    }
                    true
                }
                None => false,
            },
            Edit::StartPos(x, z) => match &mut v.summary {
                Some(s) => {
                    s.start_position.x = x;
                    s.start_position.y = z;
                    true
                }
                None => false,
            },
        };
        // the project is marked unsaved only by a real change
        if changed {
            self.dirty = true;
        }
    }

    /// Writes the edited views back into the container that is saved.
    pub fn sync_to_source(&mut self) {
        match &mut self.source {
            // a .mpc has one view: its mission and summary are written back
            Source::Mpc(mf) => {
                if let Some(v) = self.views.first() {
                    mf.mission = v.mission.clone();
                    mf.summary = v.summary.clone();
                }
            }
            // a .sp: view 0 is the init script, views 1 and up are the sub-missions in package order
            Source::Sp(pkg) => {
                if let Some(v) = self.views.first() {
                    pkg.init.objects = v.mission.objects.clone();
                    pkg.init.logic = v.mission.logic.clone();
                }
                for (k, (_, m)) in pkg.missions.iter_mut().enumerate() {
                    if let Some(v) = self.views.get(k + 1) {
                        m.objects = v.mission.objects.clone();
                        m.logic = v.mission.logic.clone();
                    }
                }
            }
        }
    }
}

/// Position of an actor (the `pos` of its first Matrix property), if it has one.
pub fn actor_position(a: &NodeDef) -> Option<[f32; 3]> {
    a.properties.iter().find_map(|pr| if let PropValue::Matrix { pos, .. } = &pr.value { Some([pos.x, pos.y, pos.z]) } else { None })
}

/// Actors drawn as map objects (same index as the actor; actors without a position become inert placeholders).
pub fn actors_as_objects(mv: &MissionView) -> Vec<MissionObject> {
    use zartex_core::binio::Vec3 as V3;
    mv.mission
        .logic
        .actors
        .definitions
        .iter()
        .map(|a| {
            let Some(p) = actor_position(a) else { return MissionObject::Prop { id: 0, unk: [0; 5] } };
            let position = V3 { x: p[0], y: p[1], z: p[2] };
            let name = actor_type_name(a.type_id).to_lowercase();
            if name.contains("vehicle") || name.contains("car") || name.contains("bike") || name.contains("boat") {
                MissionObject::Vehicle { creation_data: vec![], position, uid: 0 }
            } else if name.contains("character") || name.contains("cop") || name.contains("ped") {
                MissionObject::Character { creation_data: vec![], position, skin_id: 0, uid: 0 }
            } else if name.contains("volume") {
                MissionObject::Volume { reserved: 0, position }
            } else {
                MissionObject::Marker { v1: position, position }
            }
        })
        .collect()
}

// ---------------------------------------------------------------- delete with renumbering

/// Wire collection index of a node from its first property (type 19), or None.
fn wire_prop(d: &NodeDef) -> Option<usize> {
    match d.properties.first() {
        Some(Property { type_id: 19, value: PropValue::Int(w), .. }) if *w >= 0 => Some(*w as usize),
        _ => None,
    }
}

/// Removes a logic node: wires into it go, later node numbers shift down, and its wire collection is dropped.
pub fn delete_node(v: &mut MissionView, n: usize) -> bool {
    let l = &mut v.mission.logic;
    if n >= l.nodes.definitions.len() {
        return false;
    }
    // the node's own wire collection is remembered before the node is removed
    let wire = wire_prop(&l.nodes.definitions[n]);
    l.nodes.definitions.remove(n);
    for c in l.wires.collections.iter_mut() {
        // wires into the deleted node are removed; wires to later nodes move down by one
        c.wires.retain(|x| x.node_id < 0 || x.node_id as usize != n);
        for x in c.wires.iter_mut() {
            if x.node_id >= 0 && (x.node_id as usize) > n {
                x.node_id -= 1;
            }
        }
    }
    // the node's wire collection is removed too; the nodes that point at later collections are renumbered
    if let Some(w) = wire {
        if w < l.wires.collections.len() {
            l.wires.collections.remove(w);
            for d in l.nodes.definitions.iter_mut() {
                if let Some(Property { type_id: 19, value: PropValue::Int(x), .. }) = d.properties.first_mut() {
                    if *x >= 0 && (*x as usize) > w {
                        *x -= 1;
                    }
                }
            }
        }
    }
    true
}

/// True when a map object is the kind that an actor of the given type name would have (matched by name, for example a Vehicle object for a vehicle actor).
fn object_kind_matches(actor_type: &str, o: &MissionObject) -> bool {
    let t = actor_type.to_lowercase();
    match o {
        MissionObject::Vehicle { .. } => t.contains("vehicle"),
        MissionObject::Character { .. } => t.contains("character") || t.contains("cop") || t.contains("ped"),
        MissionObject::Volume { .. } => t.contains("volume"),
        MissionObject::Area { .. } => t.contains("area"),
        MissionObject::Path { .. } => t.contains("path"),
        MissionObject::Camera { .. } => t.contains("camera"),
        MissionObject::AnimProp { .. } => t.contains("anim"),
        MissionObject::Marker { .. } => t.contains("marker"),
        MissionObject::Switch { .. } => t.contains("switch"),
        MissionObject::Collectable { .. } => t.contains("collect"),
        MissionObject::ObjectiveIcon { .. } => t.contains("icon") || t.contains("objective"),
        MissionObject::Prop { .. } => t.contains("prop"),
    }
}

/// Removes an actor (and its map object when it clearly belongs to it): references to it become "none" (-1), later
/// actor numbers shift down.
pub fn delete_actor(v: &mut MissionView, a: usize) -> bool {
    if a >= v.mission.logic.actors.definitions.len() {
        return false;
    }
    let tname = actor_type_name(v.mission.logic.actors.definitions[a].type_id);
    let obj = v.mission.logic.actors.definitions[a].object_id;
    let shared = v.mission.logic.actors.definitions.iter().enumerate().any(|(i, d)| i != a && d.object_id == obj);
    // the actor's map object is removed only when no other actor uses it and its kind matches the actor type
    let delete_obj = obj >= 0 && !shared && v.mission.objects.objects.get(obj as usize).map(|o| object_kind_matches(&tname, o)).unwrap_or(false);
    let l = &mut v.mission.logic;
    l.actors.definitions.remove(a);
    // references to the actor (property type 7) become -1 (none); references to later actors move down by one
    let fix = |defs: &mut Vec<NodeDef>| {
        for d in defs.iter_mut() {
            for p in d.properties.iter_mut() {
                if p.type_id == 7 {
                    if let PropValue::Int(x) = &mut p.value {
                        if *x == a as i32 {
                            *x = -1;
                        } else if *x > a as i32 {
                            *x -= 1;
                        }
                    }
                }
            }
        }
    };
    fix(&mut l.actors.definitions);
    fix(&mut l.nodes.definitions);
    // actors that point at a later map object get its index reduced by one
    if delete_obj {
        v.mission.objects.objects.remove(obj as usize);
        for d in v.mission.logic.actors.definitions.iter_mut() {
            if d.object_id > obj {
                d.object_id -= 1;
            }
        }
    }
    true
}
