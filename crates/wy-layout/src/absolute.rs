//! Absolute 占位布局。
//!
//! 复刻 Kotlin `com.wy.layout.absoluteLayout`：绝对定位的占位 `Layout`，
//! 子节点一律靠起点（位置为 0），不提供尺寸/默认值。

use crate::layout::{Layout, LayoutError};

/// 绝对定位占位布局。
///
/// - `child_position` 恒为 0（子节点靠容器起点）
/// - `child_size` / `size_from_children` 报错（没有子节点尺寸/默认值）
/// - 不允许由子节点撑尺寸
#[derive(Copy, Clone, Debug, Default)]
pub struct AbsoluteLayout;

impl AbsoluteLayout {
    /// 绝对定位布局实例。
    pub const fn new() -> Self {
        Self
    }
}

impl Layout for AbsoluteLayout {
    fn size_from_children(&self) -> Result<f32, LayoutError> {
        Err(LayoutError::no_default())
    }

    fn child_size(&self, _index: usize) -> Result<f32, LayoutError> {
        Err(LayoutError::no_child_size())
    }

    fn child_position(&self, _index: usize) -> Result<f32, LayoutError> {
        Ok(0.0)
    }

    fn allow_size_from_children(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;

    #[test]
    fn absolute_position_is_zero() {
        let l = AbsoluteLayout::new();
        assert_eq!(l.child_position(0).unwrap(), 0.0);
        assert_eq!(l.child_position(99).unwrap(), 0.0);
    }

    #[test]
    fn absolute_has_no_size_and_no_default() {
        let l = AbsoluteLayout::new();
        assert!(l.child_size(0).is_err());
        assert!(l.size_from_children().is_err());
        assert!(!l.allow_size_from_children());
    }
}
