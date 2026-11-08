//! Vulkan renderer for the 3D views (map, camera preview, model boxes): wgpu restricted to its Vulkan backend.
//!
//! Policy: at start the machine is probed once (`support`). With a usable Vulkan adapter the renderer is enabled (the user may
//! switch it off in Settings, see `apply_preference`); without one it stays disabled, the setting is locked, and the CPU
//! renderer of `city3d` is used. The environment variable `ZM_FORCE_NO_VULKAN` makes the probe fail, for testing.
//! What it draws: the same kinds of thing as the CPU renderer: shading, mission lighting and fog, the sky dome, textures,
//! and the sea plane when the city has water. The frame is copied back to the CPU and returned as a `ColorImage`, so the
//! rest of the app does not care which renderer made it. Differences: all textures are packed into one 4096 by 4096
//! atlas, and when they do not fit they are halved (up to twice); a texture that still does not fit is drawn without
//! texture. Shading and the sea are computed in the shader, so the two pictures are close but not identical.
//! Uses: `city3d` (`Scene`, `Lighting`, `Tex`, and `set_gpu_hook` for registration).
//! Used by: `view3d` (`enabled`), `main.rs` (`install_hook` and `apply_preference` at start-up and from Settings, `support`
//! for the Settings text), and the `citysnap` tool.

use crate::city3d::{Lighting, Scene};
use eframe::egui::ColorImage;
use glam::{Mat4, Vec3};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;

/// Texture index meaning "no texture" (same value as in `city3d`).
const NO_TEX: u32 = u32::MAX;
/// Width and height of the texture atlas, in pixels.
const ATLAS: u32 = 4096;
/// Floats per vertex: position (3), texture coordinates (2), atlas rectangle (4), normal (3), cut-out flag (1).
const FLOATS: usize = 13;

/// Result of the probe: a usable Vulkan device (its description) or the reason there is none.
#[derive(Clone, Debug)]
pub enum Support {
    /// A usable Vulkan device: name and device type, for example "NVIDIA ... - Vulkan - DiscreteGpu".
    Available(String),
    /// No usable Vulkan device: the reason, as text.
    Unavailable(String),
}

/// A scene uploaded to the GPU: its vertex buffer, its atlas and its size.
struct GpuScene {
    /// Identity of the scene: pointer and length of its vertex list, first vertex, and texture count.
    key: (usize, usize, u32, usize),
    vb: wgpu::Buffer,
    /// Number of vertices in `vb`.
    count: u32,
    /// Bind group with the atlas texture and its sampler.
    tex_bg: wgpu::BindGroup,
    /// Largest distance of a vertex from the origin (used to scale the sky dome).
    radius: f32,
}

/// The off-screen picture of one size: colour texture, depth buffer and the buffer the pixels are copied to.
struct Target {
    w: u32,
    h: u32,
    color: wgpu::Texture,
    depth: wgpu::TextureView,
    /// CPU-readable buffer that receives the picture.
    readback: wgpu::Buffer,
    /// Bytes per row in `readback` (rows are padded to 256 bytes).
    row: u32,
}

/// The GPU state: device and queue, the three render pipelines, up to 8 uploaded scenes and the render target.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Device description, shown in Settings.
    info: String,
    /// Pipeline for scene triangles (`vs` and `fs`).
    scene_pipe: wgpu::RenderPipeline,
    /// Pipeline for the sea plane (`vs_water` and `fs_water`).
    water_pipe: wgpu::RenderPipeline,
    /// Pipeline for the full-screen background gradient (`vs_bg` and `fs_bg`).
    bg_pipe: wgpu::RenderPipeline,
    globals_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    cache: Vec<GpuScene>,
    target: Option<Target>,
}

// the probe result, made once: the GPU state or the reason it could not be made
static GPU: OnceLock<Result<Mutex<Gpu>, String>> = OnceLock::new();
// whether the GPU path is used now (probe succeeded and the user has not switched it off)
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Probes the machine once (Vulkan only) and returns what was found.
pub fn support() -> Support {
    match GPU.get_or_init(init) {
        Ok(g) => Support::Available(g.lock().map(|g| g.info.clone()).unwrap_or_default()),
        Err(e) => Support::Unavailable(e.clone()),
    }
}

/// Applies the user's choice: acceleration is on only when the machine supports it and the user did not switch it off.
pub fn apply_preference(user_off: bool) {
    let on = matches!(support(), Support::Available(_)) && !user_off;
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Registers the GPU path with the shared renderer.
pub fn install_hook() {
    crate::city3d::set_gpu_hook(render_vp);
}

/// Creates the Vulkan instance, the adapter and the device, then the shader and the three pipelines. Each step that
/// fails returns a short text that Settings shows.
fn init() -> Result<Mutex<Gpu>, String> {
    // test hook: behave like a machine without Vulkan
    if std::env::var("ZM_FORCE_NO_VULKAN").is_ok() {
        return Err("forced off for testing (ZM_FORCE_NO_VULKAN)".into());
    }
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|e| format!("no Vulkan adapter ({e})"))?;
    let ai = adapter.get_info();
    if matches!(ai.device_type, wgpu::DeviceType::Cpu) {
        return Err(format!("only a software Vulkan device was found ({})", ai.name));
    }
    let info = format!("{} \u{00B7} Vulkan \u{00B7} {:?}", ai.name, ai.device_type);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("zartex27"), required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), ..Default::default() }))
        .map_err(|e| format!("cannot open the Vulkan device ({e})"))?;
    // ---- shader and bind group layouts: group 0 is the uniform block, group 1 the atlas texture and its sampler
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("scene"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }],
    });
    let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("atlas"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    });
    // ---- pipelines: one layout for all, the vertex layout matches the 13 floats written by `Gpu::upload`
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("scene"), bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)], immediate_size: 0 });
    let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4, 3 => Float32x3, 4 => Float32];
    let vbuf = wgpu::VertexBufferLayout { array_stride: (FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs };
    // builds a pipeline that writes to the RGBA target and the depth buffer; no culling, no blending
    let make = |vs: &str, fs: &str, buffers: &[Option<wgpu::VertexBufferLayout>], depth_write: bool, compare: wgpu::CompareFunction| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fs),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(depth_write), depth_compare: Some(compare), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let scene_pipe = make("vs", "fs", &[Some(vbuf)], true, wgpu::CompareFunction::Less);
    let bg_pipe = make("vs_bg", "fs_bg", &[], false, wgpu::CompareFunction::Always);
    let water_pipe = make("vs_water", "fs_water", &[], true, wgpu::CompareFunction::Less);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    Ok(Mutex::new(Gpu { device, queue, info, scene_pipe, water_pipe, bg_pipe, globals_layout, tex_layout, sampler, cache: Vec::new(), target: None }))
}

/// Shelf-packs the scene's textures into one atlas; returns the RGBA data and, per texture, (x, y, w, h) in pixels.
/// Textures are placed left to right in rows, tallest first. When they do not fit, every texture is halved and the
/// packing is tried again (up to two times); a texture that still does not fit gets an empty rectangle.
fn pack(textures: &[crate::city3d::Tex]) -> (u32, u32, Vec<u8>, Vec<[u32; 4]>) {
    // try at full size, then with every texture halved, until it fits in ATLAS x ATLAS
    for shrink in 0..3u32 {
        let sizes: Vec<(u32, u32)> = textures.iter().map(|t| (((t.w as u32) >> shrink).max(1), ((t.h as u32) >> shrink).max(1))).collect();
        let mut order: Vec<usize> = (0..textures.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
        let (mut x, mut y, mut row_h) = (0u32, 0u32, 0u32);
        let mut rects = vec![[0u32; 4]; textures.len()];
        let mut ok = true;
        for &i in &order {
            let (w, h) = sizes[i];
            if x + w > ATLAS {
                x = 0;
                y += row_h;
                row_h = 0;
            }
            if y + h > ATLAS {
                ok = false;
                break;
            }
            rects[i] = [x, y, w, h];
            x += w;
            row_h = row_h.max(h);
        }
        if !ok && shrink < 2 {
            continue;
        }
        let height = (y + row_h).clamp(1, ATLAS);
        let mut data = vec![0u8; (ATLAS * height * 4) as usize];
        for (i, t) in textures.iter().enumerate() {
            let [rx, ry, rw, rh] = rects[i];
            if rw == 0 || rh == 0 {
                continue;
            }
            for yy in 0..rh {
                for xx in 0..rw {
                    // nearest-neighbour from the source when shrunk
                    let sx = ((xx as usize) << shrink).min(t.w - 1);
                    let sy = ((yy as usize) << shrink).min(t.h - 1);
                    let s = (sy * t.w + sx) * 4;
                    let d = (((ry + yy) * ATLAS + rx + xx) * 4) as usize;
                    if (ry + yy) < height {
                        data[d..d + 4].copy_from_slice(&t.rgba[s..s + 4]);
                    }
                }
            }
        }
        return (ATLAS, height, data, rects);
    }
    (1, 1, vec![255; 4], Vec::new())
}

impl Gpu {
    /// Uploads a scene (its atlas texture and its vertex buffer) unless the same scene is already in the cache, and
    /// returns its index in the cache. The cache keeps the 8 most recent scenes.
    fn upload(&mut self, scene: &Scene) -> usize {
        let key = (scene.verts.as_ptr() as usize, scene.verts.len(), scene.verts.first().map(|v| v.x.to_bits()).unwrap_or(0), scene.textures.len());
        if let Some(i) = self.cache.iter().position(|c| c.key == key) {
            return i;
        }
        let (aw, ah, data, rects) = pack(&scene.textures);
        let atlas = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("atlas"),
                size: wgpu::Extent3d { width: aw, height: ah, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &data,
        );
        let view = atlas.create_view(&Default::default());
        let tex_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas"),
            layout: &self.tex_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
        });
        let mut verts: Vec<[f32; FLOATS]> = Vec::with_capacity(scene.verts.len());
        let mut radius = 1.0f32;
        for (ti, tri) in scene.verts.chunks_exact(3).enumerate() {
            let mut n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
            n = if n.length() > 1e-9 { n.normalize() } else { Vec3::Y };
            let raw = scene.tri_tex.get(ti).copied().filter(|&t| t != NO_TEX);
            let (rect, cut) = match raw {
                Some(r) => {
                    let id = (r & 0x7FFF_FFFF) as usize;
                    match (rects.get(id), scene.textures.get(id)) {
                        (Some(rc), Some(t)) => ([rc[0] as f32 / aw as f32, rc[1] as f32 / ah as f32, rc[2] as f32 / aw as f32, rc[3] as f32 / ah as f32], if r >> 31 == 1 && t.alpha { 1.0 } else { 0.0 }),
                        _ => ([0.0; 4], 0.0),
                    }
                }
                None => ([0.0; 4], 0.0),
            };
            for k in 0..3 {
                let p = tri[k];
                if p.is_finite() {
                    radius = radius.max(p.length());
                }
                let uv = scene.uvs.get(ti * 3 + k).copied().unwrap_or([0.0, 0.0]);
                // per vertex: position, texture coordinates, atlas rectangle (in 0..1 of the atlas), the face normal, cut-out flag
                verts.push([p.x, p.y, p.z, uv[0], uv[1], rect[0], rect[1], rect[2], rect[3], n.x, n.y, n.z, cut]);
            }
        }
        // an atlas of height `ah` is sampled with v in 0..1 of its own height: rects above already use it
        let vb = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("vertices"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
        if self.cache.len() >= 8 {
            self.cache.remove(0);
        }
        self.cache.push(GpuScene { key, vb, count: verts.len() as u32, tex_bg, radius });
        self.cache.len() - 1
    }

    /// Makes the render target `w` by `h` pixels. It is created again only when the size changes.
    fn target(&mut self, w: u32, h: u32) {
        if self.target.as_ref().map(|t| (t.w, t.h)) == Some((w, h)) {
            return;
        }
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("target"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = self
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let row = (w * 4).next_multiple_of(256);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        self.target = Some(Target { w, h, color, depth, readback, row });
    }

    /// Builds the uniform block of one draw (48 floats, the struct `U` of the shader): view-projection matrix, eye
    /// position and sky scale, lighting and fog when `light` is given, the sea height when `water` is given, the mode
    /// flags (lit, sky, frame height) and the background top colour. `sky_k` scales the sky dome; `is_sky` marks it.
    fn globals(&self, vp: &Mat4, eye: Vec3, h: u32, light: Option<&Lighting>, sky_k: f32, is_sky: bool, bg: ([f32; 3], [f32; 3]), water: Option<f32>) -> wgpu::BindGroup {
        let mut u = [0f32; 48];
        u[..16].copy_from_slice(&vp.to_cols_array());
        u[16..20].copy_from_slice(&[eye.x, eye.y, eye.z, sky_k]);
        if let Some(l) = light {
            u[20..24].copy_from_slice(&[l.ambient[0], l.ambient[1], l.ambient[2], 0.0]);
            u[24..28].copy_from_slice(&[l.dir_col[0], l.dir_col[1], l.dir_col[2], 0.0]);
            u[28..32].copy_from_slice(&[l.dir_vec.x, l.dir_vec.y, l.dir_vec.z, 0.0]);
            u[32..36].copy_from_slice(&[l.fog_near, l.fog_far, l.fog_near_v, l.fog_far_v]);
            u[36..40].copy_from_slice(&[l.fog_col[0], l.fog_col[1], l.fog_col[2], 0.0]);
        }
        if let Some(level) = water {
            u[23] = level - 0.4;
            u[27] = 1.0;
        }
        u[40..44].copy_from_slice(&[if light.is_some() { 1.0 } else { 0.0 }, if is_sky { 1.0 } else { 0.0 }, h as f32, 0.0]);
        u[44..48].copy_from_slice(&[bg.0[0], bg.0[1], bg.0[2], 0.0]);
        // the background gradient has its own uniform block, built in `render`
        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("globals"), contents: bytemuck::cast_slice(&u), usage: wgpu::BufferUsages::UNIFORM });
        // the bottom colour of the background (`bg.1`) is not used by these uniforms
        let _ = bg.1;
        self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("globals"), layout: &self.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }] })
    }

    /// Draws one frame: background gradient, sky dome, then the sea (when the city has water) and the scene, into the
    /// off-screen target. The picture is copied to a buffer and read back to the CPU. `None` when the size is outside
    /// 1 to 8192 pixels or the read-back fails.
    fn render(&mut self, scene: &Scene, vp: &Mat4, eye: Vec3, w: usize, h: usize, sky: Option<&Scene>, light: Option<&Lighting>) -> Option<ColorImage> {
        let (w, h) = (w as u32, h as u32);
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            return None;
        }
        self.target(w, h);
        let wi = self.upload(scene);
        let si = sky.filter(|s| !s.verts.is_empty()).map(|s| self.upload(s));
        // the upload of the second scene may have evicted the first entry: find them again by key
        let wi = if si.is_some() { self.upload(scene) } else { wi };
        let si = sky.filter(|s| !s.verts.is_empty()).map(|s| self.upload(s));
        // background gradient: the mood's fog colour when there is lighting, the plain pale sky otherwise
        let (top, bot) = match light {
            Some(l) => (l.fog_col, l.fog_col),
            None => ([0xB9 as f32 / 255.0, 0xD3 as f32 / 255.0, 0xEA as f32 / 255.0], [0xEE as f32 / 255.0, 0xF3 as f32 / 255.0, 0xF8 as f32 / 255.0]),
        };
        let bg_bot = {
            // the background uniform: the top colour goes in `top`, the bottom colour in `ambient` (see `fs_bg`)
            let mut u = [0f32; 48];
            u[40..44].copy_from_slice(&[0.0, 0.0, h as f32, 0.0]);
            u[44..48].copy_from_slice(&[top[0], top[1], top[2], 0.0]);
            u[20..24].copy_from_slice(&[bot[0], bot[1], bot[2], 0.0]);
            let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("bg"), contents: bytemuck::cast_slice(&u), usage: wgpu::BufferUsages::UNIFORM });
            self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("bg"), layout: &self.globals_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }] })
        };
        // the sky dome is scaled so that it sits inside the far plane (2500 units), and is unlit
        let sky_globals = si.map(|i| self.globals(vp, eye, h, None, 2500.0 / self.cache[i].radius.max(1.0), true, (top, bot), None));
        let world_globals = self.globals(vp, eye, h, light, 1.0, false, (top, bot), scene.water_level);
        let t = self.target.as_ref()?;
        let view = t.color.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&Default::default());
        let depth_att = |load: wgpu::LoadOp<f32>| wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: Some(wgpu::Operations { load, store: wgpu::StoreOp::Store }), stencil_ops: None };
        // ---- pass 1: background gradient, then the sky dome (depth is cleared first)
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("background and sky"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(depth_att(wgpu::LoadOp::Clear(1.0))),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.bg_pipe);
            pass.set_bind_group(0, &bg_bot, &[]);
            pass.set_bind_group(1, &self.cache[wi].tex_bg, &[]);
            pass.draw(0..3, 0..1);
            if let (Some(i), Some(g)) = (si, sky_globals.as_ref()) {
                pass.set_pipeline(&self.scene_pipe);
                pass.set_bind_group(0, g, &[]);
                pass.set_bind_group(1, &self.cache[i].tex_bg, &[]);
                pass.set_vertex_buffer(0, self.cache[i].vb.slice(..));
                pass.draw(0..self.cache[i].count, 0..1);
            }
        }
        // ---- pass 2: the sea (drawn before the scene, with the depth test), then the scene, on top of pass 1
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(depth_att(wgpu::LoadOp::Clear(1.0))),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if scene.water_level.is_some() {
                // the sea: a plane under everything, with ripples, sky reflection and sun glint
                pass.set_pipeline(&self.water_pipe);
                pass.set_bind_group(0, &world_globals, &[]);
                pass.set_bind_group(1, &self.cache[wi].tex_bg, &[]);
                pass.draw(0..6, 0..1);
            }
            if self.cache[wi].count > 0 {
                pass.set_pipeline(&self.scene_pipe);
                pass.set_bind_group(0, &world_globals, &[]);
                pass.set_bind_group(1, &self.cache[wi].tex_bg, &[]);
                pass.set_vertex_buffer(0, self.cache[wi].vb.slice(..));
                pass.draw(0..self.cache[wi].count, 0..1);
            }
        }
        // ---- read-back: copy the picture to the CPU-readable buffer and wait for the GPU to finish
        enc.copy_texture_to_buffer(
            t.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &t.readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = t.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| {
            let _ = r;
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            rgba.extend_from_slice(&mapped[y * t.row as usize..y * t.row as usize + (w * 4) as usize]);
        }
        drop(mapped);
        t.readback.unmap();
        Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba))
    }
}

/// The GPU version of `city3d::render_vp`; `None` when acceleration is off or the frame could not be drawn.
pub fn render_vp(scene: &Scene, vp: &Mat4, eye: Vec3, w: usize, h: usize, sky: Option<&Scene>, light: Option<&Lighting>) -> Option<ColorImage> {
    if !enabled() {
        return None;
    }
    let g = GPU.get()?.as_ref().ok()?;
    let mut g = g.lock().ok()?;
    g.render(scene, vp, eye, w, h, sky, light)
}

/// WGSL source of the three pipelines. `vs` and `fs` draw the scene triangles (textured from the atlas, lit by the
/// mission lighting and fogged when it is given; the sky dome uses the same pipeline unlit). `vs_water` and `fs_water`
/// draw a large plane at the sea height with ripples, reflection and a sun glint. `vs_bg` and `fs_bg` draw a full-screen
/// vertical gradient. The uniform struct `U` is filled by `Gpu::globals` and `Gpu::render`.
const SHADER: &str = r#"
struct U {
    vp: mat4x4<f32>,
    eye: vec4<f32>,      // xyz camera, w sky scale
    ambient: vec4<f32>,  // (background: bottom colour)
    dir_col: vec4<f32>,
    dir_vec: vec4<f32>,
    fog: vec4<f32>,      // near, far, near value, far value
    fog_col: vec4<f32>,
    mode: vec4<f32>,     // x lighting, y sky, z target height
    top: vec4<f32>,      // background top colour
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) rect: vec4<f32>,
    @location(3) nrm: vec3<f32>,
    @location(4) cut: f32,
};

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) rect: vec4<f32>, @location(3) n: vec3<f32>, @location(4) cut: f32) -> VOut {
    var o: VOut;
    var w = p;
    if (u.mode.y > 0.5) {
        w = u.eye.xyz + p * u.eye.w;
    }
    o.world = w;
    o.pos = u.vp * vec4<f32>(w, 1.0);
    o.uv = uv;
    o.rect = rect;
    o.nrm = n;
    o.cut = cut;
    return o;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    var n = normalize(in.nrm);
    if (dot(n, u.eye.xyz - in.world) < 0.0) {
        n = -n;
    }
    var base = vec3<f32>(0.80, 0.77, 0.72);
    if (n.y > 0.85) {
        base = vec3<f32>(0.62, 0.64, 0.66);
    }
    if (in.rect.z > 0.0) {
        let f = fract(in.uv);
        let t = in.rect.xy + f * in.rect.zw;
        let s = textureSampleLevel(atlas, samp, t, 0.0);
        if (in.cut > 0.5 && s.a < 0.5) {
            discard;
        }
        base = s.rgb;
    }
    var shade = vec3<f32>(0.5 + 0.5 * max(dot(n, normalize(vec3<f32>(0.4, 0.8, 0.45))), 0.0));
    let lit = u.mode.x > 0.5 && u.mode.y < 0.5;
    if (lit) {
        let ndl = max(dot(n, -normalize(u.dir_vec.xyz)), 0.0);
        shade = min(u.ambient.xyz + u.dir_col.xyz * ndl, vec3<f32>(1.4));
    }
    if (u.mode.y > 0.5) {
        shade = vec3<f32>(1.0);
    }
    var col = min(base * shade, vec3<f32>(1.0));
    if (lit) {
        let d = 1.0 / in.pos.w;
        let t = clamp((d - u.fog.x) / max(u.fog.y - u.fog.x, 1.0), 0.0, 1.0);
        let a = clamp((u.fog.z + (u.fog.w - u.fog.z) * t) / 100.0, 0.0, 1.0);
        col = mix(col, clamp(u.fog_col.xyz, vec3<f32>(0.0), vec3<f32>(1.0)), a);
    }
    return vec4<f32>(col, 1.0);
}

@vertex
fn vs_water(@builtin(vertex_index) i: u32) -> VOut {
    var corners = array<vec2<f32>, 6>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let c = corners[i] * 9000.0;
    let w = vec3<f32>(u.eye.x + c.x, u.ambient.w, u.eye.z + c.y);
    var o: VOut;
    o.world = w;
    o.pos = u.vp * vec4<f32>(w, 1.0);
    o.uv = vec2<f32>(0.0);
    o.rect = vec4<f32>(0.0);
    o.nrm = vec3<f32>(0.0, 1.0, 0.0);
    o.cut = 0.0;
    return o;
}

@fragment
fn fs_water(in: VOut) -> @location(0) vec4<f32> {
    let p = in.world.xz;
    // small ripples from a few sines of the position: a perturbed normal
    // ripples fade with distance so that far water does not shimmer
    let dist = 1.0 / in.pos.w;
    let amp = 1.0 / (1.0 + dist * 0.03);
    let nx = (sin(p.x * 0.11 + p.y * 0.07) * 0.10 + sin(p.y * 0.23 - p.x * 0.09) * 0.06) * amp;
    let nz = (cos(p.y * 0.09 - p.x * 0.08) * 0.10 + cos(p.x * 0.19 + p.y * 0.13) * 0.06) * amp;
    let n = normalize(vec3<f32>(nx, 1.0, nz));
    let v = normalize(u.eye.xyz - in.world);
    let f = 0.08 + 0.85 * pow(1.0 - max(dot(n, v), 0.0), 4.0);
    let lit = u.mode.x > 0.5;
    var deep = vec3<f32>(0.04, 0.20, 0.32);
    var sky = vec3<f32>(0.60, 0.74, 0.88);
    var sun_dir = normalize(vec3<f32>(0.4, 0.8, 0.45));
    var gain = vec3<f32>(1.0);
    if (lit) {
        sky = clamp(u.fog_col.xyz, vec3<f32>(0.0), vec3<f32>(1.0));
        sun_dir = -normalize(u.dir_vec.xyz);
        gain = min(u.ambient.xyz + u.dir_col.xyz * 0.6, vec3<f32>(1.2));
        deep = deep * 1.0;
    }
    var col = mix(deep, sky, f) * gain;
    let r = reflect(-sun_dir, n);
    let spec = pow(max(dot(r, v), 0.0), 90.0) * 0.9;
    col = col + vec3<f32>(spec) * select(vec3<f32>(1.0), u.dir_col.xyz, lit);
    col = min(col, vec3<f32>(1.0));
    if (lit) {
        let d = 1.0 / in.pos.w;
        let t = clamp((d - u.fog.x) / max(u.fog.y - u.fog.x, 1.0), 0.0, 1.0);
        let a = clamp((u.fog.z + (u.fog.w - u.fog.z) * t) / 100.0, 0.0, 1.0);
        col = mix(col, sky, a);
    } else {
        // far water melts into the pale sky of the unlit view
        let d = 1.0 / in.pos.w;
        col = mix(col, vec3<f32>(0.84, 0.90, 0.96), clamp((d - 400.0) / 3500.0, 0.0, 0.8));
    }
    return vec4<f32>(col, 1.0);
}

struct BgOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    var o: BgOut;
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    o.pos = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    let t = clamp(in.pos.y / max(u.mode.z, 1.0), 0.0, 1.0);
    return vec4<f32>(mix(u.top.xyz, u.ambient.xyz, t), 1.0);
}
"#;
