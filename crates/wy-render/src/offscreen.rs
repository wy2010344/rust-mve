//! 离屏渲染：把 [`Scene`] 渲染成一张 PNG，**不需要窗口**。
//!
//! # 为什么需要它
//!
//! 窗口渲染（[`crate::vello_executor`] → Vello → surface）只能给人看，
//! 但有两类活儿它做不了：
//!
//! - **文档配图**：README 里的截图必须是**可复现**的——同样的代码跑两次
//!   要出同样的图。手写截图工具随时会过期，CI 里跑一遍 offscreen 渲染
//!   出来的图则永远是真的。
//! - **无头环境**：CI 服务器、容器、远程 SSH 都没有显示器。没有离屏渲染，
//!   框架就只能"相信它能跑"，而不能"证明它能跑"。
//!
//! # 用法
//!
//! ```no_run
//! use wy_render::{offscreen, Color, Rect, Scene};
//!
//! let mut scene = Scene::new();
//! scene.fill_round_rect(
//!     Rect::new(0.0, 0.0, 200.0, 100.0),
//!     8.0,
//!     Color::from_u32(0x1F6FEBFF),
//! );
//! let png = offscreen::render_to_png(&scene, 200, 100).unwrap();
//! std::fs::write("demo.png", png).unwrap();
//! ```
//!
//! # 限制
//!
//! - 需要一个可用的 wgpu 适配器（无显卡的机器会失败，返回 [`OffscreenError::NoAdapter`]）；
//! - 文本渲染依赖系统字体，走的是与窗口渲染完全相同的 Parley 路径，
//!   所以排版结果一致。

use std::path::Path;

use crate::vello_executor::TextLayoutCache;
use crate::Scene;

/// 离屏渲染失败的原因。
#[derive(Debug)]
pub enum OffscreenError {
    /// 找不到可用的 GPU 适配器（无显卡 / 驱动缺失 / 容器里没有 GPU）。
    NoAdapter,
    /// 创建 wgpu 设备失败。
    Device(String),
    /// 创建 Vello 渲染器失败。
    Renderer(String),
    /// 渲染到纹理失败。
    Render(String),
    /// 写 PNG 失败。
    Encode(String),
    /// 读回像素失败。
    Readback(String),
}

impl std::fmt::Display for OffscreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffscreenError::NoAdapter => write!(f, "没有可用的 GPU 适配器"),
            OffscreenError::Device(e) => write!(f, "创建 GPU 设备失败：{e}"),
            OffscreenError::Renderer(e) => write!(f, "创建 Vello 渲染器失败：{e}"),
            OffscreenError::Render(e) => write!(f, "渲染失败：{e}"),
            OffscreenError::Encode(e) => write!(f, "PNG 编码失败：{e}"),
            OffscreenError::Readback(e) => write!(f, "像素回读失败：{e}"),
        }
    }
}

impl std::error::Error for OffscreenError {}

/// 把 [`Scene`] 渲染成 PNG 字节流。
///
/// `width`/`height` 是**物理像素**（Scene 坐标按 1:1 渲染）。
/// 背景是透明的（alpha = 0），需要不透明背景时在 Scene 里自己画一个矩形。
pub fn render_to_png(scene: &Scene, width: u32, height: u32) -> Result<Vec<u8>, OffscreenError> {
    let rgba = render_to_rgba(scene, width, height)?;
    encode_png(&rgba, width, height)
}

/// 把 [`Scene`] 渲染成 RGBA8 像素（每行 4 字节，紧密排列）。
pub fn render_to_rgba(scene: &Scene, width: u32, height: u32) -> Result<Vec<u8>, OffscreenError> {
    let width = width.max(1);
    let height = height.max(1);

    // ---- wgpu 设备 ----
    let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_desc.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .map_err(|_| OffscreenError::NoAdapter)?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("wy-ui offscreen device"),
        ..Default::default()
    }))
    .map_err(|e| OffscreenError::Device(e.to_string()))?;

    // ---- Vello 渲染目标（Rgba8Unorm，可回读）----
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("wy-ui offscreen target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut renderer = vello::Renderer::new(
        &device,
        vello::RendererOptions {
            ..Default::default()
        },
    )
    .map_err(|e| OffscreenError::Renderer(e.to_string()))?;

    // ---- Scene → Vello Scene（与窗口渲染完全同一条路径）----
    let mut vello_scene = vello::Scene::new();
    let mut font_cx = parley::FontContext::new();
    let mut layout_cx = parley::LayoutContext::new();
    let mut text_cache = TextLayoutCache::new();
    crate::vello_executor::execute_scene(
        scene,
        &mut vello_scene,
        &mut font_cx,
        &mut layout_cx,
        &mut text_cache,
        1.0,
    );

    let params = vello::RenderParams {
        base_color: vello::peniko::Color::TRANSPARENT,
        width,
        height,
        antialiasing_method: vello::AaConfig::Area,
    };
    renderer
        .render_to_texture(&device, &queue, &vello_scene, &view, &params)
        .map_err(|e| OffscreenError::Render(e.to_string()))?;

    // ---- 回读像素 ----
    let bytes_per_row = width * 4;
    // wgpu 的回读要求每行字节数是 256 的倍数
    let padded_bytes_per_row = bytes_per_row.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wy-ui offscreen readback"),
        size: (padded_bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("wy-ui offscreen copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok();
    rx.recv()
        .map_err(|e| OffscreenError::Readback(e.to_string()))?
        .map_err(|e| OffscreenError::Readback(e.to_string()))?;

    let mapped = slice.get_mapped_range();
    // 去掉行尾填充
    let mut rgba = Vec::with_capacity((bytes_per_row * height) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + bytes_per_row as usize;
        rgba.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(rgba)
}

/// RGBA8 像素 → PNG 字节流。
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, OffscreenError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| OffscreenError::Encode(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| OffscreenError::Encode(e.to_string()))?;
    }
    Ok(out)
}

/// 把 [`Scene`] 渲染成 PNG 并写到文件。
pub fn render_to_file(
    scene: &Scene,
    width: u32,
    height: u32,
    path: impl AsRef<Path>,
) -> Result<(), OffscreenError> {
    let png = render_to_png(scene, width, height)?;
    std::fs::write(path, png).map_err(|e| OffscreenError::Encode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Rect};

    /// 离屏渲染需要一个真实 GPU 适配器，没有就跳过（不能因此让 CI 变红）。
    #[test]
    fn renders_a_filled_rect_to_png() {
        let mut scene = Scene::new();
        scene.fill_rect(Rect::new(0.0, 0.0, 32.0, 16.0), Color::from_u32(0xFF0000FF));

        let png = match render_to_png(&scene, 32, 16) {
            Ok(p) => p,
            Err(OffscreenError::NoAdapter) => {
                eprintln!("跳过：没有 GPU 适配器");
                return;
            }
            Err(e) => panic!("离屏渲染失败：{e}"),
        };
        // PNG magic number
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "输出应是合法 PNG");
        assert!(png.len() > 64, "PNG 不应为空：{}", png.len());
    }
}
