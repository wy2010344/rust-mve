//! 布局抽象：`Layout` trait、`LayoutInsideObject` 容器、`LayoutError`。
//!
//! 复刻 Kotlin `wy-helper` 的 `layout` 模块：布局是**一维**的，只回答
//! "子节点在主轴/交叉轴上的位置（position）与尺寸（size）"两个标量问题，
//! 不做二维盒模型。二维组合由上层（wy-render）按需叠加多个一维布局。

use std::error::Error;
use std::fmt;

/// 布局错误。
///
/// 满足 `std::error::Error + Display + Debug`，由 [`Layout`] 各查询方法返回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError {
    message: String,
}

impl LayoutError {
    /// 用自定义消息构造错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 子节点被 `ignore` 标记，无法查询其位置/尺寸。
    pub fn ignored(index: usize) -> Self {
        Self::new(format!(
            "{index} is ignored, its position/size is not available in layout"
        ))
    }

    /// 子节点未提供自身尺寸（StackLayout 中非 stretch 且无自定义 Align）。
    pub fn no_own_size() -> Self {
        Self::new("子节点应该有它自己的尺寸")
    }

    /// 没有默认值（absoluteLayout 的 sizeFromChildren）。
    pub fn no_default() -> Self {
        Self::new("没有默认值")
    }

    /// 没有子节点的尺寸（absoluteLayout 的 childSize）。
    pub fn no_child_size() -> Self {
        Self::new("没有子节点的尺寸")
    }

    /// 返回错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for LayoutError {}

/// 布局：一维的"子节点定位"接口。
///
/// 对应 Kotlin `com.wy.layout.Layout`：
/// - [`Self::child_size`] — 子节点在布局方向上的尺寸
/// - [`Self::child_position`] — 子节点在布局方向上的位置
/// - [`Self::size_from_children`] — 由子节点推算出的尺寸
/// - [`Self::allow_size_from_children`] — 是否允许"由子节点撑尺寸"（防止死循环）
pub trait Layout {
    /// 由子节点推算的尺寸（布局方向的整体尺寸）。
    fn size_from_children(&self) -> Result<f32, LayoutError>;

    /// 子节点在布局方向上的尺寸。
    fn child_size(&self, index: usize) -> Result<f32, LayoutError>;

    /// 子节点在布局方向上的位置（起点偏移）。
    fn child_position(&self, index: usize) -> Result<f32, LayoutError>;

    /// 是否允许由子节点撑起尺寸。
    fn allow_size_from_children(&self) -> bool;
}

/// 布局容器对象：子节点列表 + 可用主轴空间。
///
/// 对应 Kotlin `LayoutInsideObject<T>`。`children` 为子节点切片，
/// `inner_size` 为布局方向上可用的空间。
pub struct LayoutInsideObject<'a, T> {
    /// 参与布局的子节点。
    pub children: &'a [T],
    /// 可用主轴空间。
    pub inner_size: f32,
}

impl<'a, T> LayoutInsideObject<'a, T> {
    /// 构造容器对象。
    pub fn new(children: &'a [T], inner_size: f32) -> Self {
        Self {
            children,
            inner_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_error_implements_error_traits() {
        let e = LayoutError::ignored(3);
        assert_eq!(
            e.to_string(),
            "3 is ignored, its position/size is not available in layout"
        );
        let _: &dyn Error = &e;
    }

    #[test]
    fn container_exposes_children_and_inner_size() {
        let children = [1u8, 2, 3];
        let o = LayoutInsideObject::new(&children, 100.0);
        assert_eq!(o.children, &[1, 2, 3]);
        assert_eq!(o.inner_size, 100.0);
    }
}
