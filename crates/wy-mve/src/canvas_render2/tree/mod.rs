//! canvasRender2 的**树引擎**（逐文件对标 TS `canvasRender2/tree/`）。
//!
//! TS 把引擎按职责拆成
//! `protocol / messages / assemble / dispatch / pointer / keyboard / interaction /
//! globalMouse / focusWalk / geometry / layoutAudit / engineGlobal`。
//! Rust 侧按同一职责分文件落地，逐文件对齐。
//!
//! 注意边界：`Node2` **本体**仍在 [`mod@crate::canvas_render2::node2`]——Rust 里它
//! 必须是一个具体类型，`tree/` 放的是"以节点为参数、只发消息"的算法与工厂，
//! 与 TS 的分工一致。
//!
//! 已落地：`dispatch` / `engine_global` / `focus_walk` / `geometry` /
//! `global_mouse` / `keyboard` / `layout_audit` / `layout_direction` / `pointer`。
//! 尚未单独立文件的：`protocol`（在 [`crate::canvas_render2::protocol`]）、
//! `messages`（`Node2Props` 的 `Default` 兜底）、`assemble`（`Node2::build`）、
//! `interaction`（在 [`crate::canvas_render2::selection`]）。

pub mod dispatch;
pub mod engine_global;
pub mod focus_walk;
pub mod geometry;
pub mod global_mouse;
pub mod keyboard;
pub mod layout_audit;
pub mod layout_direction;
pub mod pointer;
