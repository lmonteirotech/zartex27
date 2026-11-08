//! Readers for the game data formats (chunk container, model packages, city, vehicle hierarchies, road network, textures).
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - the module list is trimmed: the original index had more modules, which were not needed here
//!
//! Everything is read from the user's own installation; nothing from the game is bundled here.
//! The modules are `aiex`, `bytes` (shared little-endian reads), `chnk`, `d3c`, `dds`, `mdpc` and `vehicle`.

pub mod aiex;
pub mod bytes;
pub mod chnk;
pub mod d3c;
pub mod dds;
pub mod mdpc;
pub mod vehicle;
