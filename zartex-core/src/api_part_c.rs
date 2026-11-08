//! Lua API port, part c: LuaMissionScript methods that create logic nodes.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the Zartex 2 Lua mission script class and its compiler).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! Exposes: `register`, which adds the methods to the LuaMissionScript table, and the Rust functions behind them:
//! character_name_control, character_control and its wrappers (set_character_take_health, give_character_weapon, ...),
//! fade_screen, fade_out, fade_in, bomb_car, actor_has_reached_point, play_audio, the character drive functions,
//! show_clock and start_clock. Most of them return a node: (node index, wire collection index). Some also create
//! an actor (a drive target or a volume) before the node.
//! Uses: api_gen (actor_creation, countdown_intro), api_manual (nre, reg), binio (Vec3, Vec4), logic (NodeDef,
//! PropValue), luaapi (Args, ret_node, rt_err, the Lua argument reader), objects (MissionObject), script (ScriptState,
//! node_color).
//! Used by: api_manual::register_manual calls `register`. No other module calls these functions.
#![allow(clippy::too_many_arguments)]

use crate::api_gen::{actor_creation, countdown_intro};
use crate::api_manual::{nre, reg};
use crate::binio::{Vec3, Vec4};
use crate::logic::{NodeDef, PropValue};
use crate::luaapi::*;
use crate::objects::MissionObject;
use crate::script::*;
use mlua::{Lua, Result as LuaResult, Table};

/// Result of a node builder: (node index, wire collection index), or an error.
type NodeOut = LuaResult<(usize, usize)>;

// ---------------------------------------------------------------- CharacterNameControl

/// CharacterNameControl: creates a node (type 107) that sets the name text of a character, whether the name
/// is shown, and the player number. The name is stored as UTF-16. Returns (node index, wire collection index).
pub fn character_name_control(
    st: &mut ScriptState,
    character: Option<usize>,
    name: &str,
    show_name: bool,
    player_num: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let ch = character.map(|c| c as i32).unwrap_or(-1);
    let units: Vec<u16> = name.encode_utf16().collect();
    Ok(st.create_node(107, note, color, 0, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(ch)),
            st.prop("pCharacterName", 21, PropValue::Unicode(units)),
            st.prop("pShowName", 4, PropValue::Bool(show_name)),
            st.prop("pPlayerNum", 1, PropValue::Int(player_num)),
        ]
    }))
}

// ---------------------------------------------------------------- CharacterControl

/// CharacterControl(character, property, value, weapon, personalityindex, vehicle, target,
/// animation, animAction, padFileName, numPadRecordings, flags, note, r, g, b)
/// Creates a CharacterControl node (type 18) for a Character actor (type 2). `property` is the code of the
/// action the node performs. Returns (node index, wire collection index).
pub fn character_control(
    st: &mut ScriptState,
    character: Option<usize>,
    property: i32,
    value: f32,
    weapon: i32,
    personality_index: i32,
    vehicle: Option<usize>,
    target: Option<usize>,
    animation: &str,
    anim_action: i16,
    pad_file_name: &str,
    num_pad_recordings: i32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    let character = character.ok_or_else(|| rt_err("Bad argument #1 - Attempt to control a nil value"))?;
    if st.logic.actors.definitions[character].type_id != 2 {
        return Err(rt_err("Bad argument #1 - The inputed actor is not a Character actor type"));
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    Ok(st.create_node(18, note, color, 0, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(character as i32)),
            st.prop("pVehicle", 7, PropValue::Int(vehicle.map(|v| v as i32).unwrap_or(-1))),
            st.prop("pTarget", 7, PropValue::Int(target.map(|v| v as i32).unwrap_or(-1))),
            st.prop("pPersonalityIndex", 1, PropValue::Int(personality_index)),
            st.prop("pProperty", 6, PropValue::Int(property)),
            st.prop("pWeapon", 6, PropValue::Int(weapon)),
            st.prop("pValue", 2, PropValue::Float(value)),
            {
                // the animation name id is taken before the property's own name id
                let pid = st.string_id(animation);
                st.prop("pAnimation", 8, PropValue::TextFile { value: pid, index: anim_action })
            },
            st.prop("pFlags", 9, PropValue::Int(flags)),
            st.prop("pNumPadRecordings", 1, PropValue::Int(num_pad_recordings)),
            {
                let sid = st.string_id(pad_file_name);
                st.prop("pPadFileName", 3, PropValue::Str(sid))
            },
        ]
    }))
}

// Thin wrappers around CharacterControl.

/// Shared shape of SetCharacterTakeHealth .. SetCharacterVulnerability: (character, amount, flags, note, r, g, b).
type AmountFn = fn(&mut ScriptState, Option<usize>, f32, i32, Option<&str>, i32, i32, i32) -> NodeOut;
/// Shared shape of the flags-only wrappers: (character, flags, note, r, g, b).
type FlagsFn = fn(&mut ScriptState, Option<usize>, i32, Option<&str>, i32, i32, i32) -> NodeOut;

/// SetCharacterTakeHealth: CharacterControl with property code 1 and `amount` as the value.
pub fn set_character_take_health(st: &mut ScriptState, character: Option<usize>, amount: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 1, amount, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterAddHealth: CharacterControl with property code 2 and `amount` as the value.
pub fn set_character_add_health(st: &mut ScriptState, character: Option<usize>, amount: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 2, amount, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterHealthTo: CharacterControl with property code 3 and `new_health` as the value.
pub fn set_character_health_to(st: &mut ScriptState, character: Option<usize>, new_health: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 3, new_health, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterAddFelony: CharacterControl with property code 4 and `amount` as the value.
pub fn set_character_add_felony(st: &mut ScriptState, character: Option<usize>, amount: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 4, amount, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterTakeFelony: CharacterControl with property code 5 and `amount` as the value.
pub fn set_character_take_felony(st: &mut ScriptState, character: Option<usize>, amount: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 5, amount, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterFelonyTo: CharacterControl with property code 6 and `felony` as the value.
pub fn set_character_felony_to(st: &mut ScriptState, character: Option<usize>, felony: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 6, felony, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterVulnerability: CharacterControl with property code 7 and `vulnerability` as the value.
pub fn set_character_vulnerability(st: &mut ScriptState, character: Option<usize>, vulnerability: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 7, vulnerability, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterUsePadFile: CharacterControl with property code 8, the vehicle, the pad recording file name
/// and the number of recordings.
pub fn set_character_use_pad_file(
    st: &mut ScriptState,
    character: Option<usize>,
    vehicle: Option<usize>,
    file_name: &str,
    num_recordings: i32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    character_control(st, character, 8, 0.0, 0, -1, vehicle, None, "", -1, file_name, num_recordings, flags, note, r, g, b)
}

/// GiveCharacterWeapon: CharacterControl with property code 9, `weapon` and `clips` as the value.
pub fn give_character_weapon(st: &mut ScriptState, character: Option<usize>, weapon: i32, clips: f32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 9, clips, weapon, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterUnStuckInVehicle: CharacterControl with property code 11 and value 1.0.
pub fn set_character_unstuck_in_vehicle(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 11, 1.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterStuckInVehicle: CharacterControl with property code 12 and value 1.0.
pub fn set_character_stuck_in_vehicle(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 12, 1.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterStopAnimation: CharacterControl with property code 13.
pub fn set_character_stop_animation(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 13, 0.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterPlayAnimation: CharacterControl with property code 14, the animation name and its id (animAction).
pub fn set_character_play_animation(st: &mut ScriptState, character: Option<usize>, animation_name: &str, animation_id: i16, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 14, 0.0, 0, -1, None, None, animation_name, animation_id, "", 0, flags, note, r, g, b)
}

/// SetCharacterPersonalityIndexTo: CharacterControl with property code 19 and `personality` as the personality index.
pub fn set_character_personality_index_to(st: &mut ScriptState, character: Option<usize>, personality: i32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 19, 0.0, 0, personality, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// ClearCharacterWeapons: CharacterControl with property code 27.
pub fn clear_character_weapons(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 27, 0.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// SetCharacterGetOutOfVehicle: CharacterControl with property code 20.
pub fn set_character_get_out_of_vehicle(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 20, 0.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// CharacterEquipWeapon: CharacterControl with property code 22.
pub fn character_equip_weapon(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 22, 0.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

/// CharacterReholsterWeapon: CharacterControl with property code 23.
pub fn character_reholster_weapon(st: &mut ScriptState, character: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    character_control(st, character, 23, 0.0, 0, -1, None, None, "", -1, "", 0, flags, note, r, g, b)
}

// ---------------------------------------------------------------- FadeScreen / FadeOut / FadeIn

/// FadeScreen(duration, fadeToColorTime, fadeFromColorTime, R, G, B, A, note, r, g, b)
pub fn fade_screen(
    st: &mut ScriptState,
    duration: f32,
    fade_to_color_time: f32,
    fade_from_color_time: f32,
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    Ok(st.create_node(124, note, color, 0, |st| {
        vec![
            st.prop("pFadeToColourTime", 2, PropValue::Float(fade_to_color_time)),
            st.prop("pFadeFromColourTime", 2, PropValue::Float(fade_from_color_time)),
            st.prop("pDuration", 2, PropValue::Float(duration)),
            // new Vector4(R, G, B, A): the byte components widen to float, w keeps A
            st.prop(
                "pColour",
                17,
                PropValue::Vec4(Vec4 { x: red as f32, y: green as f32, z: blue as f32, w: alpha as f32 }),
            ),
        ]
    }))
}

/// FadeOut: fade_screen with a fade-to-colour time of 0 and the fade-from time set to `duration`.
pub fn fade_out(st: &mut ScriptState, duration: f32, red: u8, green: u8, blue: u8, alpha: u8, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    fade_screen(st, duration, 0.0, duration, red, green, blue, alpha, note, r, g, b)
}

/// FadeIn: fade_screen with the fade-to-colour time set to `duration` and the fade-from time of 0.
pub fn fade_in(st: &mut ScriptState, duration: f32, red: u8, green: u8, blue: u8, alpha: u8, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    fade_screen(st, duration, duration, 0.0, red, green, blue, alpha, note, r, g, b)
}

// ---------------------------------------------------------------- BombCar

/// BombCar: creates a BombCar node (type 122) for a Vehicle actor (type 3) with the warning audio (bank and
/// sample), and adds the bank to the sound bank load list. Returns (node index, wire collection index).
pub fn bomb_car(st: &mut ScriptState, vehicle: Option<usize>, minimum_speed: f32, grace_time: i32, bank: i32, sample: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    let vehicle = vehicle.ok_or_else(|| rt_err("Bad argument #1 - Attempt to set bomb car to a nil value"))?;
    if st.logic.actors.definitions[vehicle].type_id != 3 {
        return Err(rt_err("Bad argument #1 - The inputed actor is not a Vehicle type actor"));
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let out = st.create_node(122, note, color, 0, |st| {
        vec![
            st.prop("pVehicle", 7, PropValue::Int(vehicle as i32)),
            st.prop("pMinimumSpeed", 2, PropValue::Float(minimum_speed)),
            st.prop("pGraceTime", 1, PropValue::Int(grace_time)),
            st.prop("pWarningAudio", 11, PropValue::Audio { bank, sample }),
        ]
    });
    // make the mission load the inputed audio bank
    let _ = st.logic.sound_banks.append_unique(bank);
    Ok(out)
}

// ---------------------------------------------------------------- ActorHasReachedPoint

/// ActorHasReachedPoint: creates a volume actor (type 4) at (x, z) with the given radius, then a node (type 12)
/// that checks whether `actor` is inside it. Returns (node index, wire collection index).
pub fn actor_has_reached_point(
    st: &mut ScriptState,
    actor: Option<usize>,
    x: f32,
    // the C# volume uses x, z and radius only; y is read by the Lua call but never stored
    _y: f32,
    z: f32,
    radius: f32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    let actor = actor.ok_or_else(|| rt_err("Bad argument #1 - Attempt to check point from a nil value"))?;
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    // note string first, then the node's wire collection (the node itself is pushed last)
    let string_id = st.note_id(note);
    let wire = st.new_wire_collection();

    // test volume actor (its strings come before the node's pWireCollection string)
    let test_volume_idx = st.logic.actors.definitions.len();
    let volume_object_id = st.objects.len() as i32;
    st.create_actor(4, note, color, 0x72D5, volume_object_id, |st| {
        vec![
            st.prop("pAttachTo", 7, PropValue::Int(-1)),
            st.prop("pRadius", 2, PropValue::Float(radius)),
            st.prop("pFlags", 9, PropValue::Int(1)),
        ]
    });
    // the volume's position fields hold x, z and the radius (in x, y and z): the y value is not stored
    st.objects.push(MissionObject::Volume { reserved: 0, position: Vec3 { x, y: z, z: radius } });

    let mut props = vec![st.prop("pWireCollection", 19, PropValue::Int(wire as i32))];
    props.push(st.prop("pArea", 7, PropValue::Int(test_volume_idx as i32)));
    props.push(st.prop("pActor", 7, PropValue::Int(actor as i32)));
    props.push(st.prop("pFlags", 9, PropValue::Int(flags)));
    let mut def = NodeDef::new(12, string_id);
    def.color = color;
    def.properties = props;
    st.logic.nodes.definitions.push(def);
    let idx = st.logic.nodes.definitions.len() - 1;
    Ok((idx, wire))
}

// ---------------------------------------------------------------- PlayAudio

/// PlayAudio: creates a PlayAudio node (type 27) for the audio bank and sample, optionally for an actor,
/// and adds the bank to the sound bank load list. Returns (node index, wire collection index).
pub fn play_audio(st: &mut ScriptState, bank: i32, sample: i32, actor: Option<usize>, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let actor_i = actor.map(|v| v as i32).unwrap_or(-1);
    let out = st.create_node(27, note, color, 0, |st| {
        vec![
            st.prop("pAudio", 11, PropValue::Audio { bank, sample }),
            st.prop("pActor", 7, PropValue::Int(actor_i)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    // make the mission load the inputed audio bank
    let _ = st.logic.sound_banks.append_unique(bank);
    Ok(out)
}

// ---------------------------------------------------------------- CharacterDriveTo family

/// CharacterDriveTo: adds an AI target actor (type 7) plus a volume object, then returns an ActorCreation node.
pub fn character_drive_to(
    st: &mut ScriptState,
    character: Option<usize>,
    x: f32,
    y: f32,
    z: f32,
    drive_speed: f32,
    desired_transport: i32,
    attach_to: Option<usize>,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    // C# evaluates character.TheActor first, so a nil character is a NullReferenceException
    let character = character.ok_or_else(nre)?;
    if st.logic.actors.definitions[character].type_id != 2 {
        return Err(rt_err("Bad argument #1 - The inputed actor is not a Character type actor"));
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    // the C# wire collection is added (and never referenced again)
    let _unused_wire = st.new_wire_collection();
    let ch = character as i32;
    let object_id = st.objects.len() as i32;
    st.create_actor(7, note, color, 0, object_id, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(ch)),
            // literal port: the C# uses character.index here, not attachTo.index
            st.prop("pAttachTo", 7, PropValue::Int(attach_to.map(|_| ch).unwrap_or(-1))),
            st.prop("pWalkSpeed", 2, PropValue::Float(1.0)),
            st.prop("pVehicleSpeed", 2, PropValue::Float(drive_speed)),
            st.prop("pDesiredTransport", 6, PropValue::Int(desired_transport)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    st.objects.push(MissionObject::Volume { reserved: 0, position: Vec3 { x, y, z } });
    let idx = st.logic.actors.definitions.len() - 1;
    actor_creation(st, Some(idx), 1, 0, None, 0, 200, 122)
}

/// CharacterDriveAroundArea: adds a character AI actor (type 7) whose object id is the area's, then returns
/// an ActorCreation node. `area` must be an Area actor (type 4).
pub fn character_drive_around_area(
    st: &mut ScriptState,
    character: Option<usize>,
    area: Option<usize>,
    drive_speed: f32,
    desired_transport: i32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    let character = character.ok_or_else(nre)?;
    if st.logic.actors.definitions[character].type_id != 2 {
        return Err(rt_err("Bad argument #1 - The inputed actor is not a Character type actor"));
    }
    let area = area.ok_or_else(nre)?;
    if st.logic.actors.definitions[area].type_id != 4 {
        return Err(rt_err("Bad argument #2 - The inputed actor is not a Area type actor"));
    }
    let color = node_color(r as f64, g as f64, b as f64, 255.0);
    let _unused_wire = st.new_wire_collection();
    let object_id = st.logic.actors.definitions[area].object_id;
    let ch = character as i32;
    st.create_actor(7, note, color, 0, object_id, |st| {
        vec![
            st.prop("pCharacter", 7, PropValue::Int(ch)),
            st.prop("pAttachTo", 7, PropValue::Int(-1)),
            st.prop("pWalkSpeed", 2, PropValue::Float(1.0)),
            st.prop("pVehicleSpeed", 2, PropValue::Float(drive_speed)),
            st.prop("pDesiredTransport", 6, PropValue::Int(desired_transport)),
            st.prop("pFlags", 9, PropValue::Int(flags)),
        ]
    });
    let idx = st.logic.actors.definitions.len() - 1;
    actor_creation(st, Some(idx), 1, 0, None, 0, 200, 122)
}

/// CharacterDriveFollowingActor: character_drive_to with a zero position and `actor` as the attached actor.
pub fn character_drive_following_actor(
    st: &mut ScriptState,
    character: Option<usize>,
    actor: Option<usize>,
    drive_speed: f32,
    desired_transport: i32,
    flags: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    character_drive_to(st, character, 0.0, 0.0, 0.0, drive_speed, desired_transport, actor, flags, note, r, g, b)
}

/// CharacterChaseActor: character_drive_following_actor with transport 2. The flags depend on `chaser_type`:
/// 1 and 2 select two fixed flag values, any other value selects a third.
pub fn character_chase_actor(
    st: &mut ScriptState,
    character: Option<usize>,
    actor: Option<usize>,
    drive_speed: f32,
    chaser_type: i32,
    note: Option<&str>,
    r: i32,
    g: i32,
    b: i32,
) -> NodeOut {
    let flags = match chaser_type {
        1 => 8912896,
        2 => 9961472,
        _ => 25690112,
    };
    character_drive_following_actor(st, character, actor, drive_speed, 2, flags, note, r, g, b)
}

// ---------------------------------------------------------------- clocks

/// ShowClock: CountdownIntro(defaultTime, 2, flags = 1, note, r, g, b). The C# assigns 1 to flags, so the
/// Lua `flags` argument never reaches the node.
pub fn show_clock(st: &mut ScriptState, default_time: i32, _flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    countdown_intro(st, default_time, 2, 1, note.map(|s| s.to_string()), r, g, b)
}

/// StartClock: countdown node with default time 0. The command is 4 when `direction` is 0, otherwise 1.
pub fn start_clock(st: &mut ScriptState, direction: u32, flags: i32, note: Option<&str>, r: i32, g: i32, b: i32) -> NodeOut {
    let command = if direction == 0 { 4 } else { 1 };
    countdown_intro(st, 0, command, flags, note.map(|s| s.to_string()), r, g, b)
}

// ---------------------------------------------------------------- registration

/// Registers a Lua method `name` that calls `f` on the mission state and returns the node through ret_node.
fn reg_node(
    lua: &Lua,
    t: &Table,
    st: &State,
    name: &str,
    f: impl Fn(&mut ScriptState, &Args) -> NodeOut + 'static,
) -> LuaResult<()> {
    reg(lua, t, st, name, move |lua, st, a| {
        let (idx, wire) = {
            let mut s = st.borrow_mut();
            f(&mut *s, a)?
        };
        ret_node(lua, st, idx, wire)
    })
}

/// Optional note argument with a C# default.
fn note_arg(a: &Args, i: usize, def: &str) -> LuaResult<Option<String>> {
    a.string(i, Some(Some(def)))
}

/// (character, amount, flags = 0, note, r = 200, g = 255, b = 100)
fn reg_amount(lua: &Lua, t: &Table, st: &State, name: &str, default_note: &'static str, f: AmountFn) -> LuaResult<()> {
    reg_node(lua, t, st, name, move |s, a| {
        let note = note_arg(a, 3, default_note)?;
        f(
            s,
            a.actor(0)?,
            a.float(1, None)?,
            a.int(2, Some(0))?,
            note.as_deref(),
            a.int(4, Some(200))?,
            a.int(5, Some(255))?,
            a.int(6, Some(100))?,
        )
    })
}

/// (character, flags = 0, note, r = 200, g = 255, b = 100)
fn reg_flags(lua: &Lua, t: &Table, st: &State, name: &str, default_note: &'static str, f: FlagsFn) -> LuaResult<()> {
    reg_node(lua, t, st, name, move |s, a| {
        let note = note_arg(a, 2, default_note)?;
        f(
            s,
            a.actor(0)?,
            a.int(1, Some(0))?,
            note.as_deref(),
            a.int(3, Some(200))?,
            a.int(4, Some(255))?,
            a.int(5, Some(100))?,
        )
    })
}

/// Adds the methods of this file to the Lua table `t`. Does nothing when `dpl` is true, because the DPL
/// class has its own method set.
pub fn register(lua: &Lua, t: &Table, st: &State, dpl: bool) -> LuaResult<()> {
    // LuaMissionScript methods only; the DPL class has its own surface.
    if dpl {
        return Ok(());
    }

    reg_node(lua, t, st, "CharacterNameControl", |s, a| {
        let character = a.actor(0)?;
        // a nil name is ported as an empty string (the C# behaviour for null is unverified)
        let name = a.string(1, None)?.ok_or_else(crate::api_manual::nre)?;
        let show_name = a.boolean(2, Some(true))?;
        let player_num = a.int(3, Some(1))?;
        let note = note_arg(a, 4, "")?;
        character_name_control(
            s,
            character,
            &name,
            show_name,
            player_num,
            note.as_deref(),
            a.int(5, Some(0))?,
            a.int(6, Some(200))?,
            a.int(7, Some(122))?,
        )
    })?;

    reg_node(lua, t, st, "CharacterControl", |s, a| {
        let character = a.actor(0)?;
        let property = a.int(1, None)?;
        let value = a.float(2, None)?;
        let weapon = a.int(3, Some(0))?;
        let personality_index = a.int(4, Some(-1))?;
        let vehicle = a.actor(5)?;
        let target = a.actor(6)?;
        // nil strings are ported as "" (unverified against C# null handling)
        let animation = a.string(7, Some(Some("")))?.ok_or_else(crate::api_manual::nre)?;
        let anim_action = a.short(8, Some(-1))?;
        let pad_file = a.string(9, Some(Some("")))?.ok_or_else(crate::api_manual::nre)?;
        let num_pad = a.int(10, Some(0))?;
        let flags = a.int(11, Some(0))?;
        let note = note_arg(a, 12, "")?;
        character_control(
            s,
            character,
            property,
            value,
            weapon,
            personality_index,
            vehicle,
            target,
            &animation,
            anim_action,
            &pad_file,
            num_pad,
            flags,
            note.as_deref(),
            a.int(13, Some(0))?,
            a.int(14, Some(200))?,
            a.int(15, Some(122))?,
        )
    })?;

    reg_amount(lua, t, st, "SetCharacterTakeHealth", "Character.TakeHealth()", set_character_take_health)?;
    reg_amount(lua, t, st, "SetCharacterAddHealth", "Character.AddHealth()", set_character_add_health)?;
    reg_amount(lua, t, st, "SetCharacterHealthTo", "Character.SetHealthTo()", set_character_health_to)?;
    reg_amount(lua, t, st, "SetCharacterAddFelony", "Character.AddFelony()", set_character_add_felony)?;
    reg_amount(lua, t, st, "SetCharacterTakeFelony", "Character.TakeFelony()", set_character_take_felony)?;
    reg_amount(lua, t, st, "SetCharacterFelonyTo", "Character.SetFelonyTo()", set_character_felony_to)?;
    reg_amount(lua, t, st, "SetCharacterVulnerability", "Character.SetVulnerability()", set_character_vulnerability)?;

    reg_node(lua, t, st, "SetCharacterUsePadFile", |s, a| {
        let character = a.actor(0)?;
        let vehicle = a.actor(1)?;
        let file_name = a.string(2, None)?.ok_or_else(crate::api_manual::nre)?;
        let num_recordings = a.int(3, None)?;
        let flags = a.int(4, Some(0))?;
        let note = note_arg(a, 5, "Character.UsePadFile()")?;
        set_character_use_pad_file(
            s,
            character,
            vehicle,
            &file_name,
            num_recordings,
            flags,
            note.as_deref(),
            a.int(6, Some(200))?,
            a.int(7, Some(255))?,
            a.int(8, Some(100))?,
        )
    })?;

    reg_node(lua, t, st, "GiveCharacterWeapon", |s, a| {
        let character = a.actor(0)?;
        let weapon = a.int(1, None)?;
        let clips = a.float(2, None)?;
        let flags = a.int(3, Some(0))?;
        let note = note_arg(a, 4, "Character.GiveWeapon()")?;
        give_character_weapon(s, character, weapon, clips, flags, note.as_deref(), a.int(5, Some(200))?, a.int(6, Some(255))?, a.int(7, Some(100))?)
    })?;

    reg_flags(lua, t, st, "SetCharacterUnStuckInVehicle", "Character.SetUnStuckInVehicle()", set_character_unstuck_in_vehicle)?;
    reg_flags(lua, t, st, "SetCharacterStuckInVehicle", "Character.SetStuckInVehicle()", set_character_stuck_in_vehicle)?;
    reg_flags(lua, t, st, "SetCharacterStopAnimation", "Character.StopAnimation()", set_character_stop_animation)?;

    reg_node(lua, t, st, "SetCharacterPlayAnimation", |s, a| {
        let character = a.actor(0)?;
        let animation_name = a.string(1, None)?.ok_or_else(crate::api_manual::nre)?;
        let animation_id = a.short(2, None)?;
        let flags = a.int(3, Some(0))?;
        let note = note_arg(a, 4, "Character.PlayAnimation()")?;
        set_character_play_animation(
            s,
            character,
            &animation_name,
            animation_id,
            flags,
            note.as_deref(),
            a.int(5, Some(200))?,
            a.int(6, Some(255))?,
            a.int(7, Some(100))?,
        )
    })?;

    reg_node(lua, t, st, "SetCharacterPersonalityIndexTo", |s, a| {
        let character = a.actor(0)?;
        let personality = a.int(1, None)?;
        let flags = a.int(2, Some(0))?;
        let note = note_arg(a, 3, "Character.SetPersonalityIndexTo()")?;
        set_character_personality_index_to(s, character, personality, flags, note.as_deref(), a.int(4, Some(200))?, a.int(5, Some(255))?, a.int(6, Some(100))?)
    })?;

    reg_flags(lua, t, st, "ClearCharacterWeapons", "Character.Disarm()", clear_character_weapons)?;
    reg_flags(lua, t, st, "SetCharacterGetOutOfVehicle", "Character.GetOutOfVehicle()", set_character_get_out_of_vehicle)?;
    reg_flags(lua, t, st, "CharacterEquipWeapon", "Character.EquipWeapon()", character_equip_weapon)?;
    reg_flags(lua, t, st, "CharacterReholsterWeapon", "Character.ReholsterWeapon()", character_reholster_weapon)?;

    reg_node(lua, t, st, "FadeScreen", |s, a| {
        let duration = a.float(0, None)?;
        let fade_to = a.float(1, None)?;
        let fade_from = a.float(2, Some(0.0))?;
        let red = a.byte(3, Some(0))?;
        let green = a.byte(4, Some(0))?;
        let blue = a.byte(5, Some(0))?;
        let alpha = a.byte(6, Some(0))?;
        let note = note_arg(a, 7, "")?;
        fade_screen(s, duration, fade_to, fade_from, red, green, blue, alpha, note.as_deref(), a.int(8, Some(0))?, a.int(9, Some(200))?, a.int(10, Some(122))?)
    })?;

    reg_node(lua, t, st, "FadeOut", |s, a| {
        let duration = a.float(0, Some(0.3))?;
        let red = a.byte(1, Some(0))?;
        let green = a.byte(2, Some(0))?;
        let blue = a.byte(3, Some(0))?;
        let alpha = a.byte(4, Some(0))?;
        let note = note_arg(a, 5, "FadeOut()")?;
        fade_out(s, duration, red, green, blue, alpha, note.as_deref(), a.int(6, Some(255))?, a.int(7, Some(200))?, a.int(8, Some(128))?)
    })?;

    reg_node(lua, t, st, "FadeIn", |s, a| {
        let duration = a.float(0, Some(0.3))?;
        let red = a.byte(1, Some(0))?;
        let green = a.byte(2, Some(0))?;
        let blue = a.byte(3, Some(0))?;
        let alpha = a.byte(4, Some(0))?;
        let note = note_arg(a, 5, "FadeIn()")?;
        fade_in(s, duration, red, green, blue, alpha, note.as_deref(), a.int(6, Some(255))?, a.int(7, Some(200))?, a.int(8, Some(128))?)
    })?;

    reg_node(lua, t, st, "BombCar", |s, a| {
        let vehicle = a.actor(0)?;
        let minimum_speed = a.float(1, Some(22.36))?;
        let grace_time = a.int(2, Some(5))?;
        let bank = a.int(3, Some(45))?;
        let sample = a.int(4, Some(0))?;
        let note = note_arg(a, 5, "")?;
        bomb_car(s, vehicle, minimum_speed, grace_time, bank, sample, note.as_deref(), a.int(6, Some(0))?, a.int(7, Some(200))?, a.int(8, Some(122))?)
    })?;

    reg_node(lua, t, st, "ActorHasReachedPoint", |s, a| {
        let actor = a.actor(0)?;
        let x = a.float(1, None)?;
        let y = a.float(2, None)?;
        let z = a.float(3, None)?;
        let radius = a.float(4, None)?;
        let flags = a.int(5, Some(1))?;
        let note = note_arg(a, 6, "ActorHasReachedPoint")?;
        actor_has_reached_point(s, actor, x, y, z, radius, flags, note.as_deref(), a.int(7, Some(100))?, a.int(8, Some(128))?, a.int(9, Some(255))?)
    })?;

    reg_node(lua, t, st, "PlayAudio", |s, a| {
        let bank = a.int(0, None)?;
        let sample = a.int(1, None)?;
        let actor = a.actor(2)?;
        let flags = a.int(3, Some(1))?;
        let note = note_arg(a, 4, "")?;
        play_audio(s, bank, sample, actor, flags, note.as_deref(), a.int(5, Some(100))?, a.int(6, Some(0))?, a.int(7, Some(255))?)
    })?;

    reg_node(lua, t, st, "CharacterDriveTo", |s, a| {
        let character = a.actor(0)?;
        let x = a.float(1, None)?;
        let y = a.float(2, None)?;
        let z = a.float(3, None)?;
        let drive_speed = a.float(4, None)?;
        let transport = a.int(5, Some(2))?;
        let attach_to = a.actor(6)?;
        let flags = a.int(7, Some(0))?;
        let note = note_arg(a, 8, "CharacterDriveTo")?;
        character_drive_to(s, character, x, y, z, drive_speed, transport, attach_to, flags, note.as_deref(), a.int(9, Some(128))?, a.int(10, Some(0))?, a.int(11, Some(255))?)
    })?;

    reg_node(lua, t, st, "CharacterDriveAroundArea", |s, a| {
        let character = a.actor(0)?;
        let area = a.actor(1)?;
        let drive_speed = a.float(2, None)?;
        let transport = a.int(3, Some(2))?;
        let flags = a.int(4, Some(0))?;
        let note = note_arg(a, 5, "CharacterDriveTo")?;
        character_drive_around_area(s, character, area, drive_speed, transport, flags, note.as_deref(), a.int(6, Some(128))?, a.int(7, Some(0))?, a.int(8, Some(255))?)
    })?;

    reg_node(lua, t, st, "CharacterChaseActor", |s, a| {
        let character = a.actor(0)?;
        let actor = a.actor(1)?;
        let drive_speed = a.float(2, None)?;
        let chaser_type = a.int(3, Some(0))?;
        let note = note_arg(a, 4, "CharacterDriveToActor")?;
        character_chase_actor(s, character, actor, drive_speed, chaser_type, note.as_deref(), a.int(5, Some(128))?, a.int(6, Some(0))?, a.int(7, Some(255))?)
    })?;

    reg_node(lua, t, st, "CharacterDriveFollowingActor", |s, a| {
        let character = a.actor(0)?;
        let actor = a.actor(1)?;
        let drive_speed = a.float(2, None)?;
        let transport = a.int(3, Some(2))?;
        let flags = a.int(4, Some(0))?;
        let note = note_arg(a, 5, "CharacterDriveFollowingActor")?;
        character_drive_following_actor(s, character, actor, drive_speed, transport, flags, note.as_deref(), a.int(6, Some(128))?, a.int(7, Some(0))?, a.int(8, Some(255))?)
    })?;

    reg_node(lua, t, st, "ShowClock", |s, a| {
        let default_time = a.int(0, None)?;
        let flags = a.int(1, Some(1))?;
        let note = note_arg(a, 2, "")?;
        show_clock(s, default_time, flags, note.as_deref(), a.int(3, Some(255))?, a.int(4, Some(255))?, a.int(5, Some(0))?)
    })?;

    reg_node(lua, t, st, "StartClock", |s, a| {
        let direction = a.uint(0, Some(0))?;
        let flags = a.int(1, Some(0))?;
        let note = note_arg(a, 2, "")?;
        start_clock(s, direction, flags, note.as_deref(), a.int(3, Some(255))?, a.int(4, Some(101))?, a.int(5, Some(200))?)
    })?;

    Ok(())
}
