//! `CHNK` container reader: a 16-byte header, then `count` 16-byte entries that point at payloads inside the file.
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! Header: magic `CHNK`, size, count, version (bit 31 = compressed; the flag is read, payloads are not decompressed here).
//! Entry: ctx u32, off u32, version u8, desc_len u8, align_log2 u8, reserved u8,
//! size u32. The optional description string sits right after the payload.
//!
//! Exposes `Chunk` (parse a container), `Entry` (one entry: tag, payload, description) and `Error`.
//! Uses `bytes` for the little-endian reads. Used by `aiex`, `d3c`, `mdpc` and `vehicle`, and by the `zartex-app` crate
//! (city, character, vehicle and asset loaders, and the `d4cdump` and `mdxnprobe` tools).

use std::fmt;

const MAGIC: &[u8; 4] = b"CHNK";
const MAX_ENTRIES: u32 = 100_000;

/// Errors from reading a container: too short, wrong magic, more entries than `MAX_ENTRIES`, or an entry whose payload
/// lies outside the file.
#[derive(Debug)]
pub enum Error {
    Truncated,
    BadMagic,
    TooManyEntries(u32),
    OutOfRange { off: usize, size: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => write!(f, "truncated CHNK"),
            Error::BadMagic => write!(f, "not a CHNK"),
            Error::TooManyEntries(n) => write!(f, "implausible entry count {n}"),
            Error::OutOfRange { off, size } => {
                write!(f, "entry at 0x{off:X} size 0x{size:X} outside container")
            }
        }
    }
}

impl std::error::Error for Error {}

/// One entry of a container: its context code, version, alignment, payload bytes and optional description.
#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub ctx: u32,
    pub version: u8,
    /// Alignment of the payload as a power of two (in the PC files only 11 = 2048 and 12 = 4096 appear).
    /// [comunidade: driver-tools/DSCript/Spooling/SpoolablePackage.cs]
    pub align_log2: u8,
    pub data: &'a [u8],
    pub desc: String,
}

impl<'a> Entry<'a> {
    /// Four-character code of the context (the bytes of `ctx` in order, non-printable bytes shown as `?`), e.g. `GEIR`.
    pub fn tag(&self) -> String {
        self.ctx.to_le_bytes().iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '?' }).collect()
    }

    /// True when the payload starts with the `CHNK` magic, so it can be parsed as a nested container.
    pub fn is_chnk(&self) -> bool {
        self.data.starts_with(MAGIC)
    }

    /// Parses the payload as a nested container.
    pub fn children(&self) -> Result<Chunk<'a>, Error> {
        Chunk::parse(self.data)
    }
}

/// A parsed container: its version (bit 31 removed), the compression flag (bit 31), and its entries.
#[derive(Debug, Clone)]
pub struct Chunk<'a> {
    pub version: u32,
    pub compressed: bool,
    pub entries: Vec<Entry<'a>>,
}

/// Reads a little-endian `u32` at `o`; `Truncated` when the slice is too short.
fn u32_at(b: &[u8], o: usize) -> Result<u32, Error> {
    crate::bytes::u32_le(b, o).ok_or(Error::Truncated)
}

impl<'a> Chunk<'a> {
    /// Parses a container. Fails with `Truncated` when shorter than 16 bytes, `BadMagic` when it does not start with `CHNK`,
    /// `TooManyEntries` above `MAX_ENTRIES`, and `OutOfRange` when an entry's payload does not fit in the buffer.
    pub fn parse(buf: &'a [u8]) -> Result<Self, Error> {
        if buf.len() < 16 {
            return Err(Error::Truncated);
        }
        if &buf[0..4] != MAGIC {
            return Err(Error::BadMagic);
        }
        let count = u32_at(buf, 8)?;
        let ver = u32_at(buf, 12)?;
        if count > MAX_ENTRIES {
            return Err(Error::TooManyEntries(count));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let e = 16 + i * 16;
            let ctx = u32_at(buf, e)?;
            let off = u32_at(buf, e + 4)? as usize;
            let (version, slen, align_log2) = {
                let b = buf.get(e + 8..e + 12).ok_or(Error::Truncated)?;
                (b[0], b[1] as usize, b[2])
            };
            let size = u32_at(buf, e + 12)? as usize;
            let data = buf.get(off..off.checked_add(size).ok_or(Error::Truncated)?).ok_or(Error::OutOfRange { off, size })?;
            // the description is a NUL-terminated string stored in the `slen` bytes right after the payload
            let desc = if slen > 0 {
                let raw = buf.get(off + size..off + size + slen).unwrap_or(&[]);
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                raw[..end].iter().map(|&b| b as char).collect()
            } else {
                String::new()
            };
            entries.push(Entry { ctx, version, align_log2, data, desc });
        }
        Ok(Chunk { version: ver & 0x7FFF_FFFF, compressed: ver & 0x8000_0000 != 0, entries })
    }

    /// Entries whose four-character tag equals `tag`. Only the entries of this container are searched, not nested ones.
    pub fn find<'s>(&'s self, tag: &'s str) -> impl Iterator<Item = &'s Entry<'a>> + 's {
        self.entries.iter().filter(move |e| e.tag() == tag)
    }

    /// Total number of entries including those of nested containers.
    pub fn count_entries(&self) -> usize {
        self.entries.len() + self.entries.iter().filter(|e| e.is_chnk()).filter_map(|e| e.children().ok()).map(|c| c.count_entries()).sum::<usize>()
    }

    /// Total number of CHNK containers including nested ones (this one counts).
    pub fn count_chunks(&self) -> usize {
        1 + self.entries.iter().filter(|e| e.is_chnk()).filter_map(|e| e.children().ok()).map(|c| c.count_chunks()).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build() -> Vec<u8> {
        // one entry "TEST" with 4 payload bytes and a description "hi"
        let mut v = Vec::new();
        v.extend_from_slice(b"CHNK");
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&3u32.to_le_bytes());
        v.extend_from_slice(b"TEST");
        v.extend_from_slice(&32u32.to_le_bytes());
        v.extend_from_slice(&[7, 3, 4, 0]);
        v.extend_from_slice(&4u32.to_le_bytes());
        v.extend_from_slice(&[1, 2, 3, 4]);
        v.extend_from_slice(b"hi\0");
        v
    }

    #[test]
    fn parses_entry_and_desc() {
        let b = build();
        let c = Chunk::parse(&b).unwrap();
        assert_eq!(c.version, 3);
        assert!(!c.compressed);
        assert_eq!(c.entries.len(), 1);
        let e = &c.entries[0];
        assert_eq!(e.tag(), "TEST");
        assert_eq!(e.data, &[1, 2, 3, 4]);
        assert_eq!(e.desc, "hi");
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(Chunk::parse(b"nope nope nope nope"), Err(Error::BadMagic)));
        assert!(matches!(Chunk::parse(b"CHNK"), Err(Error::Truncated)));
    }
}
