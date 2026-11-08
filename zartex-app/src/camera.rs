//! Cutscene cameras: the shot sequence (`CameraSelect` nodes), camera poses, blending and aim editing.
//! It works out the order of the shots from the CameraSelect chain, the pose of each camera (a Camera object in Driv3r,
//! a Matrix property in Parallel Lines), and the pose at any time (pose_at), blended from the previous shot.
//! Preview: main.rs advances the playback time while the Cameras tab plays; pose_at gives the camera for that time and
//! the City 3D view draws it. The shots are also passed to the 3D view as a path of points, one per shot.
//! It also holds the edits of camera data: the aim (set_camera_forward) and float properties by name (set_float_prop).
//! Exposes Pose, Shot, prop_float, prop_int, camera_pose, shots, total_time, pose_at, yaw_pitch, dir_from_yaw_pitch,
//! set_camera_forward, set_float_prop and animations. Uses crate::model (MissionView helpers, edges), zartex_core (logic
//! and object data) and glam (quaternions and vectors). Used by main.rs (cameras tab, timeline, preview, aim controls),
//! by model.rs (the edits), by view3d.rs (the Pose type), and by the test programs bin/camtest.rs and bin/edittest.rs,
//! which compile this file with #[path].

use crate::model::*;
use zartex_core::logic::{PropValue, Property};
use zartex_core::objects::MissionObject;

/// Camera pose in world space: position, forward and up directions, and the vertical field of view.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub pos: [f32; 3],
    pub fwd: [f32; 3],
    pub up: [f32; 3],
    /// vertical field of view in degrees
    pub fov: f32,
}

/// One shot of the cutscene, in playback order.
#[derive(Clone, Debug)]
pub struct Shot {
    pub node: usize,
    pub actor: usize,
    pub duration: f32,
    pub blend: f32,
    pub zoom: f32,
    pub start: f32,
    pub pose: Pose,
    /// Index of the Camera object (Driv3r) the pose comes from.
    pub object: Option<usize>,
}

/// Reads a float property by its name (looked up in the string table). Whole-number properties are converted. None when the name or the kind does not match.
pub fn prop_float(v: &MissionView, props: &[Property], name: &str) -> Option<f32> {
    props.iter().find(|p| string_of(v, p.string_id) == name).and_then(|p| match p.value {
        PropValue::Float(f) => Some(f),
        PropValue::Int(i) => Some(i as f32),
        _ => None,
    })
}

/// Reads a whole-number property by its name. None when it is missing or is not a whole number.
pub fn prop_int(v: &MissionView, props: &[Property], name: &str) -> Option<i32> {
    props.iter().find(|p| string_of(v, p.string_id) == name).and_then(|p| match p.value {
        PropValue::Int(i) => Some(i),
        _ => None,
    })
}

/// Converts a vector of the zartex-core format into an array.
fn v3(v: &zartex_core::binio::Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

/// Pose of a Camera actor: from its Camera object (Driv3r) or from its Matrix property (Parallel Lines).
pub fn camera_pose(v: &MissionView, actor: usize) -> Option<(Pose, Option<usize>)> {
    let a = v.mission.logic.actors.definitions.get(actor)?;
    // zoom 1 is the game's own view; the vertical field of view is 2 * atan(tan(27.5 degrees) / zoom)
    let zoom = prop_float(v, &a.properties, "pCameraZoom").unwrap_or(1.0).max(0.05);
    // the game's own vertical field of view is 55 degrees at zoom 1
    let fov = (2.0 * ((27.5f32).to_radians().tan() / zoom).atan()).to_degrees();
    // Parallel Lines: the camera is the actor's Matrix property; forward is negated as for Driv3r objects
    if let Some(PropValue::Matrix { up, forward, pos, .. }) = a.properties.iter().map(|p| &p.value).find(|p| matches!(p, PropValue::Matrix { .. })) {
        return Some((Pose { pos: [pos.x, pos.y, pos.z], fwd: [-forward.x, -forward.y, -forward.z], up: [up.x, up.y, up.z], fov }, None));
    }
    // Driv3r: the camera is the Camera object the actor points to
    let oi = a.object_id.max(0) as usize;
    match v.mission.objects.objects.get(oi)? {
        // a camera object stores right/up/forward with forward pointing BACKWARDS: the camera looks along -forward
        // (checked on every camera with a pLookAt target: -forward points at it, +forward away from it)
        MissionObject::Camera { up, forward, position, .. } => Some((Pose { pos: v3(position), fwd: [-forward.x, -forward.y, -forward.z], up: v3(up), fov }, Some(oi))),
        _ => None,
    }
}

/// The camera sequence of the mission, in the order the `CameraSelect` nodes trigger each other (an estimate: timers
/// and branches in the script can change the real order).
/// The shot sequence of a mission, in playback order, with the start time of each shot. Timers and branches in the script are not modelled.
pub fn shots(v: &MissionView) -> Vec<Shot> {
    let l = &v.mission.logic;
    // the CameraSelect nodes are the shots; a wire from one to another gives the order
    let is_cam: Vec<bool> = l.nodes.definitions.iter().map(|n| node_type_name(v, n.type_id) == "CameraSelect").collect();
    let es = edges(v);
    let succ = |i: usize| es.iter().find(|e| e.from == i && is_cam[e.to] && e.to != i).map(|e| e.to);
    let has_pred = |i: usize| es.iter().any(|e| e.to == i && is_cam[e.from] && e.from != i);
    // the chain is followed from each shot that nothing points at; any shot not reached this way is then taken in node order
    let mut seen = vec![false; is_cam.len()];
    let mut order = Vec::new();
    for s in (0..is_cam.len()).filter(|&i| is_cam[i] && !has_pred(i)).chain((0..is_cam.len()).filter(|&i| is_cam[i])) {
        let mut cur = Some(s);
        while let Some(i) = cur {
            if seen[i] {
                break;
            }
            seen[i] = true;
            order.push(i);
            cur = succ(i);
        }
    }
    let mut t = 0.0;
    let mut out = Vec::new();
    for i in order {
        let n = &l.nodes.definitions[i];
        // a shot needs a target (pTarget) that is an actor with a camera pose; other nodes are left out
        let Some(actor) = prop_int(v, &n.properties, "pTarget").filter(|a| *a >= 0).map(|a| a as usize) else { continue };
        let Some((pose, object)) = camera_pose(v, actor) else { continue };
        // the node's pDuration and pBlendTime are used; the defaults are 2 s and no blend
        let duration = prop_float(v, &n.properties, "pDuration").unwrap_or(2.0).max(0.0);
        let blend = prop_float(v, &n.properties, "pBlendTime").unwrap_or(0.0).max(0.0);
        let zoom = l.actors.definitions.get(actor).and_then(|a| prop_float(v, &a.properties, "pCameraZoom")).unwrap_or(1.0);
        // each shot starts when the previous one ends
        out.push(Shot { node: i, actor, duration, blend, zoom, start: t, pose, object });
        t += duration;
    }
    out
}

/// Length of the sequence in seconds: the end of the last shot, or 0 when there are no shots.
pub fn total_time(shots: &[Shot]) -> f32 {
    shots.last().map(|s| s.start + s.duration).unwrap_or(0.0)
}

/// Linear interpolation between two vectors: t = 0 gives a, t = 1 gives b.
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Rotation of a pose as a quaternion. The axes are right (forward x up), up, and -forward, so that the camera looks down its own -Z axis.
fn quat_of(p: &Pose) -> glam::Quat {
    let f = glam::Vec3::from(p.fwd).normalize_or_zero();
    let u = glam::Vec3::from(p.up).normalize_or_zero();
    let r = f.cross(u).normalize_or_zero();
    let u2 = r.cross(f);
    glam::Quat::from_mat3(&glam::Mat3::from_cols(r, u2, -f)).normalize()
}

/// Camera at time `t` of the sequence: each shot blends in from the previous one during its blend time.
pub fn pose_at(shots: &[Shot], t: f32) -> Option<Pose> {
    // the shot that started last at time t; before the first shot, the first one is used
    let k = shots.iter().rposition(|s| s.start <= t).unwrap_or(0);
    let s = shots.get(k)?;
    // during the blend time of a shot (not the first one) the pose moves from the previous shot
    if k > 0 && s.blend > 0.0 && t - s.start < s.blend {
        let prev = &shots[k - 1].pose;
        let u = ((t - s.start) / s.blend).clamp(0.0, 1.0);
        // smoothstep: the blend starts and ends slowly
        let u = u * u * (3.0 - 2.0 * u);
        // rotation is spherically interpolated; position and field of view are interpolated linearly
        let q = quat_of(prev).slerp(quat_of(&s.pose), u);
        let f = q * -glam::Vec3::Z;
        let up = q * glam::Vec3::Y;
        return Some(Pose { pos: lerp3(prev.pos, s.pose.pos, u), fwd: f.to_array(), up: up.to_array(), fov: prev.fov + (s.pose.fov - prev.fov) * u });
    }
    // after the blend time the shot's own pose is used
    Some(s.pose)
}

/// Heading (yaw, around the vertical axis) and tilt (pitch) of a direction, in degrees. Yaw 0 is along +Z.
pub fn yaw_pitch(f: [f32; 3]) -> (f32, f32) {
    let v = glam::Vec3::from(f).normalize_or_zero();
    (v.x.atan2(v.z).to_degrees(), v.y.clamp(-1.0, 1.0).asin().to_degrees())
}

/// Unit direction from a heading and a tilt in degrees. The inverse of yaw_pitch.
pub fn dir_from_yaw_pitch(yaw: f32, pitch: f32) -> [f32; 3] {
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    [y.sin() * p.cos(), p.sin(), y.cos() * p.cos()]
}

/// Aims a Camera object along `look` (no roll: right stays horizontal). The stored forward is the opposite vector.
pub fn set_camera_forward(o: &mut MissionObject, look: [f32; 3]) -> bool {
    let MissionObject::Camera { right, up, forward, .. } = o else { return false };
    let look = glam::Vec3::from(look).normalize_or_zero();
    if look == glam::Vec3::ZERO {
        return false;
    }
    // right: horizontal, from the look direction and world up; looking straight up or down, X is used
    let mut r = look.cross(glam::Vec3::Y);
    if r.length() < 1e-4 {
        r = glam::Vec3::X;
    }
    let r = r.normalize();
    // up completes the frame; the stored forward is the negated look direction
    let u = r.cross(look);
    let f = -look;
    *forward = zartex_core::binio::Vec3 { x: f.x, y: f.y, z: f.z };
    *right = zartex_core::binio::Vec3 { x: r.x, y: r.y, z: r.z };
    *up = zartex_core::binio::Vec3 { x: u.x, y: u.y, z: u.z };
    true
}

/// Sets a float property of a node (when node is Some) or of an actor (when actor is Some) by its name. False when there is no such float property.
pub fn set_float_prop(v: &mut MissionView, node: Option<usize>, actor: Option<usize>, name: &str, value: f32) -> bool {
    // the property is found by comparing its name with the string table
    let strings = v.mission.logic.strings.strings.clone();
    let hit = |p: &Property| strings.get(p.string_id.max(0) as usize).map(|s| s == name).unwrap_or(false);
    let props = if let Some(n) = node {
        v.mission.logic.nodes.definitions.get_mut(n).map(|d| &mut d.properties)
    } else {
        actor.and_then(|a| v.mission.logic.actors.definitions.get_mut(a)).map(|d| &mut d.properties)
    };
    let Some(props) = props else { return false };
    for p in props.iter_mut() {
        if hit(p) {
            if let PropValue::Float(f) = &mut p.value {
                *f = value;
                return true;
            }
        }
    }
    false
}

/// Animation actors and the nodes that play them.
pub fn animations(v: &MissionView) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, a) in v.mission.logic.actors.definitions.iter().enumerate() {
        if actor_type_name(a.type_id).contains("Anim") {
            let ctrl: Vec<String> = v
                .mission
                .logic
                .nodes
                .definitions
                .iter()
                .enumerate()
                .filter(|(_, n)| node_type_name(v, n.type_id).contains("Animation") && prop_int(v, &n.properties, "pActor") == Some(i as i32))
                .map(|(k, n)| format!("node {k}, speed {:.2}", prop_float(v, &n.properties, "pSpeed").unwrap_or(1.0)))
                .collect();
            out.push((i, if ctrl.is_empty() { "never played".to_string() } else { ctrl.join("; ") }));
        }
    }
    out
}

// tests: the aim round trip keeps the axes orthonormal; a blend moves between two shots
#[cfg(test)]
mod tests {
    use super::*;
    use zartex_core::binio::Vec3 as V;

    #[test]
    fn aim_round_trips_and_stays_orthonormal() {
        let z = V { x: 0.0, y: 0.0, z: 0.0 };
        let mut o = MissionObject::Camera { right: z, up: z, forward: z, position: z, reserved: 0 };
        for (yaw, pitch) in [(0.0f32, 0.0f32), (37.0, -12.0), (-120.0, 30.0), (170.0, 5.0)] {
            assert!(set_camera_forward(&mut o, dir_from_yaw_pitch(yaw, pitch)));
            let MissionObject::Camera { right, up, forward, .. } = &o else { unreachable!() };
            let (y2, p2) = yaw_pitch([-forward.x, -forward.y, -forward.z]);
            assert!((y2 - yaw).abs() < 0.01 && (p2 - pitch).abs() < 0.01, "{yaw} {pitch} -> {y2} {p2}");
            let (f, r, u) = (glam::Vec3::new(forward.x, forward.y, forward.z), glam::Vec3::new(right.x, right.y, right.z), glam::Vec3::new(up.x, up.y, up.z));
            assert!(f.dot(r).abs() < 1e-4 && f.dot(u).abs() < 1e-4 && r.dot(u).abs() < 1e-4);
            assert!((r.cross(u) - f).length() < 1e-4, "right x up must equal the stored forward");
        }
    }

    #[test]
    fn blend_moves_between_shots() {
        let pose = |x: f32, yaw: f32| Pose { pos: [x, 0.0, 0.0], fwd: dir_from_yaw_pitch(yaw, 0.0), up: [0.0, 1.0, 0.0], fov: 60.0 };
        let shot = |start: f32, x: f32, yaw: f32, blend: f32| Shot { node: 0, actor: 0, duration: 4.0, blend, zoom: 1.0, start, pose: pose(x, yaw), object: None };
        let s = vec![shot(0.0, 0.0, 0.0, 0.0), shot(4.0, 10.0, 90.0, 2.0)];
        assert!((pose_at(&s, 3.0).unwrap().pos[0]).abs() < 1e-4);
        let mid = pose_at(&s, 5.0).unwrap();
        assert!(mid.pos[0] > 4.0 && mid.pos[0] < 6.0, "{:?}", mid.pos);
        assert!((pose_at(&s, 7.0).unwrap().pos[0] - 10.0).abs() < 1e-4);
    }
}
