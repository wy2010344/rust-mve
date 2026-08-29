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
//!     fn draw(&mut self, scene: &mut Scene, width: f32, height: f32) {
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

use wy_render::{vello_executor, Scene};

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
    /// 绘制一帧。
    ///
    /// 在此方法中使用 `scene` 记录绘制命令（矩形、文本等）。
    /// `width`/`height` 是窗口客户区的像素尺寸。
    fn draw(&mut self, scene: &mut Scene, width: f32, height: f32);

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
        needs_redraw: Rc::new(Cell::new(true)),
        access_adapter: None,
        proxy,
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
    needs_redraw: Rc<Cell<bool>>,
    access_adapter: Option<accesskit_winit::Adapter>,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
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

        // 配置 surface
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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
                use_cpu: false,
                ..Default::default()
            },
        )
        .unwrap();

        self.window = Some(window);
        self.renderer = Some(vello_renderer);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);

        // 通知应用 resize
        self.app.on_resize(size.width as f32, size.height as f32);

        // 创建按需重绘回调，供应用的信号系统使用
        let window_ref = self.window.as_ref().unwrap().clone();
        let needs_redraw = self.needs_redraw.clone();
        let request_redraw: Rc<dyn Fn()> = Rc::new(move || {
            needs_redraw.set(true);
            window_ref.request_redraw();
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
                    self.app
                        .on_resize(new_size.width as f32, new_size.height as f32);
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x as f32, position.y as f32);
            }
            WindowEvent::RedrawRequested => {
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
                    // 平台请求初始无障碍树
                    if let Some(adapter) = &mut self.access_adapter {
                        adapter.update_if_active(|| {
                            // 返回空树，应用可通过 AccessibilityBridge 提供完整树
                            accesskit::TreeUpdate {
                                nodes: vec![],
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

        // 1. 调用应用绘制，生成高层 Scene
        let mut scene = Scene::new();
        self.app.draw(&mut scene, width as f32, height as f32);

        // 2. 翻译到 Vello Scene
        let mut vello_scene = vello::Scene::new();
        vello_executor::execute_scene(
            &scene,
            &mut vello_scene,
            &mut self.font_cx,
            &mut self.layout_cx,
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

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 4. Vello 渲染到 surface texture
        let render_params = vello::RenderParams {
            base_color: vello::peniko::Color::WHITE,
            width,
            height,
            antialiasing_method: vello::AaConfig::Area,
        };

        match renderer.render_to_texture(device, queue, &vello_scene, &texture_view, &render_params)
        {
            Ok(_) => {
                surface_texture.present();
            }
            Err(e) => {
                log::error!("Vello render error: {e}");
            }
        }

        // 5. 更新无障碍树（如果有变化）
        if let Some(tree_update) = self.app.accessibility_update() {
            if let Some(adapter) = &mut self.access_adapter {
                adapter.update_if_active(|| tree_update);
            }
        }

        // 按需重绘：如果还有待处理的重绘请求，继续下一帧
        if self.needs_redraw.get() {
            self.needs_redraw.set(false);
            window.request_redraw();
        }
    }
}
