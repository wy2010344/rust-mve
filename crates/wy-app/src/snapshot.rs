//! 离屏出图：把一个应用渲染成 PNG，**不开窗口**。
//!
//! # 为什么需要它
//!
//! README 首屏的配图必须是**可复现**的——同样的代码跑两次要出同样的图。
//! 手写截图会过期（代码改了图还是旧的），这个模块让文档配图跟着代码走。
//!
//! # 用法
//!
//! 每个示例都支持 `--snapshot <路径>` 参数：
//!
//! ```text
//! cargo run -p wy-app --example finance -- --snapshot docs/shot-finance.png
//! cargo run -p wy-app --example shadcn -- --snapshot docs/shot-shadcn.png
//! ```
//!
//! 尺寸可用 `--snapshot-size 1440x900` 指定，默认 1440×900。

use wy_engine::runner::WyApp;
use wy_render::Scene;

/// 解析 `--snapshot` / `--snapshot-size` 参数。
///
/// 返回 `Some((路径, 宽, 高))` 表示需要出图并退出；`None` 表示正常启动窗口。
pub fn parse_snapshot_args() -> Option<(String, u32, u32)> {
    let mut args = std::env::args().skip(1);
    let mut path: Option<String> = None;
    let mut size: Option<(u32, u32)> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--snapshot" => path = args.next(),
            "--snapshot-size" => {
                if let Some(spec) = args.next() {
                    size = parse_size(&spec);
                }
            }
            _ => {}
        }
    }

    path.map(|p| {
        let (w, h) = size.unwrap_or((1440, 900));
        (p, w, h)
    })
}

/// 解析 `1440x900` 形式的尺寸串。
fn parse_size(spec: &str) -> Option<(u32, u32)> {
    let (w, h) = spec.split_once(['x', 'X'])?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// 渲染一帧并写成 PNG。
///
/// 返回 `Err` 说明离屏渲染不可用（最常见的是机器上没有 GPU 适配器）。
pub fn save_snapshot(app: &dyn WyApp, path: &str, width: u32, height: u32) -> Result<(), String> {
    let mut scene = Scene::new();
    app.draw(&mut scene, width as f32, height as f32);

    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败：{e}"))?;
        }
    }

    wy_render::offscreen::render_to_file(&scene, width, height, path)
        .map_err(|e| format!("离屏渲染失败：{e}（需要一个可用的 GPU 适配器）"))?;

    println!("已写出 {path}（{width}×{height}，{} 个图元）", scene.len());
    Ok(())
}
