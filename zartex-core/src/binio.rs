//! Little-endian cursor reader / writer with .NET-stream-like semantics.
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the file container reader/writer and the mission model).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//!
//! This file holds the cursor types `Reader` (reads little-endian values from a byte slice) and `Writer`
//! (appends little-endian values to a `Vec<u8>`), and the float vector types `Vec2`, `Vec3` and `Vec4`
//! that both of them read and write. It also defines the error type `ModelError` and the result alias `Res<T>`.
//! Uses `crate::chunk::ChunkError` (wrapped by `ModelError::Chunk`) and the `thiserror` crate for the error derive.
//! Used by: `crate::logic`, `crate::objects`, `crate::mission`, `crate::dpl`, `crate::script`, `crate::luaraw`,
//! `crate::readable`, `crate::compile`, several `api_*` modules of this crate, and by `zartex-app`
//! (`model`, `camera` and `view3d`).

use crate::chunk::ChunkError;
use thiserror::Error;

// ---------------------------------------------------------------- errors
/// Error of the model readers and writers: end of data, invalid content, or an error of the chunk layer.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("unexpected end of data at {0:#x}")]
    Eof(usize),
    #[error("{0}")]
    Invalid(String),
    #[error("chunk: {0}")]
    Chunk(#[from] ChunkError),
}

/// Result type of the model readers and writers.
pub type Res<T> = Result<T, ModelError>;

/// Returns `Err(ModelError::Invalid(msg))` with the given message.
pub fn invalid<T>(msg: impl Into<String>) -> Res<T> {
    Err(ModelError::Invalid(msg.into()))
}

// ---------------------------------------------------------------- vectors
/// A vector of two f32 values, `x` then `y`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
/// A vector of three f32 values, `x`, `y` then `z`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
/// A vector of four f32 values, `x`, `y`, `z` then `w`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

// ---------------------------------------------------------------- reader
/// Cursor over a byte slice. `d` is the data and `pos` is the offset of the next byte to read.
pub struct Reader<'a> {
    pub d: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    /// Creates a reader positioned at offset 0 of `d`.
    pub fn new(d: &'a [u8]) -> Self {
        Reader { d, pos: 0 }
    }
    /// Returns the next `n` bytes and moves past them. Fails with `Eof` when fewer than `n` bytes are left.
    pub fn bytes(&mut self, n: usize) -> Res<&'a [u8]> {
        let s = self.d.get(self.pos..self.pos + n).ok_or(ModelError::Eof(self.pos))?;
        self.pos += n;
        Ok(s)
    }
    /// Reads one unsigned byte.
    pub fn u8(&mut self) -> Res<u8> {
        Ok(self.bytes(1)?[0])
    }
    /// Reads a little-endian i16.
    pub fn i16(&mut self) -> Res<i16> {
        Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    /// Reads a little-endian u16.
    pub fn u16(&mut self) -> Res<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    /// Reads a little-endian i32.
    pub fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    /// Reads a little-endian u32.
    pub fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    /// Reads a little-endian u64.
    pub fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    /// Reads a little-endian i64.
    pub fn i64(&mut self) -> Res<i64> {
        Ok(i64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    /// Reads a little-endian f32.
    pub fn f32(&mut self) -> Res<f32> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    /// Reads two f32 values as a `Vec2`.
    pub fn vec2(&mut self) -> Res<Vec2> {
        Ok(Vec2 { x: self.f32()?, y: self.f32()? })
    }
    /// Reads three f32 values as a `Vec3`.
    pub fn vec3(&mut self) -> Res<Vec3> {
        Ok(Vec3 { x: self.f32()?, y: self.f32()?, z: self.f32()? })
    }
    /// Reads four f32 values as a `Vec4`.
    pub fn vec4(&mut self) -> Res<Vec4> {
        Ok(Vec4 { x: self.f32()?, y: self.f32()?, z: self.f32()?, w: self.f32()? })
    }
    /// Stream.Align(): move the position up to the next multiple of `a`.
    pub fn align(&mut self, a: usize) {
        if a != 0 {
            self.pos += (a - (self.pos % a)) % a;
        }
    }
    /// Returns the bytes from the current position to the end of the data and moves to the end.
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.d[self.pos.min(self.d.len())..];
        self.pos = self.d.len();
        s
    }
}
// ---------------------------------------------------------------- writer

/// Growable buffer that appends little-endian values to `b`.
#[derive(Default)]
pub struct Writer {
    pub b: Vec<u8>,
}

impl Writer {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Writer { b: Vec::new() }
    }
    /// Returns the current length of the buffer, which is the offset of the next write.
    pub fn pos(&self) -> usize {
        self.b.len()
    }
    /// Appends one byte.
    pub fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    /// Appends the raw bytes of `v`.
    pub fn bytes(&mut self, v: &[u8]) {
        self.b.extend_from_slice(v);
    }
    /// Appends `v` as a little-endian i16.
    pub fn i16(&mut self, v: i16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends `v` as a little-endian u16.
    pub fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends `v` as a little-endian i32.
    pub fn i32(&mut self, v: i32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends `v` as a little-endian u32.
    pub fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends `v` as a little-endian u64.
    pub fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends `v` as a little-endian i64.
    pub fn i64(&mut self, v: i64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends `v` as a little-endian f32.
    pub fn f32(&mut self, v: f32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    /// Appends the two components of `v` as f32 values.
    pub fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }
    /// Appends the three components of `v` as f32 values.
    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    /// Appends the four components of `v` as f32 values.
    pub fn vec4(&mut self, v: Vec4) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
        self.f32(v.w);
    }
}
