//! Minimal DDS header reader for the textures the game ships: DXT1, DXT3, DXT5 and uncompressed 32-bit BGRA, with a full mip chain.
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! Exposes `parse`, `Dds` and `Format`. It uses no other module of this repository.
//! Used by the `zartex-app` city view (`city3d.rs`), which imports `dds` and `Format`.

/// Pixel format of a texture: `Bc1`, `Bc2` and `Bc3` (block-compressed DXT1, DXT3 and DXT5), or `Bgra8` (uncompressed,
/// 32 bits per pixel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc2,
    Bc3,
    Bgra8,
}

impl Format {
    /// True for the block-compressed formats (`Bc1`, `Bc2`, `Bc3`), false for `Bgra8`.
    pub fn is_block(self) -> bool {
        !matches!(self, Format::Bgra8)
    }
    /// Bytes of one mip level of the given size. Block formats store 4x4 texel blocks of 8 bytes (BC1) or 16 bytes (BC2
    /// and BC3), so the size rounds up to whole blocks. A zero dimension counts as 1.
    pub fn level_size(self, w: u32, h: u32) -> usize {
        let (w, h) = (w.max(1) as usize, h.max(1) as usize);
        match self {
            Format::Bc1 => w.div_ceil(4) * h.div_ceil(4) * 8,
            Format::Bc2 | Format::Bc3 => w.div_ceil(4) * h.div_ceil(4) * 16,
            Format::Bgra8 => w * h * 4,
        }
    }
}

/// A parsed DDS header and the data of all its mip levels, stored one after another, largest first.
#[derive(Debug)]
pub struct Dds<'a> {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub format: Format,
    /// All mip levels, largest first, tightly packed.
    pub data: &'a [u8],
}

/// Reads the 128-byte DDS header and returns the mip data. `None` when the file is shorter than 128 bytes, has no `DDS `
/// magic, has a FourCC other than DXT1, DXT3 or DXT5, has neither a FourCC nor a 32-bit pixel size, or is shorter than its
/// mip levels need. A mip count of 0 is read as 1.
pub fn parse(b: &[u8]) -> Option<Dds<'_>> {
    if b.len() < 128 || &b[0..4] != b"DDS " {
        return None;
    }
    let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let height = u(12);
    let width = u(16);
    let mips = u(28).max(1);
    let pf_flags = u(80);
    let four_cc = &b[84..88];
    // bit 2 of the pixel-format flags says that the format is a FourCC code; otherwise the pixel size must be 32 bits
    let format = if pf_flags & 4 != 0 {
        match four_cc {
            b"DXT1" => Format::Bc1,
            b"DXT3" => Format::Bc2,
            b"DXT5" => Format::Bc3,
            _ => return None,
        }
    } else if u(88) == 32 {
        Format::Bgra8
    } else {
        return None;
    };
    // total bytes of all mip levels, halving each dimension and keeping it at least 1
    let mut need = 0;
    let (mut w, mut h) = (width, height);
    for _ in 0..mips {
        need += format.level_size(w, h);
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    let data = b.get(128..128 + need)?;
    Some(Dds { width, height, mips, format, data })
}
