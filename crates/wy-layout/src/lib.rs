//! 布局引擎：Flex/Stack/Absolute 一维布局。
//!
//! 复刻 Kotlin `wy-helper` 的 `layout` 模块：布局是**一维**的——只回答
//! "子节点在布局方向上的位置与尺寸"两个标量问题，不做二维盒模型。
//! 二维组合由上层（wy-render）按需叠加多个一维布局完成。
//!
//! 核心类型：
//! - [`Layout`] — 一维布局接口（child_size / child_position / size_from_children）
//! - [`LayoutInsideObject`] — 布局容器对象（子节点 + 可用空间）
//! - [`FlexLayout`] / [`StackLayout`] / [`AbsoluteLayout`] — 三种布局
//! - [`FlexObject`] / [`StackObject`] — 容器配置（兼子节点转换）
//! - [`LayoutError`] — 布局错误

mod absolute;
mod flex;
mod layout;
mod stack;

pub use absolute::AbsoluteLayout;
pub use flex::{
    DirectionFixBetweenWhenOne, DirectionJustify, FlexChildConvert, FlexLayout, FlexObject,
    FlexResult,
};
pub use layout::{Layout, LayoutError, LayoutInsideObject};
pub use stack::{Align, AlignItem, StackChildConvert, StackLayout, StackObject};
