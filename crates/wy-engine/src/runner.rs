//! 应用运行器：winit 事件循环 + wgpu surface + Vello 渲染。
//!
//! 提供 [`WyApp`] trait 供用户实现应用逻辑，[`run`] 函数启动事件循环。
//!
//! # 示例
//!
//! ```ignore
//! use wy_engine::runner::{WyApp, run};
//! use wy_render::Scene;
//!
//! struct MyApp;
//!
//! impl WyApp for MyApp {
//!     fn draw(&self, scene: &mut Scene, width: f32, height: f32) {
//!         scene.fill_rect(
//!             wy_render::Rect::new(0.0, 0.0, width, height),
//!             wy_render::Color::WHITE,
//!         );
//!     }
//! }
//!
//! fn main() {
//!     run(MyApp).unwrap();
//! }
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowId};

use crate::frame_source::WinitFrameSource;
use wy_render::{vello_executor, Scene};

// Blit shader: 将 Rgba8Unorm 中间纹理 blit 到任意 surface 格式
const BLIT_WGSL: &str = r#"
@vertex fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2(3.0, -1.0),
        vec2(-1.0, 3.0),
    );
    return vec4<f32>(pos[idx], 0.0, 1.0);
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

@fragment fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = textureDimensions(src_tex);
    let uv = pos.xy / vec2<f32>(f32(dims.x), f32(dims.y));
    return textureSample(src_tex, src_sampler, uv);
}
"#;

// 当前活跃的帧源（由 runner 设置，供应用代码读取）。
thread_local! {
    static CURRENT_FRAME_SOURCE: std::cell::RefCell<Option<WinitFrameSource>> = const { std::cell::RefCell::new(None) };
}

/// 获取当前帧源的克隆（供 `AnimateSignal` 等使用）。
///
/// 必须在 `WyApp::setup()` 之后调用（即窗口创建后），
/// 否则返回 `None`。
pub fn current_frame_source() -> Option<WinitFrameSource> {
    CURRENT_FRAME_SOURCE.with(|cell| cell.borrow().clone())
}

/// 应用事件类型（通过 EventLoopProxy 分发）。
pub enum AppEvent {
    /// AccessKit 无障碍事件。
    AccessKit(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for AppEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        AppEvent::AccessKit(event)
    }
}

/// 用户实现的应用接口。
///
/// 实现此 trait 即可获得完整的 winit + wgpu + Vello 渲染管线。
pub trait WyApp {
    /// 绘制一帧（只读）。
    ///
    /// 在此方法中使用 `scene` 记录绘制命令（矩形、文本等）。
    /// `width`/`height` 是窗口客户区的像素尺寸。
    ///
    /// 注意：`draw()` 接收 `&self`（不可变引用），
    /// 保证绘制过程中不修改应用状态。
    /// 状态变更应通过信号（`Signal::set()`）触发。
    fn draw(&self, scene: &mut Scene, width: f32, height: f32);

    /// 初始化应用（可选）。
    ///
    /// 窗口创建后调用。`request_redraw` 可用于触发按需重绘，
    /// 通常与信号系统的 `TrackEffect` 配合使用：
    /// ```ignore
    /// fn setup(&mut self, request_redraw: Rc<dyn Fn()>) {
    ///     let counter = self.counter.clone();
    ///     create_effect(move || {
    ///         let _ = counter.get(); // 注册依赖
    ///         request_redraw();     // 信号变更时触发重绘
    ///     });
    /// }
    /// ```
    fn setup(&mut self, _request_redraw: Rc<dyn Fn()>) {}

    /// 提供 WidgetTree 用于自动命中测试和事件分发（可选）。
    ///
    /// 返回 `Some(&mut WidgetTree)` 时，runner 会自动将鼠标事件
    /// 分发到 WidgetTree（dispatch_pointer_down/up/move），
    /// 并在渲染时调用 `draw_scene()` 生成 Scene。
    ///
    /// 返回 `None`（默认）时，runner 使用 `draw()` 直接绘制。
    fn widget_tree(&mut self) -> Option<&mut wy_render::widget_tree::WidgetTree> {
        None
    }

    /// 处理窗口事件（可选）。
    ///
    /// 返回 `true` 表示事件已消费，不再传播。
    fn handle_event(&mut self, _event: &winit::event::WindowEvent) -> bool {
        false
    }

    /// 窗口大小改变时调用（可选）。
    fn on_resize(&mut self, _width: f32, _height: f32) {}

    /// 处理键盘事件（可选）。
    ///
    /// Tab/Shift+Tab 用于焦点遍历，其他按键可自定义处理。
    fn handle_key_event(&mut self, _event: &crate::event::KeyEvent) {}

    /// 提供无障碍树更新（可选）。
    ///
    /// 每次渲染后调用。返回 `Some(TreeUpdate)` 会更新平台无障碍树。
    /// 使用 `AccessibilityBridge::to_tree_update()` 生成更新。
    fn accessibility_update(&mut self) -> Option<accesskit::TreeUpdate> {
        None
    }

    /// 处理无障碍动作请求（可选）。
    ///
    /// 当屏幕阅读器等辅助技术触发动作时调用（如点击按钮、聚焦输入框）。
    fn handle_accessibility_action(&mut self, _request: accesskit::ActionRequest) {}
}

/// 启动应用事件循环。
///
/// 创建窗口、初始化 wgpu/Vello，然后运行渲染循环。
/// 此函数阻塞直到窗口关闭。
pub fn run(app: impl WyApp + 'static) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();

    let frame_source = WinitFrameSource::new();

    let mut state = AppState {
        app,
        window: None,
        renderer: None,
        surface: None,
        device: None,
        queue: None,
        config: None,
        size: (800, 600),
        modifiers: ModifiersState::empty(),
        cursor_pos: (0.0, 0.0),
        font_cx: parley::FontContext::new(),
        layout_cx: parley::LayoutContext::new(),
        text_cache: wy_render::vello_executor::TextLayoutCache::new(),
        needs_redraw: Rc::new(Cell::new(true)),
        access_adapter: None,
        proxy,
        redraw_tracker: None,
        frame_source,
        vello_target: None,
        vello_target_view: None,
        blit_pipeline: None,
        blit_bind_group_layout: None,
    };

    event_loop.run_app(&mut state)?;
    Ok(())
}

/// 应用内部状态。
struct AppState<A: WyApp> {
    app: A,
    window: Option<Arc<Window>>,
    renderer: Option<vello::Renderer>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    config: Option<wgpu::SurfaceConfiguration>,
    size: (u32, u32),
    modifiers: ModifiersState,
    cursor_pos: (f32, f32),
    font_cx: parley::FontContext,
    layout_cx: parley::LayoutContext,
    text_cache: wy_render::vello_executor::TextLayoutCache,
    needs_redraw: Rc<Cell<bool>>,
    access_adapter: Option<accesskit_winit::Adapter>,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    redraw_tracker: Option<crate::redraw_tracker::RedrawTracker>,
    frame_source: WinitFrameSource,
    /// Vello 中间渲染目标（Rgba8Unorm），用于兼容不支持 Rgba8Unorm 的 surface。
    vello_target: Option<wgpu::Texture>,
    vello_target_view: Option<wgpu::TextureView>,
    /// Blit 管线：将 Rgba8Unorm 中间纹理 blit 到 surface 纹理。
    blit_pipeline: Option<wgpu::RenderPipeline>,
    blit_bind_group_layout: Option<wgpu::BindGroupLayout>,
}

impl<A: WyApp> ApplicationHandler<AppEvent> for AppState<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // 创建窗口（先隐藏，以便 AccessKit adapter 在显示前创建）
        let attrs = Window::default_attributes()
            .with_title("wy-ui")
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
            .with_visible(false);
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let size = window.inner_size();
        self.size = (size.width, size.height);

        // 创建 AccessKit adapter（必须在窗口显示前）
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.proxy.clone(),
        );
        self.access_adapter = Some(adapter);

        // 显示窗口
        window.set_visible(true);

        // 初始化 wgpu
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(instance_desc);

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("wy-ui device"),
            ..Default::default()
        }))
        .unwrap();

        // 配置 surface — 使用 adapter 首选格式（兼容 macOS Bgra8UnormSrgb）
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // 创建 Vello 渲染器
        let vello_renderer = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                ..Default::default()
            },
        )
        .unwrap();

        // 创建中间 Rgba8Unorm 纹理供 Vello 渲染（Vello render_to_texture 要求 Rgba8Unorm）
        let (vello_target, vello_target_view) =
            Self::create_vello_target(&device, size.width.max(1), size.height.max(1));

        // 创建 blit 管线：将 Rgba8Unorm 中间纹理 blit 到 surface 纹理
        let (blit_pipeline, blit_bind_group_layout) = Self::create_blit_pipeline(&device, format);

        self.window = Some(window);
        self.renderer = Some(vello_renderer);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);
        self.vello_target = Some(vello_target);
        self.vello_target_view = Some(vello_target_view);
        self.blit_pipeline = Some(blit_pipeline);
        self.blit_bind_group_layout = Some(blit_bind_group_layout);

        // 通知应用 resize
        self.app.on_resize(size.width as f32, size.height as f32);

        // 创建按需重绘回调，供应用的信号系统使用
        let window_ref = self.window.as_ref().unwrap().clone();
        let needs_redraw = self.needs_redraw.clone();
        let request_redraw: Rc<dyn Fn()> = Rc::new(move || {
            needs_redraw.set(true);
            window_ref.request_redraw();
        });

        // 创建自动重绘追踪器：框架层自动关联信号 → 重绘
        self.redraw_tracker = Some(crate::redraw_tracker::RedrawTracker::new(
            request_redraw.clone(),
        ));

        // 将帧源注册到 thread-local，供应用代码访问
        CURRENT_FRAME_SOURCE.with(|cell| {
            *cell.borrow_mut() = Some(self.frame_source.clone());
        });

        self.app.setup(request_redraw);

        // 请求首帧绘制
        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // 转发事件到 AccessKit adapter（必须在应用处理前）
        if let (Some(window), Some(adapter)) = (&self.window, &mut self.access_adapter) {
            adapter.process_event(window, &event);
        }

        // 让应用有机会处理事件
        if self.app.handle_event(&event) {
            return;
        }

        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    self.size = (new_size.width, new_size.height);
                    if let (Some(surface), Some(device), Some(config)) =
                        (&self.surface, &self.device, &mut self.config)
                    {
                        config.width = new_size.width;
                        config.height = new_size.height;
                        surface.configure(device, config);
                    }
                    // 重建 Vello 中间渲染目标
                    if let Some(device) = &self.device {
                        let (target, view) = Self::create_vello_target(
                            device,
                            new_size.width.max(1),
                            new_size.height.max(1),
                        );
                        self.vello_target = Some(target);
                        self.vello_target_view = Some(view);
                    }
                    self.app
                        .on_resize(new_size.width as f32, new_size.height as f32);
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x as f32, position.y as f32);
                // 分发鼠标移动到 WidgetTree
                if let Some(tree) = self.app.widget_tree() {
                    tree.dispatch_pointer_move(self.cursor_pos.0, self.cursor_pos.1);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                // 分发鼠标点击到 WidgetTree
                if let Some(tree) = self.app.widget_tree() {
                    match state {
                        winit::event::ElementState::Pressed => {
                            tree.dispatch_pointer_down(self.cursor_pos.0, self.cursor_pos.1);
                        }
                        winit::event::ElementState::Released => {
                            tree.dispatch_pointer_up(self.cursor_pos.0, self.cursor_pos.1);
                        }
                    }
                }
                // 请求重绘（点击可能改变状态）
                if let Some(window) = &self.window {
                    self.needs_redraw.set(true);
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                // 驱动帧动画：每次 redraw 前 tick 所有动画订阅
                self.frame_source.tick();
                self.render();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // 将 winit 键盘事件翻译为统一 KeyEvent 并转发给应用
                if let Some(key_event) =
                    crate::winit_translate::translate_key_event(event, self.modifiers)
                {
                    self.app.handle_key_event(&key_event);
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::AccessKit(ak_event) => match ak_event.window_event {
                accesskit_winit::WindowEvent::ActionRequested(request) => {
                    self.app.handle_accessibility_action(request);
                }
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    if let Some(adapter) = &mut self.access_adapter {
                        adapter.update_if_active(|| {
                            let root = accesskit::Node::new(accesskit::Role::Window);
                            accesskit::TreeUpdate {
                                nodes: vec![(accesskit::NodeId(0), root)],
                                tree: Some(accesskit::Tree::new(accesskit::NodeId(0))),
                                tree_id: accesskit::TreeId::ROOT,
                                focus: accesskit::NodeId(0),
                            }
                        });
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                    log::debug!("AccessKit accessibility deactivated");
                }
            },
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // 非 Poll 模式下不需要主动请求重绘
    }
}

impl<A: WyApp> AppState<A> {
    fn render(&mut self) {
        let (Some(window), Some(renderer), Some(surface), Some(device), Some(queue), Some(config)) = (
            &self.window,
            &mut self.renderer,
            &self.surface,
            &self.device,
            &self.queue,
            &self.config,
        ) else {
            return;
        };

        let (width, height) = self.size;
        if width == 0 || height == 0 {
            return;
        }

        // 1. 调用应用绘制，生成高层 Scene（通过 RedrawTracker 自动追踪信号依赖）
        let mut scene = Scene::new();
        if let Some(tracker) = &self.redraw_tracker {
            let tracker = tracker.clone();
            tracker.draw(|| {
                self.app.draw(&mut scene, width as f32, height as f32);
            });
            if let Some(tree) = self.app.widget_tree() {
                tree.draw_scene(&mut scene);
            }
        } else if let Some(tree) = self.app.widget_tree() {
            tree.draw_scene(&mut scene);
        } else {
            self.app.draw(&mut scene, width as f32, height as f32);
        }

        // 2. 翻译到 Vello Scene
        let mut vello_scene = vello::Scene::new();
        vello_executor::execute_scene(
            &scene,
            &mut vello_scene,
            &mut self.font_cx,
            &mut self.layout_cx,
            &mut self.text_cache,
        );

        // 3. 获取 surface texture
        let surface_texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(tex)
            | wgpu::CurrentSurfaceTexture::Suboptimal(tex) => tex,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(device, config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                return;
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 4. Vello 渲染到中间 Rgba8Unorm 纹理，再 blit 到 surface
        //
        // Vello render_to_texture 内部 bind group 硬编码 Rgba8Unorm，
        // macOS 等平台 surface 可能只支持 Bgra8UnormSrgb，
        // 两者不 copy-compatible，所以需要通过 render pass blit 转换格式。
        let render_params = vello::RenderParams {
            base_color: vello::peniko::Color::WHITE,
            width,
            height,
            antialiasing_method: vello::AaConfig::Area,
        };

        let vello_view = match &self.vello_target_view {
            Some(v) => v,
            None => return,
        };

        match renderer.render_to_texture(device, queue, &vello_scene, vello_view, &render_params) {
            Ok(_) => {
                // Blit: Rgba8Unorm 中间纹理 → surface 纹理
                if let (Some(pipeline), Some(blit_layout)) =
                    (&self.blit_pipeline, &self.blit_bind_group_layout)
                {
                    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("blit_bind_group"),
                        layout: blit_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(vello_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(
                                    &device.create_sampler(&wgpu::SamplerDescriptor::default()),
                                ),
                            },
                        ],
                    });

                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("blit_encoder"),
                        });
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("blit_pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &surface_view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            ..Default::default()
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &bind_group, &[]);
                        pass.draw(0..3, 0..1);
                    }
                    queue.submit(std::iter::once(encoder.finish()));
                }
                surface_texture.present();
            }
            Err(e) => {
                log::error!("Vello render error: {e}");
            }
        }

        // 5. 更新无障碍树
        let tree_update = self.app.accessibility_update().unwrap_or_else(|| {
            let root = accesskit::Node::new(accesskit::Role::Window);
            accesskit::TreeUpdate {
                nodes: vec![(accesskit::NodeId(0), root)],
                tree: Some(accesskit::Tree::new(accesskit::NodeId(0))),
                tree_id: accesskit::TreeId::ROOT,
                focus: accesskit::NodeId(0),
            }
        });
        if let Some(adapter) = &mut self.access_adapter {
            adapter.update_if_active(|| tree_update);
        }

        // 按需重绘：如果还有待处理的重绘请求，继续下一帧
        if self.needs_redraw.get() {
            self.needs_redraw.set(false);
            window.request_redraw();
        }
    }

    /// 创建 Vello 中间渲染目标（Rgba8Unorm）。
    ///
    /// Vello `render_to_texture` 内部 bind group 硬编码 Rgba8Unorm。
    /// macOS Apple Silicon 等平台 surface 可能只支持 Bgra8UnormSrgb，
    /// 所以需要始终使用中间 Rgba8Unorm 纹理渲染，再复制到 surface。
    fn create_vello_target(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("vello_intermediate_target"),
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
                | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// 创建 blit 渲染管线：将 Rgba8Unorm 中间纹理 blit 到 surface 纹理。
    ///
    /// 使用全屏三角形 + 纹理采样，兼容任意 surface 格式。
    fn create_blit_pipeline(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout) {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit_shader"),
            source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        (pipeline, bind_group_layout)
    }
}
