//! Runtime state of a Lua mission script (port of the data members of LuaMissionScript).
//! Everything the Lua API mutates lives here; the Lua glue only holds indexes into it.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class and its compiler).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - the X87 trait (xsin, xcos) reproduces the results of the 32-bit original, which used x87 fsin and fcos
//!
//! This file exposes ScriptState (the mutable script state), ScriptSummary, the X87 trait and the helpers that
//! convert doubles to bytes and integers (node_color, cast_byte, cast_int, cast_long) and forward_from_radians.
//! It uses the binary helpers (binio), the logic model (logic), the mission model (mission) and the object model
//! (objects). It is used by the Lua glue (luaapi, luaraw, api_gen, api_manual, api_part_a, api_part_b, api_part_c,
//! api_fix, api_dpl), by compile (the Lua environment and the .mpc output) and by the tests.

use crate::binio::*;
use crate::logic::*;
use crate::mission::*;
use crate::objects::*;

// ---------------------------------------------------------------- numeric conversions
/// `new NodeColor(double r, double g, double b, double a)`: (byte)(x * 255) with C# unchecked semantics.
/// The C# call sites pass ints, so the `double` overload is always the one chosen.
pub fn node_color(r: f64, g: f64, b: f64, a: f64) -> [u8; 4] {
    [cast_byte(r * 255.0), cast_byte(g * 255.0), cast_byte(b * 255.0), cast_byte(a * 255.0)]
}

/// (byte)double as the .NET Framework x64 JIT does it: truncate to int32 (indefinite -> i32::MIN), keep low byte.
pub fn cast_byte(x: f64) -> u8 {
    (cast_int(x) & 0xFF) as u8
}

/// (int)double: truncation; NaN/out of range -> i32::MIN (cvttsd2si "integer indefinite")
pub fn cast_int(x: f64) -> i32 {
    if x.is_nan() || x >= 2147483648.0 || x <= -2147483649.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// (long)double: truncation at 64 bits; NaN/out of range -> i64::MIN, the same rule as cast_int.
pub fn cast_long(x: f64) -> i64 {
    if x.is_nan() || x >= 9223372036854775808.0 || x < -9223372036854775808.0 {
        i64::MIN
    } else {
        x as i64
    }
}

/// Mission summary values set by the script: start position, city type, mood and locale ids.
#[derive(Debug, Clone, Default)]
pub struct ScriptSummary {
    /// Start position of the mission
    pub start_position: Vec2,
    /// City type; 1 to 6 select a level name (see level_name)
    pub city_type: i32,
    /// Mood id, stored as the mission id of the summary
    pub mood_id: i16,
    /// Locale id
    pub locale_id: i16,
}

/// The state of a running Lua mission script. The Lua API writes into it; compile_to_mpc and
/// apply_snippet copy its parts into a mission.
#[derive(Debug, Clone)]
pub struct ScriptState {
    /// Objects of the mission
    pub objects: Vec<MissionObject>,
    /// Actor, node, string and sound bank tables
    pub logic: LogicData,
    /// LuaMissionScript.wireCollection (indexes are what nodes reference)
    pub wires: Vec<Vec<WireNode>>,
    /// Summary values of the mission
    pub summary: ScriptSummary,
    /// Integer values stored for spool vehicles
    pub spool_vehicles: Vec<i32>,
    /// Folder of the script file (empty for a state that has no file)
    pub working_dir: String,
    /// ActorSetup.Table
    pub actor_setup: Vec<Vec<i32>>,
    /// Path of the script file (empty for a state that has no file)
    pub source_path: String,
    /// BuildingInstanceData
    pub instances: MissionInstanceData,
}

impl Default for ScriptState {
    fn default() -> Self {
        Self::new("")
    }
}

impl ScriptState {
    /// Creates an empty state. The string table starts with "" (id 0) and "Unknown" (id 1), and
    /// `instances.unk` starts as -4, -4.
    pub fn new(working_dir: &str) -> Self {
        let mut logic = LogicData::default();
        logic.strings.append_string("");
        logic.strings.append_string("Unknown");
        let mut instances = MissionInstanceData::default();
        instances.unk[0] = -4;
        instances.unk[1] = -4;
        ScriptState {
            objects: Vec::new(),
            logic,
            wires: Vec::new(),
            summary: ScriptSummary::default(),
            spool_vehicles: Vec::new(),
            working_dir: working_dir.to_string(),
            actor_setup: Vec::new(),
            source_path: String::new(),
            instances,
        }
    }

    // ---- strings

    /// StringCollection.findStringIdByValueOrCreateNew
    pub fn string_id(&mut self, s: &str) -> i16 {
        match self.logic.strings.find_id(s) {
            Some(i) => i as i16,
            None => self.logic.strings.append_string(s) as i16,
        }
    }

    /// `if (note == "" | note == null) "Unknown" else note`
    pub fn note_id(&mut self, note: Option<&str>) -> i16 {
        match note {
            None | Some("") => self.string_id("Unknown"),
            Some(n) => self.string_id(n),
        }
    }

    /// `new XProperty(v) { StringId = findStringIdByValueOrCreateNew(name) }`
    pub fn prop(&mut self, name: &str, type_id: u8, value: PropValue) -> Property {
        let string_id = self.string_id(name);
        Property { type_id, string_id, value }
    }

    // ---- wire collections

    /// `new CollectionOfWires(0, count); wireCollection.Add(cow)`; returns its index
    pub fn new_wire_collection(&mut self) -> usize {
        self.wires.push(Vec::new());
        self.wires.len() - 1
    }

    // ---- definitions

    /// Adds a logic node and returns (node index, wire collection index) using the common template:
    /// note string id, a fresh wire collection, then a `pWireCollection` property followed by `props`.
    /// `make_props` runs after the wire collection exists so string ids are created in C# order.
    /// `color` and `flags` are stored on the definition as they are.
    pub fn create_node(
        &mut self,
        type_id: u8,
        note: Option<&str>,
        color: [u8; 4],
        flags: i16,
        make_props: impl FnOnce(&mut ScriptState) -> Vec<Property>,
    ) -> (usize, usize) {
        let string_id = self.note_id(note);
        let wire = self.new_wire_collection();
        let mut props = vec![self.prop("pWireCollection", 19, PropValue::Int(wire as i32))];
        props.extend(make_props(self));
        let mut def = NodeDef::new(type_id, string_id);
        def.color = color;
        def.flags = flags;
        def.properties = props;
        self.logic.nodes.definitions.push(def);
        (self.logic.nodes.definitions.len() - 1, wire)
    }

    /// Adds an actor definition (properties built by the caller after the note id is taken).
    /// `object_id` is stored on the definition. Returns the index of the new actor definition.
    pub fn create_actor(
        &mut self,
        type_id: u8,
        note: Option<&str>,
        color: [u8; 4],
        flags: i16,
        object_id: i32,
        make_props: impl FnOnce(&mut ScriptState) -> Vec<Property>,
    ) -> usize {
        let string_id = self.note_id(note);
        let props = make_props(self);
        let mut def = NodeDef::new(type_id, string_id);
        def.color = color;
        def.flags = flags;
        def.object_id = object_id;
        def.properties = props;
        self.logic.actors.definitions.push(def);
        self.logic.actors.definitions.len() - 1
    }

    /// Appends a wire to wire collection `wire_index`. The wire points to the node at `node_index`;
    /// its op code is the type id of that node's definition.
    pub fn add_wire(&mut self, wire_index: usize, node_index: usize, wire_type: u8) {
        let node_id = node_index as i16;
        let op = self.logic.nodes.definitions[node_index].type_id;
        self.wires[wire_index].push(WireNode { wire_type, op_code: op, node_id });
    }

    // ---- summary

    /// Level name for `summary.city_type`: 1 to 6 give the Miami, Nice and Istanbul names (day and night); other values give "Unknown".
    pub fn level_name(&self) -> &'static str {
        match self.summary.city_type {
            1 => "Miami_Day",
            2 => "Miami_Night",
            3 => "Nice_Day",
            4 => "Nice_Night",
            5 => "Istanbul_Day",
            6 => "Istanbul_Night",
            _ => "Unknown",
        }
    }
}

/// Forward vector used by the character/vehicle creation code:
/// (Math.Cos(0) * Math.Cos(a), 0, Math.Cos(0) * Math.Sin(a)) cast to float.
pub fn forward_from_radians(a: f64) -> [f32; 3] {
    [(1.0f64 * a.xcos()) as f32, 0.0, (1.0f64 * a.xsin()) as f32]
}

// ---------------------------------------------------------------- x87 trigonometry

/// The original tool ran as a 32-bit .NET process, so Math.Sin/Math.Cos executed x87 `fsin`/`fcos`.
/// Those reduce the argument with a 66-bit approximation of pi, which changes results near multiples
/// of pi/2 (e.g. sin(PI) = 1.2246063538223773e-16 instead of 1.2246467991473532e-16). That is visible
/// in the f32 forward vectors, so we reproduce the same reduction.
pub trait X87 {
    /// Sine of `self` (radians) with the x87 argument reduction.
    fn xsin(self) -> f64;
    /// Cosine of `self` (radians) with the x87 argument reduction.
    fn xcos(self) -> f64;
}

// (66-bit pi) / 2 as a double-double: high and low part
const HALF_PI66_HI: f64 = 1.5707963267948966;
const HALF_PI66_LO: f64 = 6.123031769111886e-17;

/// Reduces `x` by multiples of pi/2: returns (quadrant n, remainder r) with x ~ n * pi/2 + r.
/// Used by xsin and xcos; the quadrant is taken modulo 4 by the callers.
fn x87_reduce(x: f64) -> (i64, f64) {
    let n = (x / HALF_PI66_HI).round();
    // x - n*hi is exact via fma; then subtract the low part
    let r = (-n).mul_add(HALF_PI66_HI, x) - n * HALF_PI66_LO;
    (n as i64, r)
}

impl X87 for f64 {
    // NaN and infinite arguments give NaN
    fn xsin(self) -> f64 {
        if !self.is_finite() {
            return f64::NAN;
        }
        let (n, r) = x87_reduce(self);
        match n.rem_euclid(4) {
            0 => r.sin(),
            1 => r.cos(),
            2 => -r.sin(),
            _ => -r.cos(),
        }
    }
    // NaN and infinite arguments give NaN
    fn xcos(self) -> f64 {
        if !self.is_finite() {
            return f64::NAN;
        }
        let (n, r) = x87_reduce(self);
        match n.rem_euclid(4) {
            0 => r.cos(),
            1 => -r.sin(),
            2 => -r.cos(),
            _ => r.sin(),
        }
    }
}
