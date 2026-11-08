//! Lua glue: MoonSharp-compatible argument conversion and the userdata types scripts see
//! (Actor, Node, CollectionOfWires, MissionSummary). The mission API itself lives in `api_*.rs`.
//! Exposes `State`, `rt_err`, `canon`, `add_aliases`, the handle types (ActorH, NodeH, WiresH, SummaryH), `Args` and the `ret_*` helpers.
//! Uses `script::ScriptState` and the mlua crate. Used by the api_*.rs files, `api_gen`, `compile`, `luaraw`, `readable` and `api_dpl`.
//!
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - added a camelCase and a snake_case alias for every function and member of a mission table (`add_aliases`)
//! - added the MISSION.missionSummary userdata (SummaryH) with the members X, Y, MoodId, MissionId, LocaleId, MissionLocaleId, CityType and Level
//!
//! Conversion rules were measured against MoonSharp (see oracle/probe.txt):
//!  * Lua number -> int: truncate; out of range -> i32::MIN; uint/byte/short wrap through (long)
//!  * Lua string/bool/nil are NOT coerced to numbers; number/bool -> string param is allowed
//!  * nil for an optional numeric param -> default; nil for an object param -> null
//!  * missing trailing args use the C# default; extra args are ignored
//!  * methods can be called with `.` (scripts do) and also with `:` (self is dropped)

use crate::script::*;
use mlua::{Error as LuaError, Lua, MetaMethod, MultiValue, Result as LuaResult, Table, UserData, UserDataMethods, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Shared, mutable script state used by every handle and by every API closure.
pub type State = Rc<RefCell<ScriptState>>;

/// Builds a Lua runtime error with the given message.
pub fn rt_err(msg: impl Into<String>) -> LuaError {
    LuaError::runtime(msg.into())
}


/// MoonSharp also exposes every member as camelCase and snake_case; map any of them back to PascalCase.
pub fn canon(key: &str) -> String {
    // underscores are dropped and the letter after each one (and the first letter) is uppercased
    let mut out = String::with_capacity(key.len());
    let mut up = true;
    for ch in key.chars() {
        if ch == '_' {
            up = true;
            continue;
        }
        if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Adds lowerCamel and snake_case aliases for every function in a table.
pub fn add_aliases(t: &Table) -> LuaResult<()> {
    let pairs: Vec<(String, Value)> = t.pairs::<String, Value>().filter_map(|p| p.ok()).collect();
    for (k, v) in pairs {
        // an alias is added only when the table has no entry with that name; the first letter is lowercased for the camelCase form
        let mut chars = k.chars();
        if let Some(f) = chars.next() {
            let camel: String = f.to_lowercase().chain(chars).collect();
            if camel != k && !t.contains_key(camel.clone())? {
                t.set(camel, v.clone())?;
            }
        }
        // snake_case: '_' goes before an uppercase letter that follows a lowercase letter or digit, or that is followed by a lowercase letter; all letters are lowercased
        let mut snake = String::new();
        for (i, ch) in k.chars().enumerate() {
            if ch.is_uppercase() && i > 0 {
                let prev = k.chars().nth(i - 1).unwrap();
                let next = k.chars().nth(i + 1);
                if prev.is_lowercase() || prev.is_ascii_digit() || next.map_or(false, |n| n.is_lowercase()) {
                    snake.push('_');
                }
            }
            snake.extend(ch.to_lowercase());
        }
        if snake != k && !t.contains_key(snake.clone())? {
            t.set(snake, v)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- handles

/// Userdata handle of an actor: the script state and the actor index.
#[derive(Clone)]
pub struct ActorH {
    pub st: State,
    pub idx: usize,
}

/// Userdata handle of a node: the script state, the node index and the wire collection index.
#[derive(Clone)]
pub struct NodeH {
    pub st: State,
    pub idx: usize,
    /// index into ScriptState::wires of this node's WireCollection
    pub wire: usize,
}

/// Userdata handle of a wire collection: the script state and the index into `ScriptState::wires`.
#[derive(Clone)]
pub struct WiresH {
    pub st: State,
    pub wire: usize,
}

/// Userdata handle of the mission summary (`MISSION.missionSummary`): the script state only.
#[derive(Clone)]
pub struct SummaryH {
    pub st: State,
}

// ---------------------------------------------------------------- args

/// Positional arguments of one call, with MoonSharp conversion rules.
pub struct Args {
    pub v: Vec<Value>,
}

/// Returns the Lua type name of a value, used in the conversion error messages.
fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        Value::Boolean(_) => "boolean",
        Value::Integer(_) | Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Table(_) => "table",
        Value::UserData(_) => "userdata",
        Value::Function(_) => "function",
        _ => "userdata",
    }
}

/// Returns the value as f64 when it is a Lua number (integer or float), otherwise None.
fn num_of(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Number(n) => Some(*n),
        _ => None,
    }
}

impl Args {
    /// Builds the argument list of one call. When the first argument is `self` (a call with `:`), it is dropped.
    pub fn new(args: MultiValue, self_is: impl Fn(&Value) -> bool) -> Args {
        let mut v: Vec<Value> = args.into_iter().collect();
        if let Some(first) = v.first() {
            if self_is(first) {
                v.remove(0);
            }
        }
        Args { v }
    }

    /// Number of arguments after `self` was dropped.
    pub fn len(&self) -> usize {
        self.v.len()
    }

    /// Error for argument `i` when it is absent or has the wrong type; `ty` is the .NET type named in the message.
    fn missing(&self, i: usize, ty: &str) -> LuaError {
        match self.v.get(i) {
            None => rt_err(format!("cannot convert a void to a clr type {ty}")),
            Some(v) => rt_err(format!("cannot convert a {} to a clr type {ty}", type_name(v))),
        }
    }

    /// Returns Some(number) if the arg is present, non-nil and numeric; Ok(None) if "use default";
    /// Err if present with a wrong type, or absent/nil with no default.
    fn num(&self, i: usize, has_default: bool, ty: &str) -> LuaResult<Option<f64>> {
        match self.v.get(i) {
            None | Some(Value::Nil) => {
                if has_default {
                    Ok(None)
                } else {
                    Err(self.missing(i, ty))
                }
            }
            Some(v) => match num_of(v) {
                Some(n) => Ok(Some(n)),
                None => Err(self.missing(i, ty)),
            },
        }
    }

    /// Argument `i` as a 32-bit integer. `def` is the default used when the argument is absent or nil.
    pub fn int(&self, i: usize, def: Option<i32>) -> LuaResult<i32> {
        Ok(match self.num(i, def.is_some(), "System.Int32")? {
            Some(n) => cast_int(n),
            None => def.unwrap(),
        })
    }
    /// Argument `i` as an unsigned 32-bit integer (wrapped through a 64-bit cast). `def` as in `int`.
    pub fn uint(&self, i: usize, def: Option<u32>) -> LuaResult<u32> {
        Ok(match self.num(i, def.is_some(), "System.UInt32")? {
            Some(n) => cast_long(n) as u32,
            None => def.unwrap(),
        })
    }
    /// Argument `i` as an unsigned 8-bit integer (wrapped through a 64-bit cast). `def` as in `int`.
    pub fn byte(&self, i: usize, def: Option<u8>) -> LuaResult<u8> {
        Ok(match self.num(i, def.is_some(), "System.Byte")? {
            Some(n) => cast_long(n) as u8,
            None => def.unwrap(),
        })
    }
    /// Argument `i` as a 16-bit integer (wrapped through a 64-bit cast). `def` as in `int`.
    pub fn short(&self, i: usize, def: Option<i16>) -> LuaResult<i16> {
        Ok(match self.num(i, def.is_some(), "System.Int16")? {
            Some(n) => cast_long(n) as i16,
            None => def.unwrap(),
        })
    }
    /// Argument `i` as a 64-bit integer. `def` as in `int`.
    pub fn long(&self, i: usize, def: Option<i64>) -> LuaResult<i64> {
        Ok(match self.num(i, def.is_some(), "System.Int64")? {
            Some(n) => cast_long(n),
            None => def.unwrap(),
        })
    }
    /// Argument `i` as a 32-bit float. `def` as in `int`.
    pub fn float(&self, i: usize, def: Option<f32>) -> LuaResult<f32> {
        Ok(match self.num(i, def.is_some(), "System.Single")? {
            Some(n) => n as f32,
            None => def.unwrap(),
        })
    }
    /// Argument `i` as a 64-bit float. `def` as in `int`.
    pub fn double(&self, i: usize, def: Option<f64>) -> LuaResult<f64> {
        Ok(match self.num(i, def.is_some(), "System.Double")? {
            Some(n) => n,
            None => def.unwrap(),
        })
    }

    /// Argument `i` as a boolean. Only Lua booleans are accepted; `def` as in `int`.
    pub fn boolean(&self, i: usize, def: Option<bool>) -> LuaResult<bool> {
        match self.v.get(i) {
            Some(Value::Boolean(b)) => Ok(*b),
            None | Some(Value::Nil) if def.is_some() => Ok(def.unwrap()),
            _ => Err(self.missing(i, "System.Boolean")),
        }
    }

    /// string param. Absent -> `def`; explicit nil -> None (null); number/bool are converted.
    pub fn string(&self, i: usize, def: Option<Option<&str>>) -> LuaResult<Option<String>> {
        match self.v.get(i) {
            None => match def {
                Some(d) => Ok(d.map(|s| s.to_string())),
                None => Err(self.missing(i, "System.String")),
            },
            Some(Value::Nil) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.to_string_lossy().to_string())),
            Some(Value::Boolean(b)) => Ok(Some(if *b { "True".into() } else { "False".into() })),
            Some(v) => match num_of(v) {
                Some(n) => Ok(Some(dotnet_number_to_string(n))),
                None => Err(self.missing(i, "System.String")),
            },
        }
    }

    /// Actor param (nil -> None)
    pub fn actor(&self, i: usize) -> LuaResult<Option<usize>> {
        match self.v.get(i) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::UserData(u)) => match u.borrow::<ActorH>() {
                Ok(a) => Ok(Some(a.idx)),
                Err(_) => Err(rt_err("cannot convert a userdata to a clr type Zartex.Actor")),
            },
            Some(v) => Err(rt_err(format!("cannot convert a {} to a clr type Zartex.Actor", type_name(v)))),
        }
    }

    /// Node param: returns the node index, or None for nil. Other types are an error.
    pub fn node(&self, i: usize) -> LuaResult<Option<usize>> {
        match self.v.get(i) {
            None | Some(Value::Nil) => Ok(None),
            Some(Value::UserData(u)) => match u.borrow::<NodeH>() {
                Ok(a) => Ok(Some(a.idx)),
                Err(_) => Err(rt_err("cannot convert a userdata to a clr type Zartex.Node")),
            },
            Some(v) => Err(rt_err(format!("cannot convert a {} to a clr type Zartex.Node", type_name(v)))),
        }
    }

    /// Argument `i` as a Lua table. Any other value, or no value, is an error.
    pub fn table(&self, i: usize) -> LuaResult<Table> {
        match self.v.get(i) {
            Some(Value::Table(t)) => Ok(t.clone()),
            _ => Err(self.missing(i, "MoonSharp.Interpreter.Table")),
        }
    }

    /// True when argument `i` is a Lua number.
    pub fn is_number(&self, i: usize) -> bool {
        matches!(self.v.get(i), Some(Value::Number(_)) | Some(Value::Integer(_)))
    }
    /// True when argument `i` is a Lua string.
    pub fn is_string(&self, i: usize) -> bool {
        matches!(self.v.get(i), Some(Value::String(_)))
    }
    /// True when argument `i` is absent or nil.
    pub fn is_nil(&self, i: usize) -> bool {
        matches!(self.v.get(i), None | Some(Value::Nil))
    }
}

/// Number -> string the way MoonSharp/.NET does for a `string` parameter (culture neutral here).
pub fn dotnet_number_to_string(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

// ---------------------------------------------------------------- userdata

/// Creates an actor handle as a Lua userdata value.
fn actor_ud(lua: &Lua, st: &State, idx: usize) -> LuaResult<Value> {
    Ok(Value::UserData(lua.create_userdata(ActorH { st: st.clone(), idx })?))
}
/// Creates a node handle (with its wire collection index) as a Lua userdata value.
fn node_ud(lua: &Lua, st: &State, idx: usize, wire: usize) -> LuaResult<Value> {
    Ok(Value::UserData(lua.create_userdata(NodeH { st: st.clone(), idx, wire })?))
}

/// Returns the actor with index `idx` as a Lua value.
pub fn ret_actor(lua: &Lua, st: &State, idx: usize) -> LuaResult<Value> {
    actor_ud(lua, st, idx)
}
/// Returns the node with index `idx` and wire collection `wire` as a Lua value.
pub fn ret_node(lua: &Lua, st: &State, idx: usize, wire: usize) -> LuaResult<Value> {
    node_ud(lua, st, idx, wire)
}

/// True when the value is a userdata of type `T`.
fn is_ud<T: 'static>(v: &Value) -> bool {
    matches!(v, Value::UserData(u) if u.is::<T>())
}

// Fields of a wire collection: Add, RemoveAt, RemoveRange and RemoveNode return functions; Index returns the wire index.
impl UserData for WiresH {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| -> LuaResult<Value> {
            let key = canon(&key);
            let st = this.st.clone();
            let wire = this.wire;
            let f = match key.as_str() {
                "Add" => lua.create_function(move |_, args: MultiValue| {
                    let a = Args::new(args, is_ud::<WiresH>);
                    // the opcode comes from the Node object itself (it may belong to another mission)
                    let (node_idx, op) = match a.v.first() {
                        Some(Value::UserData(u)) => match u.borrow::<NodeH>() {
                            Ok(n) => (n.idx, n.st.borrow().logic.nodes.definitions[n.idx].type_id),
                            Err(_) => return Err(rt_err("cannot convert a userdata to a clr type Zartex.Node")),
                        },
                        None | Some(Value::Nil) => return Err(rt_err("Attempt to add to wire collection a nil value")),
                        Some(_) => return Err(rt_err("cannot convert to a clr type Zartex.Node")),
                    };
                    let t = a.byte(1, Some(1))?;
                    st.borrow_mut().wires[wire].push(crate::logic::WireNode { wire_type: t, op_code: op, node_id: node_idx as i16 });
                    Ok(())
                })?,
                "RemoveAt" => lua.create_function(move |_, args: MultiValue| {
                    let a = Args::new(args, is_ud::<WiresH>);
                    let id = a.int(0, None)?;
                    let mut s = st.borrow_mut();
                    let w = &mut s.wires[wire];
                    if id < 0 || id as usize >= w.len() {
                        return Err(rt_err("Index was out of range."));
                    }
                    w.remove(id as usize);
                    Ok(())
                })?,
                "RemoveRange" => lua.create_function(move |_, args: MultiValue| {
                    let a = Args::new(args, is_ud::<WiresH>);
                    let id = a.int(0, None)?;
                    let count = a.int(1, None)?;
                    let mut s = st.borrow_mut();
                    let w = &mut s.wires[wire];
                    if id < 0 || count < 0 || (id + count) as usize > w.len() {
                        return Err(rt_err("Index and count do not denote a valid range of elements."));
                    }
                    w.drain(id as usize..(id + count) as usize);
                    Ok(())
                })?,
                "RemoveNode" => lua.create_function(move |_, args: MultiValue| {
                    let a = Args::new(args, is_ud::<WiresH>);
                    let node = a.node(0)?.ok_or_else(|| rt_err("Object reference not set to an instance of an object."))?;
                    let mut s = st.borrow_mut();
                    let w = &mut s.wires[wire];
                    if let Some(p) = w.iter().position(|x| x.node_id == node as i16) {
                        w.remove(p);
                    }
                    Ok(())
                })?,
                "Index" => return Ok(Value::Integer(wire as i64)),
                "Wires" => return Ok(Value::Nil),
                _ => return Ok(Value::Nil),
            };
            Ok(Value::Function(f))
        });
    }
}

// Fields of an actor: Index, TypeId and Flags. Other names go to `prop_methods`.
impl UserData for ActorH {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| -> LuaResult<Value> {
            let key = canon(&key);
            let st = this.st.clone();
            let idx = this.idx;
            match key.as_str() {
                "Index" => Ok(Value::Integer(idx as i64)),
                "TypeId" => Ok(Value::Integer(st.borrow().logic.actors.definitions[idx].type_id as i64)),
                "Flags" => Ok(Value::Integer(st.borrow().logic.actors.definitions[idx].flags as i64)),
                other => prop_methods(lua, other, st, idx, true),
            }
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (key, val): (String, Value)| -> LuaResult<()> {
            let key = canon(&key);
            let n = num_of(&val).ok_or_else(|| rt_err("cannot convert value"))?;
            let mut s = this.st.borrow_mut();
            let d = &mut s.logic.actors.definitions[this.idx];
            match key.as_str() {
                "TypeId" => d.type_id = cast_long(n) as u8,
                "Flags" => d.flags = cast_long(n) as i16,
                _ => return Err(rt_err(format!("userdata 'Actor' cannot be indexed with '{key}'"))),
            }
            Ok(())
        });
    }
}

// Fields of a node: Index, Wires / WireCollection, TypeId, Flags and SetWireCollection. Other names go to `prop_methods`.
impl UserData for NodeH {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| -> LuaResult<Value> {
            let key = canon(&key);
            let st = this.st.clone();
            let idx = this.idx;
            let wire = this.wire;
            match key.as_str() {
                "Index" => Ok(Value::Integer(idx as i64)),
                "Wires" | "WireCollection" => Ok(Value::UserData(lua.create_userdata(WiresH { st, wire })?)),
                "TypeId" => Ok(Value::Integer(st.borrow().logic.nodes.definitions[idx].type_id as i64)),
                "Flags" => Ok(Value::Integer(st.borrow().logic.nodes.definitions[idx].flags as i64)),
                "SetWireCollection" => Ok(Value::Function(lua.create_function(move |_, args: MultiValue| {
                    let a = Args::new(args, is_ud::<NodeH>);
                    let other = match a.v.first() {
                        Some(Value::UserData(u)) => u.borrow::<WiresH>().map(|w| w.wire).map_err(|_| rt_err("bad wire collection"))?,
                        _ => return Err(rt_err("cannot convert to CollectionOfWires")),
                    };
                    let append_id = a.boolean(1, Some(false))?;
                    let mut s = st.borrow_mut();
                    let copy = s.wires[other].clone();
                    s.wires[wire] = copy;
                    // with append_id, the first property of type 19 (the wire collection) gets the negative index of the source collection
                    if append_id {
                        for p in s.logic.nodes.definitions[idx].properties.iter_mut() {
                            if p.type_id == 19 {
                                p.value = crate::logic::PropValue::Int(-(other as i32));
                                break;
                            }
                        }
                    }
                    Ok(())
                })?)),
                other => prop_methods(lua, other, st, idx, false),
            }
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (key, val): (String, Value)| -> LuaResult<()> {
            let key = canon(&key);
            let mut s = this.st.borrow_mut();
            match key.as_str() {
                "TypeId" | "Flags" => {
                    let n = num_of(&val).ok_or_else(|| rt_err("cannot convert value"))?;
                    let d = &mut s.logic.nodes.definitions[this.idx];
                    if key == "TypeId" {
                        d.type_id = cast_long(n) as u8
                    } else {
                        d.flags = cast_long(n) as i16
                    }
                    Ok(())
                }
                "Wires" | "WireCollection" => match val {
                    Value::UserData(u) => {
                        let other = u.borrow::<WiresH>().map(|w| w.wire).map_err(|_| rt_err("bad wire collection"))?;
                        let copy = s.wires[other].clone();
                        s.wires[this.wire] = copy;
                        Ok(())
                    }
                    _ => Err(rt_err("cannot convert to CollectionOfWires")),
                },
                _ => Err(rt_err(format!("userdata 'Node' cannot be indexed with '{key}'"))),
            }
        });
    }
}

/// Returns the definition at `idx` from the actor list (`is_actor` true) or from the node list.
fn def_of(s: &mut ScriptState, is_actor: bool, idx: usize) -> &mut crate::logic::NodeDef {
    if is_actor {
        &mut s.logic.actors.definitions[idx]
    } else {
        &mut s.logic.nodes.definitions[idx]
    }
}

/// Methods shared by Actor and Node (property setters / getters / SetColor).
fn prop_methods(lua: &Lua, key: &str, st: State, idx: usize, is_actor: bool) -> LuaResult<Value> {
    use crate::logic::PropValue;
    let f = match key {
        "SetColor" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let (r, g, b) = (a.byte(0, None)?, a.byte(1, None)?, a.byte(2, None)?);
            let al = a.byte(3, Some(255))?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            d.color = node_color(r as f64, g as f64, b as f64, al as f64);
            Ok(())
        })?,
        "SetFloatPropertyValue" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let id = a.int(0, None)?;
            let val = a.float(1, None)?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            match d.properties.get_mut(id as usize).map(|p| &mut p.value) {
                Some(PropValue::Float(f)) => *f = val,
                _ => return Err(rt_err("Unable to cast object to type 'FloatProperty'.")),
            }
            Ok(())
        })?,
        "SetIntPropertyValue" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let id = a.int(0, None)?;
            let val = a.int(1, None)?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            match d.properties.get_mut(id as usize).map(|p| (p.type_id, &mut p.value)) {
                Some((1, PropValue::Int(v))) => *v = val,
                _ => return Err(rt_err("Unable to cast object to type 'IntegerProperty'.")),
            }
            Ok(())
        })?,
        "SetFloat3PropertyValue" | "SetFloat4PropertyValue" => {
            let four = key == "SetFloat4PropertyValue";
            lua.create_function(move |_, args: MultiValue| {
                let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
                let id = a.int(0, None)?;
                let (x, y, z) = (a.float(1, None)?, a.float(2, None)?, a.float(3, None)?);
                let w = if four { a.float(4, None)? } else { 0.0 };
                let mut s = st.borrow_mut();
                let d = def_of(&mut s, is_actor, idx);
                match d.properties.get_mut(id as usize).map(|p| (p.type_id, &mut p.value)) {
                    Some((17, PropValue::Vec4(v))) if !four => *v = crate::binio::Vec4 { x, y, z, w: 0.0 },
                    Some((_, PropValue::Raw(_))) if four => return Err(rt_err("Float4Property is not supported")),
                    _ => return Err(rt_err("Unable to cast object to the property type.")),
                }
                Ok(())
            })?
        }
        "SetTextPropertyValue" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let id = a.int(0, None)?;
            let index = a.short(1, None)?;
            let value = a.short(2, None)?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            match d.properties.get_mut(id as usize).map(|p| &mut p.value) {
                Some(PropValue::TextFile { value: v, index: i }) => {
                    *i = index;
                    *v = value;
                }
                _ => return Err(rt_err("Unable to cast object to type 'TextFileItemProperty'.")),
            }
            Ok(())
        })?,
        "SetAudioPropertyValue" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let id = a.int(0, None)?;
            let bank = a.short(1, None)?;
            let sample = a.short(2, None)?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            match d.properties.get_mut(id as usize).map(|p| &mut p.value) {
                Some(PropValue::Audio { bank: b, sample: sm }) => {
                    *b = bank as i32;
                    *sm = sample as i32;
                }
                _ => return Err(rt_err("Unable to cast object to type 'AudioProperty'.")),
            }
            Ok(())
        })?,
        "SetActorPropertyValue" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let id = a.int(0, None)?;
            let actor = a.actor(1)?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            match d.properties.get_mut(id as usize).map(|p| (p.type_id, &mut p.value)) {
                Some((7, PropValue::Int(v))) => *v = actor.map(|x| x as i32).unwrap_or(-1),
                _ => return Err(rt_err("Unable to cast object to type 'ActorProperty'.")),
            }
            Ok(())
        })?,
        "GetProperyValue" => lua.create_function(move |_, args: MultiValue| {
            let a = Args::new(args, |v| is_ud::<ActorH>(v) || is_ud::<NodeH>(v));
            let id = a.int(0, None)?;
            let mut s = st.borrow_mut();
            let d = def_of(&mut s, is_actor, idx);
            Ok(match d.properties.get(id as usize).map(|p| (p.type_id, &p.value)) {
                Some((_, PropValue::Float(f))) => Value::Number(*f as f64),
                Some((1, PropValue::Int(v))) | Some((7, PropValue::Int(v))) => Value::Number(*v as f64),
                Some(_) => Value::Nil,
                None => return Err(rt_err("Index was out of range.")),
            })
        })?,
        _ => return Ok(Value::Nil),
    };
    Ok(Value::Function(f))
}

// Fields of the mission summary (MISSION.missionSummary): X, Y, MoodId / MissionId, LocaleId / MissionLocaleId, CityType and Level.
impl UserData for SummaryH {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| -> LuaResult<Value> {
            let key = canon(&key);
            let s = this.st.borrow();
            Ok(match key.as_str() {
                "X" => Value::Number(s.summary.start_position.x as f64),
                "Y" => Value::Number(s.summary.start_position.y as f64),
                "MoodId" | "MissionId" => Value::Integer(s.summary.mood_id as i64),
                "LocaleId" | "MissionLocaleId" => Value::Integer(s.summary.locale_id as i64),
                "CityType" => Value::Integer(s.summary.city_type as i64),
                "Level" => Value::String(lua.create_string(s.level_name())?),
                _ => Value::Nil,
            })
        });
        m.add_meta_method(MetaMethod::NewIndex, |_, this, (key, val): (String, Value)| -> LuaResult<()> {
            let key = canon(&key);
            let mut s = this.st.borrow_mut();
            match key.as_str() {
                "Level" => {
                    // a string sets the city type by its name; nil resets the city type to 0
                    if let Value::String(st_) = &val {
                        s.summary.city_type = crate::mission::MissionSummary::city_by_name(&st_.to_string_lossy());
                    } else if matches!(val, Value::Nil) {
                        s.summary.city_type = 0;
                    } else {
                        return Err(rt_err("cannot convert value to string"));
                    }
                    Ok(())
                }
                _ => {
                    let n = num_of(&val).ok_or_else(|| rt_err("cannot convert value"))?;
                    match key.as_str() {
                        "X" => s.summary.start_position.x = n as f32,
                        "Y" => s.summary.start_position.y = n as f32,
                        "MoodId" | "MissionId" => s.summary.mood_id = cast_long(n) as i16,
                        "LocaleId" | "MissionLocaleId" => s.summary.locale_id = cast_long(n) as i16,
                        "CityType" => s.summary.city_type = cast_int(n),
                        _ => return Err(rt_err(format!("userdata 'MissionSummary' cannot be indexed with '{key}'"))),
                    }
                    Ok(())
                }
            }
        });
    }
}
