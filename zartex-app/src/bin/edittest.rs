//! Checks that edits survive a save and reload, and that the real Parallel Lines packages open.
//!
//! usage: edittest <mission file (.mpc or Parallel Lines package)> [edit]
//! Without `edit` it saves the file unchanged and reports whether the bytes and the parsed content are the same. With `edit`
//! it moves an object and the start point, changes a float and a text property, adds and removes wires, changes the summary,
//! saves, reloads, and prints what changed. Uses `zartex_core::logic` through the app's `model.rs`.
//! Includes `model.rs` and `camera.rs` through `#[path]` (the same code as the app).
#[path = "../model.rs"]
mod model;
#[path = "../camera.rs"]
mod camera;
use model::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1]).unwrap();
    let mut p = Project::open_bytes(&a[1], &bytes).unwrap();
    println!("opened {} views: {:?}", p.views.len(), p.views.iter().take(4).map(|v| v.title.clone()).collect::<Vec<_>>());
    if a.get(2).map(|s| s.as_str()) == Some("edit") {
        let before = p.view().mission.objects.objects.iter().position(|o| object_position(o).is_some()).unwrap();
        let old = object_position(&p.view().mission.objects.objects[before]).unwrap();
        p.apply(Edit::ObjectPos(before, [old[0] + 12.5, old[1] + 1.0, old[2] - 7.25]));
        p.apply(Edit::StartPos(1111.5, 222.25));
        // property and summary edits: first float property of the first node that has one, a new text, mood and city
        let mut float_edit = None;
        'outer: for (ni, n) in p.view().mission.logic.nodes.definitions.iter().enumerate() {
            for (k, pr) in n.properties.iter().enumerate() {
                if let zartex_core::logic::PropValue::Float(f) = pr.value { float_edit = Some((ni, k, f)); break 'outer; }
            }
        }
        let text_edit = p.view().mission.logic.nodes.definitions.iter().enumerate().find_map(|(ni, n)| n.properties.iter().position(|pr| matches!(pr.value, zartex_core::logic::PropValue::Str(_))).map(|k| (ni, k)));
        if let Some((ni, k, f)) = float_edit { p.apply(Edit::Prop(Owner::Node(ni), k, PropEdit::Float(f + 1.25))); }
        if let Some((ni, k)) = text_edit { p.apply(Edit::Prop(Owner::Node(ni), k, PropEdit::Text("edited text".into()))); }
        let before_edges = edges(p.view()).len();
        p.apply(Edit::WireAdd(0, 1, 3));
        p.apply(Edit::WireAdd(0, 2, 1));
        p.apply(Edit::WireRemove(0, 0));
        let expected_edges = edges(p.view()).len();
        println!("edges {before_edges} -> {expected_edges}");
        p.apply(Edit::Summary(SummaryEdit::MoodId(9)));
        p.apply(Edit::Summary(SummaryEdit::City(3)));
        p.apply(Edit::Summary(SummaryEdit::PingIn(77.0)));
        let fe = float_edit; let te = text_edit;
        p.sync_to_source();
        let out = match &mut p.source {
            Source::Mpc(mf) => mf.to_bytes().unwrap(),
            Source::Sp(pkg) => pkg.to_bytes().unwrap(),
        };
        let q = Project::open_bytes(&a[1], &out).unwrap();
        let now = object_position(&q.view().mission.objects.objects[before]).unwrap();
        println!("object {before}: {:?} -> {:?}", old, now);
        println!("start: {:?}", q.view().summary.as_ref().map(|s| (s.start_position.x, s.start_position.y)));
        if let Some((ni, k, f)) = fe { if let zartex_core::logic::PropValue::Float(g) = q.view().mission.logic.nodes.definitions[ni].properties[k].value { println!("float prop node {ni}[{k}]: {f} -> {g}"); } }
        if let Some((ni, k)) = te { if let zartex_core::logic::PropValue::Str(id) = q.view().mission.logic.nodes.definitions[ni].properties[k].value { println!("text prop node {ni}[{k}] -> {:?}", string_of(q.view(), id)); } }
        println!("edges after reload: {}", edges(q.view()).len());
        let sm = q.view().summary.as_ref().unwrap();
        println!("summary mood {} city {} ping_in {}", sm.mission_id, sm.city_type, sm.ping_in_radius);
        println!("size {} -> {}", bytes.len(), out.len());
    } else {
        let out = match &mut p.source {
            Source::Mpc(mf) => mf.to_bytes().unwrap(),
            Source::Sp(pkg) => pkg.to_bytes().unwrap(),
        };
        let q = Project::open_bytes(&a[1], &out).unwrap(); let same = format!("{:?}", q.view().mission) == format!("{:?}", p.view().mission) && format!("{:?}", q.view().summary) == format!("{:?}", p.view().summary); println!("unedited: bytes identical {}, parsed content identical {}", out == bytes, same);
    }
}
