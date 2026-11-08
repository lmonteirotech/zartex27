//! `MDPC` v6 model package reader: meshes, LOD layers, materials, substances and textures.
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! Layout notes live in the reverse-engineering docs (re/formats/MDPC.md); all offsets
//! inside a package are relative to the start of its payload.
//!
//! Exposes `Package::parse`, `load_packages`, `CTX_MDPC`, the record types (`Model`, `Lod`, `LodInstance`, `SubModel`,
//! `VertexBuffer`, `Texture`, `Substance`, `Material`) and `Error`.
//! Uses `chnk` to walk containers. Used by `vehicle` (wheel radius) and by the `zartex-app` crate for the city, character
//! and vehicle models and the asset list (`city3d.rs`, `character3d.rs`, `vehicle3d.rs`, `assets.rs`).

use crate::chnk::Chunk;

/// Context code of an MDPC entry: the bytes `MDPC` read as a little-endian `u32`.
pub const CTX_MDPC: u32 = 0x4350_444D;

/// Errors while reading a package: a read past the end (the message names the field), an unsupported version, or an
/// inconsistent offset or count.
#[derive(Debug)]
pub enum Error {
    Truncated(&'static str),
    Version(u32),
    Bad(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated(w) => write!(f, "MDPC truncated reading {w}"),
            Error::Version(v) => write!(f, "unsupported MDPC version {v}"),
            Error::Bad(w) => write!(f, "MDPC inconsistent: {w}"),
        }
    }
}
impl std::error::Error for Error {}

/// Bounds-checked little-endian reader over a payload. Each read names the field it reads, for the error.
struct Rd<'a>(&'a [u8]);

impl Rd<'_> {
    fn u32(&self, o: usize, what: &'static str) -> Result<u32, Error> {
        self.0.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or(Error::Truncated(what))
    }
    fn u16(&self, o: usize, what: &'static str) -> Result<u16, Error> {
        self.0.get(o..o + 2).map(|s| u16::from_le_bytes(s.try_into().unwrap())).ok_or(Error::Truncated(what))
    }
    fn i16(&self, o: usize, what: &'static str) -> Result<i16, Error> {
        self.u16(o, what).map(|v| v as i16)
    }
    fn f32(&self, o: usize, what: &'static str) -> Result<f32, Error> {
        self.u32(o, what).map(f32::from_bits)
    }
}

/// One draw call of a sub-model: primitive type, base vertex, first index, primitive count and material reference.
#[derive(Debug, Clone)]
pub struct SubModel {
    /// D3DPT value: 4 = triangle list, 5 = triangle strip.
    pub prim: u32,
    pub base_vertex: u32,
    pub start_index: u32,
    pub prim_count: i32,
    /// `package<<16 | material index`; package 0 = no material.
    pub material: u32,
}

/// One placed instance of a LOD level: the sub-models it draws, its matrix and its transform flag.
#[derive(Debug, Clone)]
pub struct LodInstance {
    pub sub_first: usize,
    pub sub_count: usize,
    /// Row-vector 4x4 (translation in the last row), as stored.
    pub matrix: [f32; 16],
    pub use_transform: u16,
}

/// One LOD level: a range of `LodInstance`s (`inst_first`, `count`) and two flag words (`flags`, `mask`).
#[derive(Debug, Clone, Default)]
pub struct Lod {
    pub inst_first: usize,
    pub count: usize,
    pub flags: u32,
    pub mask: u32,
}

/// A model: its bounding box (eight corners), its vertex buffer index and type, and its seven LOD levels.
#[derive(Debug, Clone)]
pub struct Model {
    pub bbox: [[f32; 3]; 8],
    pub vb_index: i16,
    pub vb_type: i16,
    pub lods: [Lod; 7],
}

/// A vertex buffer: vertex count, stride in bytes, and byte offset within the payload.
#[derive(Debug, Clone)]
pub struct VertexBuffer {
    pub count: usize,
    pub stride: usize,
    pub offset: usize,
}

/// A texture: where its data sits (relative to the texture block), its size in bytes, and its width and height.
#[derive(Debug, Clone)]
pub struct Texture {
    pub data_off: usize,
    pub data_size: usize,
    pub width: u16,
    pub height: u16,
}

/// A substance. `bin` is the low byte of the `sinf` word and `flags` the rest of it; `ts` is the three low bytes of the
/// `sflg` word and `tex_flags` its top byte. `textures` holds indices into the texture list.
#[derive(Debug, Clone)]
pub struct Substance {
    pub bin: u8,
    pub flags: u32,
    pub ts: [u8; 3],
    pub tex_flags: u8,
    pub textures: Vec<usize>,
}

/// A material: the indices of the substances it refers to.
#[derive(Debug, Clone)]
pub struct Material {
    pub substances: Vec<usize>,
}

/// A parsed MDPC payload: its models, LOD instances, sub-models, vertex buffers, indices, materials, substances and
/// textures. Vertex and texture bytes are read from the payload when asked for.
pub struct Package<'a> {
    data: &'a [u8],
    pub uid: u32,
    pub models: Vec<Model>,
    pub lod_instances: Vec<LodInstance>,
    pub submodels: Vec<SubModel>,
    pub vbufs: Vec<VertexBuffer>,
    pub indices: Vec<u16>,
    pub materials: Vec<Material>,
    pub substances: Vec<Substance>,
    pub textures: Vec<Texture>,
    /// Absolute position of texture data (PCMP block + its texDataOff).
    tex_base: usize,
}

impl<'a> Package<'a> {
    /// Parses an MDPC payload. Fails unless the version is 6 and every table lies inside the payload. The material block
    /// (`PCMP`) is read only when its offset is not 0.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        let r = Rd(data);
        let version = r.u32(0, "version")?;
        if version != 6 {
            return Err(Error::Version(version));
        }
        let uid = r.u32(4, "uid")?;
        let n_models = r.u32(8, "nModels")? as usize;
        let models_off = r.u32(0xC, "modelsOff")? as usize;
        let n_lodinst = r.u32(0x10, "nLodInst")? as usize;
        let lodinst_off = r.u32(0x14, "lodInstOff")? as usize;
        let n_sub = r.u32(0x18, "nSub")? as usize;
        let sub_off = r.u32(0x1C, "subOff")? as usize;
        let mat_off = r.u32(0x2C, "matDataOff")? as usize;
        let n_indices = r.u32(0x30, "nIndices")? as usize;
        let indices_off = r.u32(0x38, "indicesOff")? as usize;
        let n_vb = r.u32(0x3C, "nVB")? as usize;
        let vdecl_off = r.u32(0x40, "vdeclOff")? as usize;

        // the tables below are read at the offsets given in the header, each entry at its fixed size
        let mut vbufs = Vec::with_capacity(n_vb);
        for i in 0..n_vb {
            let o = vdecl_off + 0x1C * i;
            vbufs.push(VertexBuffer {
                count: r.u32(o, "vb count")? as usize,
                offset: r.u32(o + 8, "vb offset")? as usize,
                stride: r.u32(o + 0xC, "vb stride")? as usize,
            });
        }

        let mut indices = Vec::with_capacity(n_indices);
        for i in 0..n_indices {
            indices.push(r.u16(indices_off + 2 * i, "index")?);
        }

        let mut submodels = Vec::with_capacity(n_sub);
        for i in 0..n_sub {
            let o = sub_off + 0x38 * i;
            submodels.push(SubModel {
                prim: r.u32(o, "prim")?,
                base_vertex: r.u32(o + 4, "baseVertex")?,
                start_index: r.u32(o + 0x10, "startIndex")?,
                prim_count: r.u32(o + 0x14, "primCount")? as i32,
                material: r.u32(o + 0x30, "material")?,
            });
        }

        let mut lod_instances = Vec::with_capacity(n_lodinst);
        for i in 0..n_lodinst {
            let o = lodinst_off + 0x58 * i;
            let off = r.u32(o, "subOff")? as usize;
            let sub_count = r.u16(o + 0x48, "subCount")? as usize;
            let sub_first = if sub_count == 0 || off < sub_off { 0 } else { (off - sub_off) / 0x38 };
            if sub_first + sub_count > n_sub {
                return Err(Error::Bad("lod instance submodel range"));
            }
            let mut matrix = [0f32; 16];
            for (k, m) in matrix.iter_mut().enumerate() {
                *m = r.f32(o + 8 + 4 * k, "matrix")?;
            }
            lod_instances.push(LodInstance { sub_first, sub_count, matrix, use_transform: r.u16(o + 0x4A, "useTransform")? });
        }

        // each model has seven LOD levels; their instance ranges are converted from byte offsets to indices
        let mut models = Vec::with_capacity(n_models);
        for i in 0..n_models {
            let o = models_off + 0x188 * i;
            let mut bbox = [[0f32; 3]; 8];
            for (c, corner) in bbox.iter_mut().enumerate() {
                for (k, v) in corner.iter_mut().enumerate() {
                    *v = r.f32(o + 0x28 + 16 * c + 4 * k, "bbox")?;
                }
            }
            let mut lods: [Lod; 7] = Default::default();
            for (s, lod) in lods.iter_mut().enumerate() {
                let lo = o + 0xA8 + 0x20 * s;
                let off = r.u32(lo, "lod instOff")? as usize;
                let count = r.u32(lo + 8, "lod count")? as usize;
                let inst_first = if count == 0 || off < lodinst_off { 0 } else { (off - lodinst_off) / 0x58 };
                if inst_first + count > n_lodinst {
                    return Err(Error::Bad("lod instance range"));
                }
                *lod = Lod { inst_first, count, flags: r.u32(lo + 0x10, "lod flags")?, mask: r.u32(lo + 0x14, "lod mask")? };
            }
            models.push(Model { bbox, vb_index: r.i16(o + 0x18, "vbIndex")?, vb_type: r.i16(o + 0x1A, "vbType")?, lods });
        }

        let mut p = Package {
            data,
            uid,
            models,
            lod_instances,
            submodels,
            vbufs,
            indices,
            materials: Vec::new(),
            substances: Vec::new(),
            textures: Vec::new(),
            tex_base: 0,
        };
        if mat_off != 0 {
            p.parse_pcmp(mat_off)?;
        }
        Ok(p)
    }

    /// Reads the `PCMP` material block at `mb`: textures, then substances (with their texture references), then materials
    /// (with their substance references). The references are turned from file offsets into indices.
    fn parse_pcmp(&mut self, mb: usize) -> Result<(), Error> {
        let r = Rd(self.data);
        if self.data.get(mb..mb + 4) != Some(b"PCMP") {
            return Err(Error::Bad("PCMP signature"));
        }
        let n_mat = r.u32(mb + 8, "nMaterials")? as usize;
        let mat_off = r.u32(mb + 0xC, "materialsOff")? as usize;
        let n_sub = r.u32(mb + 0x18, "nSubstances")? as usize;
        let sub_off = r.u32(mb + 0x1C, "substancesOff")? as usize;
        let n_tex = r.u32(mb + 0x28, "nTextures")? as usize;
        let tex_off = r.u32(mb + 0x2C, "texturesOff")? as usize;
        self.tex_base = mb + r.u32(mb + 0x30, "texDataOff")? as usize;

        // file offset of each texture record -> its index, to resolve the substance references
        let mut tex_by_off = std::collections::HashMap::new();
        for i in 0..n_tex {
            let o = mb + tex_off + 32 * i;
            tex_by_off.insert(tex_off + 32 * i, i);
            self.textures.push(Texture {
                data_off: r.u32(o + 8, "tex dataOff")? as usize,
                data_size: r.u32(o + 12, "tex dataSize")? as usize,
                width: r.u16(o + 0x14, "tex w")?,
                height: r.u16(o + 0x16, "tex h")?,
            });
        }
        // file offset of each substance record -> its index, to resolve the material references
        let mut sub_by_off = std::collections::HashMap::new();
        for i in 0..n_sub {
            let o = mb + sub_off + 32 * i;
            sub_by_off.insert(sub_off + 32 * i, i);
            let sinf = r.u32(o, "sinf")?;
            let sflg = r.u32(o + 4, "sflg")?;
            let refs_off = r.u32(o + 16, "texRefsOff")? as usize;
            let refs_n = r.u32(o + 20, "texRefsCount")? as usize;
            let mut textures = Vec::with_capacity(refs_n);
            for k in 0..refs_n {
                let t = r.u32(mb + refs_off + 8 * k, "texRef")? as usize;
                textures.push(*tex_by_off.get(&t).ok_or(Error::Bad("texture ref"))?);
            }
            self.substances.push(Substance {
                bin: sinf as u8,
                flags: sinf >> 8,
                ts: [sflg as u8, (sflg >> 8) as u8, (sflg >> 16) as u8],
                tex_flags: (sflg >> 24) as u8,
                textures,
            });
        }
        for i in 0..n_mat {
            let o = mb + mat_off + 24 * i;
            let refs_off = r.u32(o, "subRefsOff")? as usize;
            let refs_n = r.u32(o + 4, "subRefsCount")? as usize;
            let mut substances = Vec::with_capacity(refs_n);
            for k in 0..refs_n {
                let s = r.u32(mb + refs_off + 8 * k, "subRef")? as usize;
                substances.push(*sub_by_off.get(&s).ok_or(Error::Bad("substance ref"))?);
            }
            self.materials.push(Material { substances });
        }
        Ok(())
    }

    /// Raw DDS (or BMP) bytes of texture `i`, or `None` for a bad index or a range outside the payload.
    pub fn texture_bytes(&self, i: usize) -> Option<&'a [u8]> {
        let t = self.textures.get(i)?;
        let s = self.tex_base + t.data_off;
        self.data.get(s..s + t.data_size)
    }

    /// Position, normal and UV of vertex `i` of buffer `vb` (every vertex type
    /// starts with these three attributes).
    pub fn vertex(&self, vb: usize, i: usize) -> Option<([f32; 3], [f32; 3], [f32; 2])> {
        let b = self.vbufs.get(vb)?;
        if i >= b.count {
            return None;
        }
        let o = b.offset + i * b.stride;
        let r = Rd(self.data);
        let f = |k: usize| r.f32(o + 4 * k, "vertex").ok();
        Some(([f(0)?, f(1)?, f(2)?], [f(3)?, f(4)?, f(5)?], [f(6)?, f(7)?]))
    }

    /// Raw floats of vertex `i` of buffer `vb` (stride / 4 values).
    pub fn vertex_floats(&self, vb: usize, i: usize) -> Option<Vec<f32>> {
        let b = self.vbufs.get(vb)?;
        if i >= b.count {
            return None;
        }
        let r = Rd(self.data);
        (0..b.stride / 4).map(|k| r.f32(b.offset + i * b.stride + 4 * k, "vertex").ok()).collect()
    }

    /// Triangle indices of a sub-model (base vertex already added), appended
    /// as a plain triangle list. Strip triangles (type 5) alternate their winding and degenerate ones are dropped; list
    /// triangles (type 4) are copied; any other type is an error.
    pub fn submodel_triangles(&self, s: &SubModel, out: &mut Vec<u32>) -> Result<(), Error> {
        if s.prim_count < 1 {
            return Ok(());
        }
        let pc = s.prim_count as usize;
        let start = s.start_index as usize;
        let add = s.base_vertex;
        match s.prim {
            5 => {
                let seq = self.indices.get(start..start + pc + 2).ok_or(Error::Bad("strip outside index buffer"))?;
                for i in 0..pc {
                    let (a, b, c) = (seq[i], seq[i + 1], seq[i + 2]);
                    if a == b || b == c || a == c {
                        continue;
                    }
                    let (b, c) = if i & 1 == 1 { (c, b) } else { (b, c) };
                    out.extend_from_slice(&[a as u32 + add, b as u32 + add, c as u32 + add]);
                }
            }
            4 => {
                let seq = self.indices.get(start..start + pc * 3).ok_or(Error::Bad("list outside index buffer"))?;
                out.extend(seq.iter().map(|&i| i as u32 + add));
            }
            _ => return Err(Error::Bad("unsupported primitive type")),
        }
        Ok(())
    }
}

/// All MDPC packages found directly in a `.pcs` container (or embedded in a
/// `.d3c`), in file order.
pub fn load_packages<'a>(root: &Chunk<'a>) -> Result<Vec<Package<'a>>, Error> {
    let mut out = Vec::new();
    collect(root, &mut out)?;
    Ok(out)
}

/// Parses every MDPC entry of `c` and of its nested containers, in file order. Nested containers that cannot be read are
/// skipped; a package that fails to parse is an error.
fn collect<'a>(c: &Chunk<'a>, out: &mut Vec<Package<'a>>) -> Result<(), Error> {
    for e in &c.entries {
        if e.ctx == CTX_MDPC {
            out.push(Package::parse(e.data)?);
        } else if e.is_chnk() {
            if let Ok(child) = e.children() {
                collect(&child, out)?;
            }
        }
    }
    Ok(())
}
