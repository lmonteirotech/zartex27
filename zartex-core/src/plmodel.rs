//! Parser for Driver: Parallel Lines model packages (MDXN, entry version 1) and the PCMP v3
//! material/texture block stored inside them.
//!
//! The CHNK container is read with `crate::chunk`. Everything inside a package is read by byte
//! offset (tables are not 4-byte aligned). Offsets are relative to the start of the MDXN chunk
//! data unless noted. The PCMP block offsets are relative to the start of the PCMP block.
//!
//! Coordinate space: positions are model-local and Z-up as stored. The only transform applied is
//! the per-axis `Scale` from the model record (`x * s.x`, `y * s.y`, `z * s.z`). No axis swap is
//! applied, so `[x, y, z]` here is the same axis order as the file.
//!
//! It provides: `parse_package` and `load_package` (one package); `find_packages` and `index_packages` (the MDXN
//! packages of a city file, where `index_packages` reads only the chunk tables); `decode_rgba` (DXT1, DXT3 and DXT5
//! to RGBA8); and the placement readers `read_tiles`, `read_props`, `read_interior_props`, `read_super_ids`, and the
//! experimental `read_roads`.
//! It uses `crate::chunk` (`ChunkFile` and `Node`, to walk the CHNK container).
//! It is used by the zartex-app crate (`plassets.rs` loads the packages and placements of a city).
//! The tests need the environment variable PL_DIR (the game folder); they are skipped when it is not set.

use std::collections::HashMap;
use std::path::Path;

use crate::chunk::{ChunkFile, Node};

const TAG_GEGR: i32 = i32::from_le_bytes(*b"GEGR");
const TAG_GESR: i32 = i32::from_le_bytes(*b"GESR");
const TAG_IRCT: i32 = i32::from_le_bytes(*b"IRCT");
const TAG_GEGL: i32 = i32::from_le_bytes(*b"GEGL");
const TAG_GESI: i32 = i32::from_le_bytes(*b"GESI");
const TAG_GEIR: i32 = i32::from_le_bytes(*b"GEIR");
const TAG_GEBI: i32 = i32::from_le_bytes(*b"GEBI");
const TAG_MDXN: i32 = i32::from_le_bytes(*b"MDXN");
const MDXN_VERSION: u8 = 1;

const PKG_HDR_SIZE: usize = 0x44;
const MODEL_SIZE: usize = 0x14C;
const LOD_SLOT_OFF: usize = 0xA4;
const LOD_SLOT_SIZE: usize = 0x18;
const LOD_SLOTS: usize = 7;
const SUB_SIZE: usize = 0x38;
const VB_DECL_SIZE: usize = 0x20;
const VERTEX_STRIDE: usize = 0x3C;
const PRIM_TRIANGLE_STRIP: i32 = 6;
const NO_MATERIAL_UID: u16 = 0xCCCC;

const PCMP_MAGIC: &[u8; 4] = b"PCMP";
const PCMP_HDR_SIZE: usize = 0x48;
const MAT_SIZE: usize = 0x10;
const SUBST_SIZE: usize = 0x1C;
const TEXREF_SIZE: usize = 4;
const TEX_SIZE: usize = 0x20;
const DDS_HEADER: usize = 128;

// ---------------------------------------------------------------- public types

/// Block-compressed format of a texture, from the fourCC of its DDS header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexFormat {
    /// Colour block only (DXT1).
    Dxt1,
    /// Explicit 4-bit alpha per texel (DXT3).
    Dxt3,
    /// Interpolated alpha (DXT5).
    Dxt5,
}

/// One texture record from the PCMP block plus its complete DDS file.
#[derive(Debug, Clone)]
pub struct PlTexture {
    /// The uid of the texture record.
    pub uid: u32,
    pub handle: i32,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    /// Raw type field from the packed format word (bits 8-15). DXT3 = 14 and DXT5 = 15 were seen.
    pub packed_type: u8,
    pub format: TexFormat,
    /// Raw DDS file bytes (header + all mips), exactly as stored.
    pub dds: Vec<u8>,
}

/// A substance record. `tex_refs[0]` is the diffuse map.
#[derive(Debug, Clone)]
pub struct PlSubstance {
    pub flags: u32,
    pub render_bin: u8,
    pub alpha_cutout: bool,
    /// Indices into `PlPackage::textures`.
    pub tex_refs: Vec<usize>,
}

impl PlSubMesh {
    /// False when the sub-model UID is the 0xCCCC "no material" marker.
    pub fn has_material(&self) -> bool {
        self.material.1 != NO_MATERIAL_UID
    }
}

/// A material record. Its handle (sub-model `Handle`) indexes `PlPackage::materials`.
#[derive(Debug, Clone)]
pub struct PlMaterial {
    pub mat_type: i32,
    pub anim_speed: f32,
    pub substances: Vec<PlSubstance>,
}

/// One sub-model. Vertices are compacted: only the vertices used by a triangle are stored, and
/// `indices` index into `positions`/`normals`/`uvs`/`colors`.
#[derive(Debug, Clone)]
pub struct PlSubMesh {
    /// Raw primType from the sub-model record (only 6 = triangle strip is accepted).
    pub prim_type: i32,
    /// (Handle, UID) from the sub-model record. UID 0xCCCC means no material. UID 0 means the
    /// global GEGR package. Other UIDs name the package whose header uid matches. Resolution is
    /// left to the caller because it spans packages.
    pub material: (u16, u16),
    /// Model-local positions, scaled by the model Scale (Z-up, not axis-swapped).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[u8; 4]>,
    /// Triangle list, 3 indices per triangle.
    pub indices: Vec<u32>,
}

/// One model record of a package, with the sub-models of its first used LOD slot.
#[derive(Debug, Clone)]
pub struct PlModel {
    /// The two 32-bit words at the start of the model record.
    pub uid: (i32, i32),
    /// Scale of each axis, applied to the positions (see the module header).
    pub scale: [f32; 3],
    /// Index of the vertex buffer declaration that the sub-models use.
    pub vb_index: i16,
    /// Stored from the model record at +0x1A.
    pub vb_type: i16,
    pub flags: u32,
    /// Index of the first non-empty LOD slot that was used, if any.
    pub lod_slot: Option<usize>,
    /// Number of LodInstance records in the used slot. Only the first one is read.
    pub lod_instance_count: usize,
    pub submodels: Vec<PlSubMesh>,
}

/// A parsed package: its uid and models, and the materials and textures of its PCMP block.
#[derive(Debug, Clone)]
pub struct PlPackage {
    /// The package uid, from the header at +0x04.
    pub uid: i32,
    pub models: Vec<PlModel>,
    pub materials: Vec<PlMaterial>,
    pub textures: Vec<PlTexture>,
}

// ---------------------------------------------------------------- low level readers

/// Returns `len` bytes at byte offset `at`, or an error when the read runs past the end of the data.
fn bytes_at(d: &[u8], at: usize, len: usize) -> Result<&[u8], String> {
    at.checked_add(len)
        .and_then(|end| d.get(at..end))
        .ok_or_else(|| format!("read of {len} bytes at {at:#x} runs past end of data ({:#x})", d.len()))
}

// Little-endian readers of one value at byte offset `at`. Each fails in the same way as `bytes_at`.
fn u16_at(d: &[u8], at: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(bytes_at(d, at, 2)?.try_into().unwrap()))
}

fn i16_at(d: &[u8], at: usize) -> Result<i16, String> {
    Ok(i16::from_le_bytes(bytes_at(d, at, 2)?.try_into().unwrap()))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(bytes_at(d, at, 4)?.try_into().unwrap()))
}

fn i32_at(d: &[u8], at: usize) -> Result<i32, String> {
    Ok(i32::from_le_bytes(bytes_at(d, at, 4)?.try_into().unwrap()))
}

fn f32_at(d: &[u8], at: usize) -> Result<f32, String> {
    Ok(f32::from_le_bytes(bytes_at(d, at, 4)?.try_into().unwrap()))
}

/// Converts an offset read from the file to a usize. A negative value is an error; `what` names it in the message.
fn to_off(v: i32, what: &str) -> Result<usize, String> {
    usize::try_from(v).map_err(|_| format!("negative offset {v} for {what}"))
}

/// Converts a count read from the file to a usize. A negative value is an error; `what` names it in the message.
fn to_count(v: i32, what: &str) -> Result<usize, String> {
    usize::try_from(v).map_err(|_| format!("negative count {v} for {what}"))
}

/// Index of the `rec_size`-byte record at `rel` inside a table that starts at `table_off`.
fn record_index(rel: usize, table_off: usize, rec_size: usize, count: usize, what: &str) -> Result<usize, String> {
    if rel < table_off || (rel - table_off) % rec_size != 0 {
        return Err(format!("{what} reference {rel:#x} is not a record start in table at {table_off:#x}"));
    }
    let idx = (rel - table_off) / rec_size;
    if idx >= count {
        return Err(format!("{what} index {idx} out of range (count {count})"));
    }
    Ok(idx)
}

// ---------------------------------------------------------------- package parsing

/// A vertex buffer declaration: the vertex count and the byte offset of the vertex data.
struct VbDecl {
    count: usize,
    off: usize,
}

/// Parse one MDXN package (the chunk data of an MDXN entry with version 1).
pub fn parse_package(chunk_data: &[u8]) -> Result<PlPackage, String> {
    let d = chunk_data;
    if d.len() < PKG_HDR_SIZE {
        return Err(format!("package is {} bytes, shorter than the {PKG_HDR_SIZE}-byte header", d.len()));
    }

    let uid = i32_at(d, 0x04)?;
    let n_models = to_count(i32_at(d, 0x08)?, "nModels")?;
    let models_off = to_off(i32_at(d, 0x0C)?, "modelsOff")?;
    let idx_off = to_off(i32_at(d, 0x38)?, "idxOff")?;
    let idx_byte_len = to_count(i32_at(d, 0x34)?, "idxByteLen")?;
    let n_vb = to_count(i32_at(d, 0x3C)?, "nVb")?;
    let vb_decl_off = to_off(i32_at(d, 0x40)?, "vbDeclOff")?;
    let mat_base = to_off(i32_at(d, 0x2C)?, "MaterialDataOffset")?;
    // +0x28 (TextureDataOffset) is not read. The texture data is located through the PCMP block instead.

    // The index buffer must lie inside the package.
    bytes_at(d, idx_off, idx_byte_len)?;

    // Vertex buffer declarations: count, byte length, data offset and stride of each one. Only the stride 0x3C is accepted.
    let mut vbs = Vec::with_capacity(n_vb);
    for k in 0..n_vb {
        let o = vb_decl_off + k * VB_DECL_SIZE;
        let count = to_count(i32_at(d, o)?, "vb count")?;
        let byte_len = to_count(i32_at(d, o + 4)?, "vb byteLen")?;
        let off = to_off(i32_at(d, o + 8)?, "vb off")?;
        let stride = to_count(i32_at(d, o + 0xC)?, "vb stride")?;
        if stride != VERTEX_STRIDE {
            return Err(format!("vb decl {k}: stride {stride:#x}, expected {VERTEX_STRIDE:#x}"));
        }
        if count * stride != byte_len {
            return Err(format!("vb decl {k}: count {count} * stride {stride} != byteLen {byte_len}"));
        }
        bytes_at(d, off, byte_len)?;
        vbs.push(VbDecl { count, off });
    }

    // One PlModel per model record. Only the first LOD slot that has instances contributes sub-models.
    let mut models = Vec::with_capacity(n_models);
    for mi in 0..n_models {
        let mo = models_off + mi * MODEL_SIZE;
        let model_uid = (i32_at(d, mo)?, i32_at(d, mo + 4)?);
        let scale = [f32_at(d, mo + 8)?, f32_at(d, mo + 12)?, f32_at(d, mo + 16)?];
        let vb_index = i16_at(d, mo + 0x18)?;
        let vb_type = i16_at(d, mo + 0x1A)?;
        let flags = u32_at(d, mo + 0x1C)?;

        // First non-empty LOD slot.
        let mut lod: Option<(usize, usize, usize)> = None;
        for s in 0..LOD_SLOTS {
            let so = mo + LOD_SLOT_OFF + s * LOD_SLOT_SIZE;
            let inst_off = to_off(i32_at(d, so)?, "lod instOffset")?;
            let inst_cnt = to_count(i32_at(d, so + 4)?, "lod instCount")?;
            if inst_cnt > 0 {
                lod = Some((s, inst_off, inst_cnt));
                break;
            }
        }

        // The sub-models come from the first LodInstance of the used slot, and their count is in that record.
        let mut submodels = Vec::new();
        let (mut lod_slot, mut lod_instance_count) = (None, 0);
        if let Some((slot, inst_off, inst_cnt)) = lod {
            lod_slot = Some(slot);
            lod_instance_count = inst_cnt;
            let sub_base = to_off(i32_at(d, inst_off)?, "lod subOffset")?;
            let sub_count = to_count(i16_at(d, inst_off + 0x44)? as i32, "subCount")?;
            let vb = usize::try_from(vb_index)
                .ok()
                .and_then(|i| vbs.get(i))
                .ok_or_else(|| format!("model {mi}: vbIndex {vb_index} has no vertex decl"))?;
            for s in 0..sub_count {
                submodels.push(build_submodel(d, sub_base + s * SUB_SIZE, scale, vb, idx_off, idx_byte_len)
                    .map_err(|e| format!("model {mi} sub {s}: {e}"))?);
            }
        }

        models.push(PlModel {
            uid: model_uid,
            scale,
            vb_index,
            vb_type,
            flags,
            lod_slot,
            lod_instance_count,
            submodels,
        });
    }

    let (textures, materials) = parse_pcmp(d, mat_base)?;
    Ok(PlPackage { uid, models, materials, textures })
}

/// Read one sub-model record, its index run, and the vertices it references.
fn build_submodel(
    d: &[u8],
    at: usize,
    scale: [f32; 3],
    vb: &VbDecl,
    idx_off: usize,
    idx_byte_len: usize,
) -> Result<PlSubMesh, String> {
    let prim = i32_at(d, at)?;
    if prim != PRIM_TRIANGLE_STRIP {
        return Err(format!("unsupported primType {prim} at {at:#x}"));
    }
    let idx_count = to_count(i32_at(d, at + 0xC)?, "index count")?;
    let idx_byte = to_count(i32_at(d, at + 0x10)?, "index byte offset")?;
    let handle = u16_at(d, at + 0x30)?;
    let muid = u16_at(d, at + 0x32)?;

    if idx_byte % 2 != 0 {
        return Err(format!("odd index byte offset {idx_byte}"));
    }
    if idx_byte + idx_count * 2 > idx_byte_len {
        return Err(format!("index run {idx_byte}+{idx_count} runs past index buffer ({idx_byte_len} bytes)"));
    }

    let mut raw = Vec::with_capacity(idx_count);
    for q in 0..idx_count {
        let v = u16_at(d, idx_off + idx_byte + 2 * q)?;
        if v as usize >= vb.count {
            return Err(format!("index {v} >= vertex count {}", vb.count));
        }
        raw.push(v);
    }

    let mut mesh = PlSubMesh {
        prim_type: prim,
        material: (handle, muid),
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    };
    let mut remap: HashMap<u16, u32> = HashMap::new();

    // Triangle strip: triangle i is (i, i+1, i+2), with the winding flipped on odd i.
    // Degenerate triangles (any repeated vertex) are dropped.
    for i in 0..raw.len().saturating_sub(2) {
        let (mut a, mut b, c) = (raw[i], raw[i + 1], raw[i + 2]);
        if i % 2 == 1 {
            std::mem::swap(&mut a, &mut b);
        }
        if a == b || b == c || a == c {
            continue;
        }
        for v in [a, b, c] {
            let ci = push_vertex(d, vb, scale, v, &mut remap, &mut mesh)?;
            mesh.indices.push(ci);
        }
    }
    Ok(mesh)
}

/// Adds vertex `v` (an index into the vertex buffer) to the sub-model and returns its index in `positions`. A vertex that
/// was added before is not added again: `remap` gives its index. Positions are scaled. Normals, UVs and colours are read
/// from the 0x3C-byte record; the tangent and binormal are not read.
fn push_vertex(
    d: &[u8],
    vb: &VbDecl,
    scale: [f32; 3],
    v: u16,
    remap: &mut HashMap<u16, u32>,
    mesh: &mut PlSubMesh,
) -> Result<u32, String> {
    if let Some(&i) = remap.get(&v) {
        return Ok(i);
    }
    let at = vb.off + v as usize * VERTEX_STRIDE;
    let b = bytes_at(d, at, VERTEX_STRIDE)?;
    let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    // pos f32x3 @0, normal f32x3 @12, uv f32x2 @24, tangent @32, binormal @44, color RGBA8 @56
    mesh.positions.push([f(0) * scale[0], f(4) * scale[1], f(8) * scale[2]]);
    mesh.normals.push([f(12), f(16), f(20)]);
    mesh.uvs.push([f(24), f(28)]);
    mesh.colors.push([b[56], b[57], b[58], b[59]]);
    let i = (mesh.positions.len() - 1) as u32;
    remap.insert(v, i);
    Ok(i)
}

// ---------------------------------------------------------------- PCMP

/// Reads the PCMP block at `base` (the package MaterialDataOffset): the textures, each with its whole DDS file copied out,
/// and the materials, which are resolved through the substance-reference and substance tables.
fn parse_pcmp(d: &[u8], base: usize) -> Result<(Vec<PlTexture>, Vec<PlMaterial>), String> {
    let hdr = bytes_at(d, base, PCMP_HDR_SIZE)?;
    if &hdr[0..4] != PCMP_MAGIC {
        return Err(format!("no PCMP magic at MaterialDataOffset {base:#x}"));
    }
    // Header fields are read as i32 at fixed offsets from the start of the PCMP block.
    let p = |o: usize| i32_at(d, base + o);
    let version = p(4)?;
    if version != 3 {
        return Err(format!("PCMP version {version}, expected 3"));
    }
    let mat_count = to_count(p(8)?, "matCount")?;
    let mat_off = to_off(p(0xC)?, "matOff")?;
    let sref_count = to_count(p(0x10)?, "substanceRefCount")?;
    let sref_off = to_off(p(0x14)?, "substanceRefOff")?;
    let sub_count = to_count(p(0x18)?, "substanceCount")?;
    let sub_off = to_off(p(0x1C)?, "substanceOff")?;
    let tref_count = to_count(p(0x30)?, "texRefCount")?;
    let tref_off = to_off(p(0x34)?, "texRefOff")?;
    let tex_count = to_count(p(0x38)?, "texCount")?;
    let tex_off = to_off(p(0x3C)?, "texOff")?;
    let tex_data = base + to_off(p(0x40)?, "texDataOff")?;

    // Sanity: the sub-tables must sit where the header says they do.
    bytes_at(d, base + mat_off, mat_count * MAT_SIZE)?;
    bytes_at(d, base + sref_off, sref_count * TEXREF_SIZE)?;
    bytes_at(d, base + sub_off, sub_count * SUBST_SIZE)?;
    bytes_at(d, base + tref_off, tref_count * TEXREF_SIZE)?;

    // Textures.
    let mut textures = Vec::with_capacity(tex_count);
    for t in 0..tex_count {
        let r = base + tex_off + t * TEX_SIZE;
        let uid = u32_at(d, r)?;
        let handle = i32_at(d, r + 4)?;
        let data_off = to_off(i32_at(d, r + 0x10)?, "texture dataOffset")?;
        let packed = u32_at(d, r + 0x18)?;
        let packed_type = ((packed >> 8) & 0xFF) as u8;
        let packed_mips = (packed >> 16) & 0xF;
        let width = 1u32 << ((packed >> 20) & 0xF);
        let height = 1u32 << ((packed >> 24) & 0xF);

        // The DDS file is at the texture data offset. Its header must agree with the packed size fields.
        let dds_at = tex_data + data_off;
        if bytes_at(d, dds_at, 4)? != b"DDS " {
            return Err(format!("texture {t}: no DDS magic at {dds_at:#x}"));
        }
        let hsize = u32_at(d, dds_at + 4)?;
        if hsize != 124 {
            return Err(format!("texture {t}: DDS header size {hsize}, expected 124"));
        }
        let hh = u32_at(d, dds_at + 12)?;
        let hw = u32_at(d, dds_at + 16)?;
        // dwMipMapCount is 0 for a single-mip texture, so compare with max(1, count).
        let hmips = u32_at(d, dds_at + 28)?.max(1);
        if (hw, hh) != (width, height) {
            return Err(format!("texture {t}: DDS {hw}x{hh} != packed {width}x{height}"));
        }
        if hmips != packed_mips.max(1) {
            return Err(format!("texture {t}: DDS mips {hmips} != packed mips {packed_mips}"));
        }
        let fourcc = bytes_at(d, dds_at + 84, 4)?;
        let format = match fourcc {
            b"DXT1" => TexFormat::Dxt1,
            b"DXT3" => TexFormat::Dxt3,
            b"DXT5" => TexFormat::Dxt5,
            other => return Err(format!("texture {t}: unsupported fourCC {:?}", other)),
        };
        let total = dds_total_size(format, width as usize, height as usize, hmips as usize);
        // Copy the whole DDS file: header plus all mip levels.
        let dds = bytes_at(d, dds_at, total)?.to_vec();

        textures.push(PlTexture {
            uid,
            handle,
            width,
            height,
            mip_count: hmips,
            packed_type,
            format,
            dds,
        });
    }

    // Materials, resolved through the substance-reference and substance tables.
    let mut materials = Vec::with_capacity(mat_count);
    for m in 0..mat_count {
        let r = base + mat_off + m * MAT_SIZE;
        let sref_list = to_off(i32_at(d, r)?, "material substanceRefsOffset")?;
        let sref_n = to_count(i32_at(d, r + 4)?, "material substance count")?;
        let mat_type = i32_at(d, r + 8)?;
        let anim_speed = f32_at(d, r + 0xC)?;

        let mut substances = Vec::with_capacity(sref_n);
        for k in 0..sref_n {
            let rel = to_off(i32_at(d, base + sref_list + k * TEXREF_SIZE)?, "substance reference")?;
            let si = record_index(rel, sub_off, SUBST_SIZE, sub_count, "substance")?;
            let sr = base + sub_off + si * SUBST_SIZE;
            let flags = u32_at(d, sr)?;
            let sinf = u32_at(d, sr + 4)?;
            let tref_list = to_off(i32_at(d, sr + 8)?, "substance texRefsOff")?;
            let tref_n = to_count(i32_at(d, sr + 0xC)?, "substance texRefsCount")?;
            let mut tex_refs = Vec::with_capacity(tref_n);
            for j in 0..tref_n {
                let trel = to_off(i32_at(d, base + tref_list + j * TEXREF_SIZE)?, "texture reference")?;
                tex_refs.push(record_index(trel, tex_off, TEX_SIZE, tex_count, "texture")?);
            }
            substances.push(PlSubstance {
                flags,
                render_bin: (sinf & 0xFF) as u8,
                alpha_cutout: sinf & 0x4 != 0,
                tex_refs,
            });
        }
        materials.push(PlMaterial { mat_type, anim_speed, substances });
    }

    Ok((textures, materials))
}

/// Total DDS file size: 128-byte header plus all mip levels in block-compressed form.
fn dds_total_size(format: TexFormat, width: usize, height: usize, mips: usize) -> usize {
    let bs = if format == TexFormat::Dxt1 { 8 } else { 16 };
    let (mut w, mut h) = (width, height);
    let mut total = DDS_HEADER;
    for _ in 0..mips.max(1) {
        total += ((w + 3) / 4).max(1) * ((h + 3) / 4).max(1) * bs;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    total
}

// ---------------------------------------------------------------- DXT decoding

/// Decode mip 0 of a texture to RGBA8, row-major, `width * height * 4` bytes.
pub fn decode_rgba(t: &PlTexture) -> Vec<u8> {
    let (w, h) = (t.width as usize, t.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let bs = if t.format == TexFormat::Dxt1 { 8 } else { 16 };
    // The block data follows the 128-byte DDS header. A block is 8 bytes (DXT1) or 16 bytes (DXT3 and DXT5).
    let data = t.dds.get(DDS_HEADER..).unwrap_or(&[]);
    let bw = ((w + 3) / 4).max(1);
    let bh = ((h + 3) / 4).max(1);
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * bs;
            let Some(block) = data.get(at..at + bs) else { return out };
            let texels = decode_block(t.format, block);
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x < w && y < h {
                        let o = (y * w + x) * 4;
                        out[o..o + 4].copy_from_slice(&texels[py * 4 + px]);
                    }
                }
            }
        }
    }
    out
}

/// Expands a 5:6:5 colour to 8 bits per channel. The top bits of each channel are copied into its low bits.
fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u8;
    let g = ((c >> 5) & 63) as u8;
    let b = (c & 31) as u8;
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// The eight alpha values of a DXT5 block. When a0 > a1, six values are interpolated between them; otherwise four are
/// interpolated, and 0 and 255 are added.
fn dxt5_alpha_table(a0: u8, a1: u8) -> [u8; 8] {
    let mut t = [0u8; 8];
    t[0] = a0;
    t[1] = a1;
    let (w0, w1) = (a0 as u16, a1 as u16);
    if a0 > a1 {
        for i in 1..=6u16 {
            t[(i + 1) as usize] = (((7 - i) * w0 + i * w1) / 7) as u8;
        }
    } else {
        for i in 1..=4u16 {
            t[(i + 1) as usize] = (((5 - i) * w0 + i * w1) / 5) as u8;
        }
        t[6] = 0;
        t[7] = 255;
    }
    t
}

/// Decode one 4x4 block. Texel order is row-major (index = 4*y + x).
fn decode_block(fmt: TexFormat, b: &[u8]) -> [[u8; 4]; 16] {
    // DXT1 has only a colour block. DXT3 and DXT5 have an alpha block followed by a colour block.
    let (alpha_src, color) = match fmt {
        TexFormat::Dxt1 => (None, &b[0..8]),
        _ => (Some(&b[0..8]), &b[8..16]),
    };
    let c0 = u16::from_le_bytes([color[0], color[1]]);
    let c1 = u16::from_le_bytes([color[2], color[3]]);
    let p0 = rgb565(c0);
    let p1 = rgb565(c1);
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if fmt == TexFormat::Dxt1 && c0 <= c1 {
        for i in 0..3 {
            pal[2][i] = ((p0[i] as u16 + p1[i] as u16) / 2) as u8;
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    } else {
        for i in 0..3 {
            pal[2][i] = ((2 * p0[i] as u16 + p1[i] as u16) / 3) as u8;
            pal[3][i] = ((p0[i] as u16 + 2 * p1[i] as u16) / 3) as u8;
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    }
    let bits = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
    let mut out = [[0u8; 4]; 16];
    for (i, texel) in out.iter_mut().enumerate() {
        *texel = pal[((bits >> (2 * i)) & 3) as usize];
    }
    if let Some(a) = alpha_src {
        match fmt {
            TexFormat::Dxt3 => {
                let abits = u64::from_le_bytes(a.try_into().unwrap());
                for (i, texel) in out.iter_mut().enumerate() {
                    texel[3] = (((abits >> (4 * i)) & 0xF) as u8) * 17;
                }
            }
            _ => {
                let t = dxt5_alpha_table(a[0], a[1]);
                let mut ab = 0u64;
                for k in 0..6 {
                    ab |= (a[2 + k] as u64) << (8 * k);
                }
                for (i, texel) in out.iter_mut().enumerate() {
                    texel[3] = t[((ab >> (3 * i)) & 7) as usize];
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- container walk

/// Walk a `.d4c` chunk file and return the MDXN (version 1) chunk data of every GEGR and GESR
/// package. Labels are "GEGR" and "GESR<k>" where k counts GESR packages from 0.
pub fn find_packages(path: impl AsRef<Path>) -> Result<Vec<(String, Vec<u8>)>, String> {
    let chunk = ChunkFile::load(path).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut gesr_index = 0usize;
    for node in &chunk.content.children {
        let Node::Package(pkg) = node else { continue };
        let label = if pkg.c.context == TAG_GEGR {
            "GEGR".to_string()
        } else if pkg.c.context == TAG_GESR {
            gesr_index += 1;
            format!("GESR{}", gesr_index - 1)
        } else {
            continue;
        };
        for child in &pkg.children {
            if let Node::Buffer(b) = child {
                if b.c.context == TAG_MDXN && b.c.version == MDXN_VERSION {
                    out.push((label.clone(), b.data().to_vec()));
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Where a package's MDXN data sits in the file (see `index_packages`).
#[derive(Debug, Clone)]
pub struct PackageRef {
    pub label: String,
    pub offset: u64,
    pub len: u64,
}

/// Reads `n` bytes at byte offset `pos` of the file.
fn rd_at(f: &mut std::fs::File, pos: u64, n: usize) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; n];
    f.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

/// One entry of a CHNK table: the context tag, the offset and size of the child, and its version.
struct Entry {
    context: i32,
    offset: i32,
    version: u8,
    size: i32,
}

/// The entries of the chunk that starts at `start` (header: "CHNK", size, count, version; then 16-byte entries).
fn chunk_entries(f: &mut std::fs::File, start: u64) -> Result<Vec<Entry>, String> {
    let h = rd_at(f, start, 16)?;
    if &h[0..4] != b"CHNK" {
        return Err("not a chunk".into());
    }
    let count = i32::from_le_bytes(h[8..12].try_into().unwrap());
    if h[15] & 0x80 != 0 {
        return Err("compressed chunk".into());
    }
    let raw = rd_at(f, start + 16, count.max(0) as usize * 16)?;
    Ok(raw
        .chunks(16)
        .map(|e| Entry { context: i32::from_le_bytes(e[0..4].try_into().unwrap()), offset: i32::from_le_bytes(e[4..8].try_into().unwrap()), version: e[8], size: i32::from_le_bytes(e[12..16].try_into().unwrap()) })
        .collect())
}

/// Finds the MDXN (version 1) package of the GEGR and of each GESR by reading only the chunk tables
/// (a few kilobytes), not the 900 MB file. Labels as in `find_packages`.
pub fn index_packages(path: impl AsRef<Path>) -> Result<Vec<PackageRef>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let mut out = Vec::new();
    let mut gesr = 0usize;
    for e in &top {
        let label = if e.context == TAG_GEGR {
            "GEGR".to_string()
        } else if e.context == TAG_GESR {
            gesr += 1;
            format!("GESR{}", gesr - 1)
        } else {
            continue;
        };
        let start = e.offset as u64;
        for c in chunk_entries(&mut f, start)? {
            if c.context == TAG_MDXN && c.version == MDXN_VERSION {
                out.push(PackageRef { label: label.clone(), offset: start + c.offset as u64, len: c.size as u64 });
                break;
            }
        }
    }
    Ok(out)
}

/// One record of the global GEBI: a terrain tile (one of the GEGR's big models) and where it sits in the city.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TilePlacement {
    /// Index into the global package's models
    pub model: u16,
    /// Offset added to the model's own coordinates (metres, X and Z; Y is unchanged)
    pub x: f32,
    pub z: f32,
}

/// The tiles of the city from the GEGR's GEBI v10 (16-byte records after a 0x18-byte header):
/// word C >> 16 is the model; words B and D carry the offset as `(word & 0xFFFFF) - 0x5A000` in 1/64 m
/// (0x5A000 in both means "already in place": the 26 models whose coordinates are baked in the city).
/// The comments in the source say that all 147 records name the 147 models larger than 300 m, and that this decoding is
/// the one of the eight sign and axis choices whose tiles overlap least. This file does not check that.
pub fn read_tiles(path: impl AsRef<Path>) -> Result<Vec<TilePlacement>, String> {
    let path = path.as_ref();
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let gegr = top.iter().find(|e| e.context == TAG_GEGR).ok_or("no GEGR")?;
    let start = gegr.offset as u64;
    let g = chunk_entries(&mut f, start)?.into_iter().find(|c| c.context == TAG_GEBI).ok_or("no GEBI in the GEGR")?;
    let d = rd_at(&mut f, start + g.offset as u64, g.size as usize)?;
    if d.len() < 0x18 || u32::from_le_bytes(d[0..4].try_into().unwrap()) != 0x18 {
        return Err("unexpected GEBI header".into());
    }
    let n = (d.len() - 0x18) / 16;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let r = &d[0x18 + i * 16..0x18 + i * 16 + 16];
        let w = |k: usize| u32::from_le_bytes(r[k * 4..k * 4 + 4].try_into().unwrap());
        let off = |v: u32| ((v & 0xFFFFF) as i32 - 0x5A000) as f32 / 64.0;
        out.push(TilePlacement { model: (w(2) >> 16) as u16, x: off(w(1)), z: off(w(3)) });
    }
    Ok(out)
}

/// One prop of a region's GEBI: a model of the global package standing somewhere in the city.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropPlacement {
    /// The region: the position of the GEIR chunk in the file, or 10000 + k for interior k.
    pub region: u16,
    /// Package of the model: 0 = global, otherwise 1 + the position of the super region id in `read_super_ids`.
    /// Interiors use `INTERIOR_PKG_BASE + k` for their own package (see `read_interior_props`).
    pub pkg: u16,
    /// Index into that package's models
    pub model: u16,
    pub x: f32,
    pub z: f32,
    /// Rotation about the vertical axis, radians
    pub yaw: f32,
    /// Height when the record's owner fixes it (interiors); otherwise the viewer uses the terrain
    pub y: Option<f32>,
}

/// The super region id of each GESR, in file order: the first word of its PROP or ANOD chunk, or the high half of the
/// second word of its ASRD chunk. Props name the super region whose package holds their model by this id.
pub fn read_super_ids(path: impl AsRef<Path>) -> Result<Vec<u32>, String> {
    let mut f = std::fs::File::open(path.as_ref()).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let mut out = Vec::new();
    for e in top.iter().filter(|e| e.context == TAG_GESR) {
        let start = e.offset as u64;
        let ch = chunk_entries(&mut f, start)?;
        let mut id = u32::MAX;
        for (tag, ver) in [(*b"PROP", 0), (*b"ANOD", 0), (*b"ASRD", 1)] {
            if let Some(c) = ch.iter().find(|c| c.context == i32::from_le_bytes(tag)) {
                let d = rd_at(&mut f, start + c.offset as u64, 8)?;
                let w = |k: usize| u32::from_le_bytes(d[k * 4..k * 4 + 4].try_into().unwrap());
                id = if ver == 0 { w(0) } else { w(1) >> 16 };
                break;
            }
        }
        out.push(id);
    }
    Ok(out)
}

/// The props of every region (GEIR) GEBI v10: the same 16-byte records as the tiles. Word C >> 16 is the model:
/// the low 10 bits are the index, the bits above are the id of the super region whose package holds it (0 = the global
/// package); words B and D give X and Z as for the tiles; word A holds the yaw as two i16 (cos, sin). Height is not
/// stored in the record: the viewer drops props onto the terrain.
pub fn read_props(path: impl AsRef<Path>) -> Result<Vec<PropPlacement>, String> {
    let path = path.as_ref();
    let ids = read_super_ids(path)?;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let mut out = Vec::new();
    let mut region = 0u16;
    for e in top.iter().filter(|e| e.context == TAG_GEIR) {
        let this = region;
        region += 1;
        let start = e.offset as u64;
        let Some(g) = chunk_entries(&mut f, start)?.into_iter().find(|c| c.context == TAG_GEBI) else { continue };
        let d = rd_at(&mut f, start + g.offset as u64, g.size as usize)?;
        if d.len() < 0x18 || u32::from_le_bytes(d[0..4].try_into().unwrap()) != 0x18 {
            continue;
        }
        for r in d[0x18..].chunks_exact(16) {
            let w = |k: usize| u32::from_le_bytes(r[k * 4..k * 4 + 4].try_into().unwrap());
            let id = (w(2) >> 16) as u16;
            let sr = (id >> 10) as u32;
            let pkg = if sr == 0 {
                0
            } else {
                match ids.iter().position(|&x| x == sr) {
                    Some(k) => k as u16 + 1,
                    None => continue,
                }
            };
            let off = |v: u32| ((v & 0xFFFFF) as i32 - 0x5A000) as f32 / 64.0;
            let a = w(0);
            let (c, s) = ((a & 0xFFFF) as u16 as i16 as f32, (a >> 16) as u16 as i16 as f32);
            out.push(PropPlacement { region: this, pkg, model: id & 0x3FF, x: off(w(1)), z: off(w(3)), yaw: s.atan2(c), y: None });
        }
    }
    Ok(out)
}

/// Interior numbering in `PropPlacement::pkg`: 1000 + k is the own package (MDXN) of interior k.
pub const INTERIOR_PKG_BASE: u16 = 1000;

/// The own model package of each interior (IRCT), in file order.
pub fn index_interiors(path: impl AsRef<Path>) -> Result<Vec<PackageRef>, String> {
    let mut f = std::fs::File::open(path.as_ref()).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let mut out = Vec::new();
    for (k, e) in top.iter().filter(|e| e.context == TAG_IRCT).enumerate() {
        let start = e.offset as u64;
        for c in chunk_entries(&mut f, start)? {
            if c.context == TAG_MDXN && c.version == MDXN_VERSION {
                out.push(PackageRef { label: format!("IRCT{k}"), offset: start + c.offset as u64, len: c.size as u64 });
                break;
            }
        }
    }
    Ok(out)
}

/// Entrance points of the interiors from the GEGL's GESI: (x, z, y) of each, as stored (the order is not the interiors').
fn read_entrances(f: &mut std::fs::File, top: &[Entry]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    let Some(g) = top.iter().find(|e| e.context == TAG_GEGL) else { return out };
    let start = g.offset as u64;
    let Ok(ch) = chunk_entries(f, start) else { return out };
    let Some(c) = ch.iter().find(|c| c.context == TAG_GESI) else { return out };
    let Ok(d) = rd_at(f, start + c.offset as u64, c.size as usize) else { return out };
    let fl = |i: usize| f32::from_le_bytes(d[i..i + 4].try_into().unwrap());
    let mut i = 0;
    while i + 16 <= d.len() {
        let (a, b, h, one) = (fl(i), fl(i + 4), fl(i + 8), fl(i + 12));
        if one == 1.0 && a.abs() > 50.0 && a.abs() < 6000.0 && b.abs() < 6000.0 && h.abs() < 400.0 {
            out.push([a, b, h]);
            i += 16;
        } else {
            i += 4;
        }
    }
    out
}

/// Everything the interiors place: for interior k, records whose super region id is 31 use the interior's own package
/// (`INTERIOR_PKG_BASE + k`), records with id 30 use package `19 + k`, and other ids use the super region list as in
/// `read_props`. The height comes from the entrance point nearest to the interior's first record.
pub fn read_interior_props(path: impl AsRef<Path>) -> Result<Vec<PropPlacement>, String> {
    let path = path.as_ref();
    let ids = read_super_ids(path)?;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let entr = read_entrances(&mut f, &top);
    let mut out = Vec::new();
    for (k, e) in top.iter().filter(|e| e.context == TAG_IRCT).enumerate() {
        let start = e.offset as u64;
        let Some(g) = chunk_entries(&mut f, start)?.into_iter().find(|c| c.context == TAG_GEBI) else { continue };
        let d = rd_at(&mut f, start + g.offset as u64, g.size as usize)?;
        if d.len() < 0x18 {
            continue;
        }
        let mut recs: Vec<PropPlacement> = Vec::new();
        for r in d[0x18..].chunks_exact(16) {
            let w = |j: usize| u32::from_le_bytes(r[j * 4..j * 4 + 4].try_into().unwrap());
            let id = (w(2) >> 16) as u16;
            let sr = (id >> 10) as u32;
            let pkg = match sr {
                31 => INTERIOR_PKG_BASE + k as u16,
                30 => 19 + k as u16,
                0 => 0,
                _ => match ids.iter().position(|&x| x == sr) {
                    Some(j) => j as u16 + 1,
                    None => continue,
                },
            };
            let off = |v: u32| ((v & 0xFFFFF) as i32 - 0x5A000) as f32 / 64.0;
            let a = w(0);
            let (c, s) = ((a & 0xFFFF) as u16 as i16 as f32, (a >> 16) as u16 as i16 as f32);
            recs.push(PropPlacement { region: 10000 + k as u16, pkg, model: id & 0x3FF, x: off(w(1)), z: off(w(3)), yaw: s.atan2(c), y: None });
        }
        let Some(first) = recs.first().copied() else { continue };
        let y = entr
            .iter()
            .map(|p| ((p[0] - first.x).powi(2) + (p[1] - first.z).powi(2), p[2]))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|(d2, _)| *d2 < 60.0 * 60.0)
            .map(|(_, h)| h);
        for mut r in recs {
            r.y = y;
            out.push(r);
        }
    }
    Ok(out)
}

/// EXPERIMENTAL, does not line up with the streets: road polylines (X, Z in metres) from the global AIEX v6. Only a guess at the point table: a 16-byte record is
/// `u32 x, u32 z, f32 length, u32 id` (section 1 of the offset table at word 18), x and z the low 24 bits minus 0x800000 in
/// centimetres (the top bytes are not decoded); two consecutive records of one group that are `length` apart are joined.
/// Lanes, junctions and links (the other sections) are not read.
pub fn read_roads(path: impl AsRef<Path>) -> Result<Vec<Vec<[f32; 2]>>, String> {
    let mut f = std::fs::File::open(path.as_ref()).map_err(|e| e.to_string())?;
    let top = chunk_entries(&mut f, 0)?;
    let gegr = top.iter().find(|e| e.context == TAG_GEGR).ok_or("no GEGR")?;
    let start = gegr.offset as u64;
    let a = chunk_entries(&mut f, start)?.into_iter().find(|c| c.context == i32::from_le_bytes(*b"AIEX")).ok_or("no AIEX")?;
    let d = rd_at(&mut f, start + a.offset as u64, a.size as usize)?;
    let w = |i: usize| u32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    let (s0, s1) = (w(19), w(20));
    if s1 > d.len() || s0 >= s1 {
        return Err("unexpected AIEX table".into());
    }
    let rec: Vec<(f32, f32, f32, u32)> = d[s0..s1]
        .chunks_exact(16)
        .map(|r| {
            let u = |k: usize| u32::from_le_bytes(r[k * 4..k * 4 + 4].try_into().unwrap());
            let c = |v: u32| ((v & 0xFF_FFFF) as i32 - 0x80_0000) as f32 / 100.0;
            (c(u(1)), c(u(0)), f32::from_bits(u(2)), u(3) >> 16)
        })
        .collect();
    let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    for i in 0..rec.len() {
        let (x, z, len, g) = rec[i];
        if cur.is_empty() {
            cur.push([x, z]);
        }
        let joined = rec.get(i + 1).map(|n| {
            let dist = ((n.0 - x).powi(2) + (n.1 - z).powi(2)).sqrt();
            n.3 == g && ((dist - len).abs() < 1.0 || (dist - n.2).abs() < 1.0)
        });
        if joined == Some(true) {
            cur.push([rec[i + 1].0, rec[i + 1].1]);
        } else {
            if cur.len() > 1 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    Ok(out)
}

/// Reads and parses one package found by `index_packages`.
pub fn load_package(path: impl AsRef<Path>, r: &PackageRef) -> Result<PlPackage, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    parse_package(&rd_at(&mut f, r.offset, r.len as usize)?)
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    /// The Parallel Lines city file: `PL_DIR` is the game folder. Tests that need it are skipped when it is not set.
    fn d4c() -> String {
        std::env::var("PL_DIR").map(|d| format!("{d}/Cities/nyc_now_pc.d4c")).unwrap_or_default()
    }

    /// Width and height of a DDS file, read from its header. Returns (width, height).
    fn dds_dims(dds: &[u8]) -> (u32, u32) {
        let h = u32::from_le_bytes(dds[12..16].try_into().unwrap());
        let w = u32::from_le_bytes(dds[16..20].try_into().unwrap());
        (w, h)
    }

    /// Checks the triangle indices, the DDS sizes and the RGBA decoding of a package, and prints a summary.
    /// Returns the model count and the triangle count.
    fn check_package(label: &str, pkg: &PlPackage) -> (usize, usize) {
        let mut tris = 0usize;
        let mut subs = 0usize;
        for m in &pkg.models {
            for sm in &m.submodels {
                subs += 1;
                assert!(sm.indices.len() % 3 == 0, "{label}: index count not a multiple of 3");
                let nv = sm.positions.len() as u32;
                for &i in &sm.indices {
                    assert!(i < nv, "{label}: triangle index {i} >= vertex count {nv}");
                }
                tris += sm.indices.len() / 3;
            }
        }
        for (ti, t) in pkg.textures.iter().enumerate() {
            let (dw, dh) = dds_dims(&t.dds);
            assert_eq!((dw, dh), (t.width, t.height), "{label}: texture {ti} DDS dims");
            let rgba = decode_rgba(t);
            assert_eq!(rgba.len(), (t.width * t.height * 4) as usize, "{label}: texture {ti} rgba size");
        }
        let with_tris = pkg.models.iter().filter(|m| m.submodels.iter().any(|s| !s.indices.is_empty())).count();
        let mut fmts = String::new();
        let mut counts = [0usize; 3];
        for t in &pkg.textures {
            counts[t.format as usize] += 1;
        }
        let _ = write!(fmts, "DXT1={} DXT3={} DXT5={}", counts[0], counts[1], counts[2]);
        eprintln!(
            "[{label}] uid={} models={} (with triangles {}) submodels={} triangles={} materials={} textures={} ({fmts})",
            pkg.uid,
            pkg.models.len(),
            with_tris,
            subs,
            tris,
            pkg.materials.len(),
            pkg.textures.len(),
        );
        (pkg.models.len(), tris)
    }

    /// The index finds 26 packages, and one of them loads. Skipped without PL_DIR.
    #[test]
    fn indexed_load_matches_full_load() {
        if !std::path::Path::new(&d4c()).exists() {
            eprintln!("no d4c, skipped");
            return;
        }
        let idx = index_packages(d4c()).unwrap();
        assert_eq!(idx.len(), 26);
        let p = load_package(d4c(), &idx[1]).unwrap();
        assert!(!p.models.is_empty() && !p.textures.is_empty());
        eprintln!("indexed {} packages; {} has {} models, {} textures", idx.len(), idx[1].label, p.models.len(), p.textures.len());
    }

    /// CRC-32, as used by PNG chunks.
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }

    /// Adler-32, the checksum of a zlib stream.
    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    /// Appends one PNG chunk: the length, the type, the data, and the CRC of the type and data.
    fn png_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = ty.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }

    /// Minimal PNG writer (RGBA8, stored deflate blocks). Avoids a new dependency.
    fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
        let mut raw = Vec::with_capacity((w * 4 + 1) * h);
        for y in 0..h {
            raw.push(0);
            raw.extend_from_slice(&rgba[y * w * 4..(y + 1) * w * 4]);
        }
        let mut z = vec![0x78, 0x01];
        let mut chunks = raw.chunks(65535).peekable();
        while let Some(ch) = chunks.next() {
            let last = chunks.peek().is_none();
            z.push(last as u8);
            let len = ch.len() as u16;
            z.extend_from_slice(&len.to_le_bytes());
            z.extend_from_slice(&(!len).to_le_bytes());
            z.extend_from_slice(ch);
        }
        z.extend_from_slice(&adler32(&raw).to_be_bytes());

        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(w as u32).to_be_bytes());
        ihdr.extend_from_slice(&(h as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, no filter, no interlace

        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png_chunk(&mut out, b"IHDR", &ihdr);
        png_chunk(&mut out, b"IDAT", &z);
        png_chunk(&mut out, b"IEND", &[]);
        std::fs::write(path, out)
    }

    /// Writes one model as OBJ. Coordinates are written as stored (model-local, Z-up, scaled).
    fn write_obj(path: &Path, m: &PlModel) -> std::io::Result<()> {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "# model uid {:?} scale {:?}; axes as stored: x, y, z (Z-up, no axis swap)",
            m.uid, m.scale
        );
        let mut base = 1usize;
        for (si, sm) in m.submodels.iter().enumerate() {
            let _ = writeln!(s, "g sub{si}");
            for p in &sm.positions {
                let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
            }
            for t in sm.indices.chunks(3) {
                let _ = writeln!(s, "f {} {} {}", base + t[0] as usize, base + t[1] as usize, base + t[2] as usize);
            }
            base += sm.positions.len();
        }
        std::fs::write(path, s)
    }

    /// Parses the GEGR and the first GESR package of the city file, checks their models and textures, and writes the
    /// first GEGR texture (PNG) and model (OBJ) to the temp folder for inspection. Skipped without PL_DIR.
    #[test]
    fn gegr_and_first_gesr_parse_and_decode() {
        let file = d4c();
        let path = Path::new(&file);
        if !path.exists() {
            eprintln!("skipping plmodel test: PL_DIR not set or {file} not found");
            return;
        }
        let pkgs = find_packages(path).expect("walk d4c");
        eprintln!("MDXN v1 packages found: {}", pkgs.len());
        assert!(pkgs.len() >= 2, "expected GEGR plus GESR packages");

        // The debug files go to the system temp folder. Nothing reads them back.
        let tmp = std::env::temp_dir();
        for want in ["GEGR", "GESR0"] {
            let (label, data) = pkgs.iter().find(|(l, _)| l == want).expect("package present");
            let pkg = parse_package(data).unwrap_or_else(|e| panic!("{label}: parse failed: {e}"));
            let (models, tris) = check_package(label, &pkg);
            assert!(models > 0, "{label}: no models");
            assert!(tris > 0, "{label}: no triangles");

            if want == "GEGR" {
                let t0 = pkg.textures.first().expect("GEGR has textures");
                let rgba = decode_rgba(t0);
                assert!(rgba.iter().any(|&b| b != 0), "first texture decoded to all zeros");
                let png = tmp.join("plmodel_gegr_tex0.png");
                write_png(&png, t0.width as usize, t0.height as usize, &rgba).expect("write png");
                eprintln!("debug PNG: {} ({}x{}, {:?}, uid {:#x})", png.display(), t0.width, t0.height, t0.format, t0.uid);

                let m0 = pkg.models.iter().find(|m| m.submodels.iter().any(|s| !s.indices.is_empty())).unwrap();
                let obj = tmp.join("plmodel_gegr_model0.obj");
                write_obj(&obj, m0).expect("write obj");
                eprintln!("debug OBJ: {} (model uid {:?})", obj.display(), m0.uid);
            }
        }
    }
}
