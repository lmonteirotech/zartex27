//! Port of DSCript.Spooling (Spooler / SpoolableBuffer / SpoolablePackage / FileChunker).
//! ORIGIN NOTICE: this file is a conversion, made with the assistance of a language model and checked by a person, of
//! logic that already existed in the earlier tools Zartex and Zartex 2 (the file container reader/writer and the mission model).
//! The logic is not original to this repository. What this repository adjusted, added or corrected in it:
//! - not itemised for this file
//! Layout rules are copied from the C# so output is byte-identical (including the dirty-index logic).
//!
//! This file reads and writes the CHNK container. A container is a tree of packages and buffers. Each node
//! has a 4-byte context tag, a version, an alignment exponent (the block size is `1 << alignment`), an
//! optional description and an offset. `ChunkFile` is the entry point: `load`, `from_bytes`, `to_bytes`
//! and `save`. The file uses only the `thiserror` crate and `std::fs`.
//! Used by: `crate::mission` and `crate::dpl` (the mission and DPL readers), `crate::plmodel`, `crate::binio`
//! (for `ChunkError`), and the integration tests of `zartex-core`.

use thiserror::Error;

/// Magic number of a chunk header: the bytes `CHNK` read as a little-endian i32.
pub const CHNK_MAGIC: i32 = 0x4B4E_4843; // "CHNK"
/// The only chunk format version that is read; other values are rejected.
pub const CHUNK_VERSION: i32 = 3;
/// Filler bytes. A new file is started filled with these, repeated by byte index modulo 4.
pub const PADDING: [u8; 4] = [0xA1, 0xAC, 0x83, 0xDF];

/// Errors from reading or writing a chunk file.
#[derive(Debug, Error)]
pub enum ChunkError {
    #[error("bad magic - cannot load chunk file")]
    BadMagic,
    #[error("unsupported version {0} - cannot load chunk file")]
    BadVersion(i32),
    #[error("compressed (LZW) chunk files cannot be saved")]
    Compressed,
    #[error("unexpected end of data at {0:#x}")]
    Eof(usize),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Rounds `value` up to a multiple of `alignment`. Returns `value` unchanged when `alignment` is 0.
pub fn align(value: i32, alignment: i32) -> i32 {
    if alignment != 0 {
        value + (alignment - (value % alignment)) % alignment
    } else {
        value
    }
}

/// Fields shared by every spooler: the context tag, the version, the alignment exponent, the
/// description, the offset in the file, and the dirty and modified flags.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Common {
    pub context: i32,
    pub version: u8,
    pub alignment: u8, // SpoolerAlignment raw value (default 0xC = 4096)
    pub description: String,
    pub offset: i32,
    pub is_dirty: bool,
    pub is_modified: bool,
}

impl Common {
    /// Creates the common fields for `context` with alignment 0xC (4096 bytes); every other field is zero or empty.
    pub fn new(context: i32) -> Self {
        Common { context, alignment: 0xC, ..Default::default() }
    }
}

/// A leaf node that holds raw bytes.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub c: Common,
    data: Vec<u8>,
}

/// A node that holds child nodes. `size` caches the size computed by the last call to `size()`.
#[derive(Debug, Clone)]
pub struct Package {
    pub c: Common,
    pub children: Vec<Node>,
    size: i32,
}

/// A child of a package: a leaf buffer or a nested package.
#[derive(Debug, Clone)]
pub enum Node {
    Buffer(Buffer),
    Package(Package),
}

impl Buffer {
    /// Creates a buffer with `context`, the description `description` (cut to its first 255 characters) and the bytes `data`.
    pub fn new(context: i32, description: &str, data: Vec<u8>) -> Self {
        let mut c = Common::new(context);
        c.description = description.chars().take(255).collect();
        Buffer { c, data }
    }
    /// Returns the stored bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    /// SetBuffer: marks dirty + modified.
    pub fn set_data(&mut self, data: Vec<u8>) {
        self.data = data;
        self.c.is_dirty = true;
        self.c.is_modified = true;
    }
    /// Returns the length of the stored bytes, as an i32.
    pub fn size(&self) -> i32 {
        self.data.len() as i32
    }
}

impl Package {
    /// Creates an empty package with `context`.
    pub fn new(context: i32) -> Self {
        Package { c: Common::new(context), children: Vec::new(), size: 0 }
    }

    /// Collection.Add / Insert / Remove / Clear all do this.
    pub fn set_dirty_flag(&mut self) {
        self.c.is_dirty = true;
        self.c.is_modified = true;
    }
    /// Appends a child and marks this package dirty and modified.
    pub fn push(&mut self, n: Node) {
        self.children.push(n);
        self.set_dirty_flag();
    }
    /// Removes and returns the child at index `idx`, and marks this package dirty and modified.
    /// Panics when `idx` is out of range.
    pub fn remove(&mut self, idx: usize) -> Node {
        let n = self.children.remove(idx);
        self.set_dirty_flag();
        n
    }

    /// True when this package or any descendant has a change that is not committed yet.
    pub fn are_changes_pending(&self) -> bool {
        self.c.is_modified || self.children.iter().any(|n| n.are_changes_pending())
    }

    /// Clears the modified flag of this package and of every descendant.
    pub fn commit_changes(&mut self) {
        for ch in &mut self.children {
            ch.commit_changes();
        }
        self.c.is_modified = false;
    }

    /// SpoolablePackage.Size getter, including the (re)layout of children offsets.
    pub fn size(&mut self) -> i32 {
        let count = self.children.len();
        let mut size: i32 = 0x10 + (count as i32 * 0x10);

        if count > 0 {
            let mut dirty_index: i32 = -1;

            // A package that is clean and was sized before only needs a new layout from the first child
            // that has pending changes. Otherwise the layout starts again from child 0.
            if !self.c.is_dirty && self.size > 0 {
                for i in 0..count {
                    if self.children[i].are_changes_pending() {
                        dirty_index = i as i32;
                        break;
                    }
                }
            } else {
                dirty_index = 0;
            }

            if dirty_index > -1 {
                let di = dirty_index as usize;
                // The children before `di` keep their offsets. The running size starts after the last of them.
                if di > 0 {
                    let clean = &mut self.children[di - 1];
                    let off = clean.c().offset;
                    let dl = clean.c().description.len() as i32;
                    let sz = clean.size();
                    size = off + sz + dl;
                }
                // Each child is aligned, gets its offset, and then adds its data and its description length.
                for s in di..count {
                    let child = &mut self.children[s];
                    size = align(size, 1i32 << child.c().alignment);
                    child.c_mut().offset = size;
                    let csz = child.size();
                    size += csz + child.c().description.len() as i32;
                }
                // The package size is padded to the alignment of the last child.
                let last_align = 1i32 << self.children[count - 1].c().alignment;
                size = align(size, last_align);
            } else {
                return self.size;
            }
        }
        self.size = size;
        size
    }
}

impl Node {
    /// Returns the common fields of this node.
    pub fn c(&self) -> &Common {
        match self {
            Node::Buffer(b) => &b.c,
            Node::Package(p) => &p.c,
        }
    }
    /// Returns the common fields of this node, mutably.
    pub fn c_mut(&mut self) -> &mut Common {
        match self {
            Node::Buffer(b) => &mut b.c,
            Node::Package(p) => &mut p.c,
        }
    }
    /// Returns the size of this node in bytes. For a package this lays out its children.
    pub fn size(&mut self) -> i32 {
        match self {
            Node::Buffer(b) => b.size(),
            Node::Package(p) => p.size(),
        }
    }
    /// True when this node has a change that is not committed yet (for a package, see `Package::are_changes_pending`).
    pub fn are_changes_pending(&self) -> bool {
        match self {
            Node::Buffer(b) => b.c.is_modified,
            Node::Package(p) => p.are_changes_pending(),
        }
    }
    /// Clears the modified flag of this node (for a package, see `Package::commit_changes`).
    pub fn commit_changes(&mut self) {
        match self {
            Node::Buffer(b) => b.c.is_modified = false,
            Node::Package(p) => p.commit_changes(),
        }
    }
    /// Returns the buffer when this node is a leaf buffer, otherwise None.
    pub fn as_buffer(&self) -> Option<&Buffer> {
        if let Node::Buffer(b) = self { Some(b) } else { None }
    }
    /// Returns the buffer mutably when this node is a leaf buffer, otherwise None.
    pub fn as_buffer_mut(&mut self) -> Option<&mut Buffer> {
        if let Node::Buffer(b) = self { Some(b) } else { None }
    }
    /// Returns the package when this node is a package, otherwise None.
    pub fn as_package(&self) -> Option<&Package> {
        if let Node::Package(p) = self { Some(p) } else { None }
    }
    /// Returns the package mutably when this node is a package, otherwise None.
    pub fn as_package_mut(&mut self) -> Option<&mut Package> {
        if let Node::Package(p) = self { Some(p) } else { None }
    }
}

// ---------------------------------------------------------------- reading

/// Reads a little-endian i32 at byte offset `at`. Fails with `Eof` when the four bytes are not all present.
fn rd_i32(d: &[u8], at: usize) -> Result<i32, ChunkError> {
    d.get(at..at + 4)
        .map(|s| i32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ChunkError::Eof(at))
}

/// FileChunker.ReadChunk. `base` = absolute position of the CHNK header.
fn read_chunk(d: &[u8], base: usize, parent: &mut Package, compressed: &mut bool) -> Result<(), ChunkError> {
    if rd_i32(d, base)? != CHNK_MAGIC {
        return Err(ChunkError::BadMagic);
    }
    let _size = rd_i32(d, base + 4)?;
    let count = rd_i32(d, base + 8)?;
    let mut version = rd_i32(d, base + 12)?;
    // The top bit of the version marks an LZW-compressed file. The bit is cleared before the version check.
    if (version as u32) & 0x8000_0000 != 0 {
        *compressed = true;
        version = ((version as u32) & 0x7FFF_FFFF) as i32;
    }
    if version != CHUNK_VERSION {
        return Err(ChunkError::BadVersion(version));
    }

    // Each table entry is 0x10 bytes: context, offset, version, description length, alignment, size.
    for i in 0..count as usize {
        let e = base + 0x10 + i * 0x10;
        let context = rd_i32(d, e)?;
        let offset = rd_i32(d, e + 4)?;
        let ver = *d.get(e + 8).ok_or(ChunkError::Eof(e + 8))?;
        let strlen = *d.get(e + 9).ok_or(ChunkError::Eof(e + 9))? as usize;
        let alignment = *d.get(e + 10).ok_or(ChunkError::Eof(e + 10))?;
        let size = rd_i32(d, e + 12)?;

        // The description sits right after the child data and is read up to the first zero byte.
        let mut description = String::new();
        if strlen > 0 {
            let p = base + (offset + size) as usize;
            let raw = d.get(p..p + strlen).ok_or(ChunkError::Eof(p))?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(strlen);
            description = raw[..end].iter().map(|&b| b as char).collect();
        }

        let pos = base + offset as usize;
        let common = Common {
            context,
            version: ver,
            alignment,
            description,
            offset,
            is_dirty: false,
            is_modified: false,
        };

        // A child whose data starts with a CHNK header is a nested package. Anything else is a leaf buffer.
        if rd_i32(d, pos).ok() == Some(CHNK_MAGIC) {
            let mut pkg = Package { c: common, children: Vec::new(), size };
            read_chunk(d, pos, &mut pkg, compressed)?;
            parent.children.push(Node::Package(pkg));
        } else {
            let data = d.get(pos..pos + size as usize).ok_or(ChunkError::Eof(pos))?.to_vec();
            parent.children.push(Node::Buffer(Buffer { c: common, data }));
        }
        // FileChunker resets the parent's flags after each Add
        parent.c.is_dirty = false;
        parent.c.is_modified = false;
    }
    Ok(())
}

// ---------------------------------------------------------------- writing

/// Writes a little-endian i32 at byte offset `at` of `out`. Panics when the four bytes do not fit.
fn wr_i32(out: &mut [u8], at: usize, v: i32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// FileChunker.WriteChunk (into a pre-filled buffer).
fn write_chunk(out: &mut [u8], base: usize, pkg: &mut Package) {
    let count = pkg.children.len();
    let total = pkg.size();
    wr_i32(out, base, CHNK_MAGIC);
    wr_i32(out, base + 4, total);
    wr_i32(out, base + 8, count as i32);
    wr_i32(out, base + 12, CHUNK_VERSION);

    // For each child: write its table entry, then its data (a nested package is written recursively),
    // then its description.
    for i in 0..count {
        let child = &mut pkg.children[i];
        let size = child.size(); // may lay out a nested package
        let c = child.c().clone();
        let e = base + 0x10 + i * 0x10;
        wr_i32(out, e, c.context);
        wr_i32(out, e + 4, c.offset);
        out[e + 8] = c.version;
        out[e + 9] = c.description.len() as u8;
        out[e + 10] = c.alignment;
        out[e + 11] = 0xFB;
        wr_i32(out, e + 12, size);

        let pos = base + c.offset as usize;
        match child {
            Node::Package(p) => write_chunk(out, pos, p),
            Node::Buffer(b) => out[pos..pos + b.data.len()].copy_from_slice(&b.data),
        }
        // The description is written as one byte per character, at offset + size. The byte after the
        // description is not written here, so it keeps its padding value.
        if !c.description.is_empty() {
            let p = base + (c.offset + size) as usize;
            let bytes: Vec<u8> = c.description.chars().map(|ch| ch as u32 as u8).collect();
            out[p..p + bytes.len()].copy_from_slice(&bytes);
        }
    }
}

/// A chunk file: the top-level `Content` package plus the loaded state.
#[derive(Debug, Clone)]
pub struct ChunkFile {
    pub content: Package,
    pub is_compressed: bool,
}

impl Default for ChunkFile {
    /// An empty file: an empty `Content` package with context 0, not compressed.
    fn default() -> Self {
        ChunkFile { content: Package::new(0), is_compressed: false }
    }
}

impl ChunkFile {
    /// Parses a chunk file from `d`. The top-level package has context 0.
    pub fn from_bytes(d: &[u8]) -> Result<Self, ChunkError> {
        let mut content = Package::new(0);
        let mut compressed = false;
        read_chunk(d, 0, &mut content, &mut compressed)?;
        // Content's _size stays 0 (new SpoolablePackage()), so the top level always relays out.
        Ok(ChunkFile { content, is_compressed: compressed })
    }

    /// Reads the file at `path` and parses it with `from_bytes`.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, ChunkError> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// Serialises the file. Fails with `Compressed` for an LZW-compressed file.
    /// The output starts filled with `PADDING`, and then the tree is written into it.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>, ChunkError> {
        if self.is_compressed {
            return Err(ChunkError::Compressed);
        }
        let total = self.content.size() as usize;
        let mut out: Vec<u8> = (0..total).map(|i| PADDING[i % 4]).collect();
        write_chunk(&mut out, 0, &mut self.content);
        Ok(out)
    }

    /// Serialises the file with `to_bytes` and writes the result to `path`.
    pub fn save(&mut self, path: impl AsRef<std::path::Path>) -> Result<(), ChunkError> {
        let b = self.to_bytes()?;
        std::fs::write(path, b)?;
        Ok(())
    }
}
