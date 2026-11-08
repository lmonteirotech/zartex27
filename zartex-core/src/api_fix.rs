//! Methods the generator mis-translates: the C# first builds a property list and then, when `name != null`,
//! replaces the whole list (the strings of BOTH lists end up in the string table).
//! Exposes `register`, which sets CounterControl and CounterWatch on a mission table.
//! Uses `api_manual::reg` to register the functions, `logic::PropValue` for the property values and the Lua glue of `luaapi`.
//! Used by `api_manual::register_manual`, which runs after the generated methods are registered, so these replace the generated methods of the same name (api_gen.rs).
//!
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - replaces the whole property list when a name is given, for CounterControl and CounterWatch, where the generated methods keep the first list

use crate::api_manual::reg;
use crate::logic::PropValue;
use crate::luaapi::*;
use crate::script::*;
use mlua::{Lua, Result as LuaResult, Table};

/// Creates a counter node of `type_id` and returns (node index, wire collection index).
/// `third_name` and `third` give the third property (pAction for CounterControl, pCondition for CounterWatch).
/// When `name` is given, the property list is rebuilt; the strings of the first list stay in the string table.
#[allow(clippy::too_many_arguments)]
fn counter_node(
    st: &mut ScriptState,
    type_id: u8,
    value: i32,
    counter_index: i32,
    third_name: &str,
    third: i32,
    name: Option<String>,
    flags: i32,
    note: Option<String>,
    r: i32,
    g: i32,
    b: i32,
) -> (usize, usize) {
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let (idx, wire) = st.create_node(type_id, note.as_deref(), color, 0, |st| {
        vec![
            st.prop("pValue", 1, PropValue::Int(value)),
            st.prop("pCounterIndex", 1, PropValue::Int(counter_index)),
            st.prop(third_name, 6, PropValue::Int(third)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    if let Some(name) = name {
        // the list built above is replaced, not merged with this one
        let w = st.prop("pWireCollection", 19, PropValue::Int(wire as i32));
        let sid = st.string_id(&name);
        let n = st.prop("pName", 3, PropValue::Str(sid));
        let props = vec![
            w,
            n,
            st.prop("pValue", 1, PropValue::Int(value)),
            st.prop("pCounterIndex", 1, PropValue::Int(counter_index)),
            st.prop(third_name, 6, PropValue::Int(third)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ];
        st.logic.nodes.definitions[idx].properties = props;
    }
    (idx, wire)
}

/// Registers CounterControl (type 17, third property pAction) and CounterWatch (type 4, third property pCondition).
/// The `_dpl` flag is not used here.
pub fn register(lua: &Lua, t: &Table, st: &State, _dpl: bool) -> LuaResult<()> {
    for (method, type_id, third) in [("CounterControl", 17u8, "pAction"), ("CounterWatch", 4u8, "pCondition")] {
        reg(lua, t, st, method, move |lua, st, a| {
            let (idx, wire) = counter_node(
                &mut st.borrow_mut(),
                type_id,
                a.int(0, None)?,
                a.int(1, None)?,
                third,
                a.int(2, None)?,
                a.string(3, Some(None))?,
                a.int(4, Some(0))?,
                a.string(5, Some(Some("")))?,
                a.int(6, Some(0))?,
                a.int(7, Some(200))?,
                a.int(8, Some(122))?,
            );
            ret_node(lua, st, idx, wire)
        })?;
    }
    Ok(())
}
