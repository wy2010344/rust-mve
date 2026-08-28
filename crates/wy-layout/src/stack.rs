//! Stack 一维布局（交叉轴对齐）。
//!
//! 复刻 Kotlin `com.wy.layout.StackLayout`：子节点沿布局方向堆叠，同时
//! 在交叉轴上通过 `AlignItem`（或逐子节点的 [`Align`]）对齐。支持
//! `align_fix`（外部提供固定交叉轴尺寸）与"由非对齐子节点撑尺寸"。
//!
//! 与 Kotlin 一致，非 stretch 且无自定义 `Align` 的子节点**没有自己的交叉轴
//! 尺寸**（查询尺寸时会报错），但位置（start/center/end）仍可查询。
//! 本实现构造时立即求值并缓存交叉轴尺寸 `size_val`。

use crate::layout::{Layout, LayoutError, LayoutInsideObject};

/// 交叉轴对齐策略。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AlignItem {
    /// 居中。
    Center,
    /// 靠起点。
    Start,
    /// 靠终点。
    End,
    /// 拉伸填满交叉轴。
    Stretch,
}

/// 自定义逐子节点对齐：给定交叉轴可用尺寸，计算子节点的尺寸与位置。
///
/// 对应 Kotlin `com.wy.layout.Align`。调用方负责实现（可把子节点固有尺寸
/// 闭包捕获进来）。
pub trait Align {
    /// 子节点在交叉轴上的尺寸。
    fn size(&self, size: f32) -> f32;
    /// 子节点在交叉轴上的位置，`self_width` 为子节点自身宽度。
    fn position(&self, size: f32, self_width: f32) -> f32;
}

/// 子节点转换：提供逐子节点的对齐信息与交叉轴尺寸。
///
/// 对应 Kotlin `StackChildConvert<T>`。
pub trait StackChildConvert<T> {
    /// 子节点的自定义对齐（`None` 则回退到 `align_item`）。
    fn align(&self, child: &T) -> Option<&dyn Align>;
    /// 子节点在交叉轴方向的外部尺寸。
    fn outer_size(&self, child: &T) -> f32;
    /// 子节点是否被忽略（不参与布局）。
    fn ignore(&self, child: &T) -> bool;
}

/// Stack 容器配置：兼作子节点转换（实现 [`StackChildConvert`]）。
///
/// 对应 Kotlin `StackObject<T>`。
pub trait StackObject<T>: StackChildConvert<T> {
    /// 交叉轴对齐策略，默认居中。
    fn align_item(&self) -> AlignItem {
        AlignItem::Center
    }

    /// 是否由外部提供固定交叉轴尺寸。
    fn align_fix(&self) -> bool {
        false
    }

    /// 由容器对象 + 子容器计算出 [`StackLayout`]。
    fn to_layout(&self, inside: &LayoutInsideObject<T>) -> StackLayout
    where
        Self: Sized,
    {
        StackLayout::new(self, inside)
    }
}

/// Stack 一维布局，构造时立即求值并持有交叉轴尺寸与逐子节点结果。
///
/// 对应 Kotlin `StackLayout<T>`。计算结果为标量（与子节点类型 `T` 无关），
/// 故本类型不携带 `T` 泛型；`[`StackLayout::new`]` 在构造时临时借用 `T`。
/// 使用前请确认子节点索引在 `children` 范围内。
pub struct StackLayout {
    size_val: f32,
    align_fix: bool,
    // 交叉轴尺寸/位置（按 children 索引）；`None` 表示查询时需报错的子节点。
    child_sizes: Vec<Option<f32>>,
    child_positions: Vec<Option<f32>>,
}

impl StackLayout {
    /// 根据容器配置与子容器立即计算布局。
    pub fn new<T>(arg: &dyn StackObject<T>, inside: &LayoutInsideObject<T>) -> Self {
        let align_fix = arg.align_fix();
        let align_item = arg.align_item();

        // 交叉轴尺寸：alignFix 时用外部 inner_size；否则取"非对齐子节点的最大尺寸"。
        let size_val = if align_fix {
            inside.inner_size
        } else {
            let mut w = 0.0f32;
            for c in inside.children {
                if !arg.ignore(c) && arg.align(c).is_none() {
                    w = w.max(arg.outer_size(c));
                }
            }
            w
        };

        let n = inside.children.len();
        let mut child_sizes = vec![None; n];
        let mut child_positions = vec![None; n];

        for (i, c) in inside.children.iter().enumerate() {
            if arg.ignore(c) {
                continue; // 位置与尺寸均不可查询
            }
            if let Some(align) = arg.align(c) {
                let s = align.size(size_val);
                let p = align.position(size_val, arg.outer_size(c));
                child_sizes[i] = Some(s);
                child_positions[i] = Some(p);
                continue;
            }
            match align_item {
                AlignItem::Stretch => {
                    child_sizes[i] = Some(size_val);
                    child_positions[i] = Some(0.0);
                }
                AlignItem::Start => {
                    child_sizes[i] = None; // 无自身尺寸
                    child_positions[i] = Some(0.0);
                }
                AlignItem::Center => {
                    child_sizes[i] = None;
                    child_positions[i] = Some((size_val - arg.outer_size(c)) / 2.0);
                }
                AlignItem::End => {
                    child_sizes[i] = None;
                    child_positions[i] = Some(size_val - arg.outer_size(c));
                }
            }
        }

        Self {
            size_val,
            align_fix,
            child_sizes,
            child_positions,
        }
    }

    /// 返回交叉轴可用尺寸（`size()`）。
    pub fn size(&self) -> f32 {
        self.size_val
    }
}

impl Layout for StackLayout {
    fn size_from_children(&self) -> Result<f32, LayoutError> {
        Ok(self.size_val)
    }

    fn child_size(&self, index: usize) -> Result<f32, LayoutError> {
        match self.child_sizes.get(index).copied().flatten() {
            Some(s) => Ok(s),
            None => {
                // 区分：是被 ignore（位置也为 None）还是没有自身尺寸。
                if self.child_positions.get(index).copied().flatten().is_some() {
                    Err(LayoutError::no_own_size())
                } else {
                    Err(LayoutError::ignored(index))
                }
            }
        }
    }

    fn child_position(&self, index: usize) -> Result<f32, LayoutError> {
        self.child_positions
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| LayoutError::ignored(index))
    }

    fn allow_size_from_children(&self) -> bool {
        // 阻断，否则会造成"由子节点撑尺寸"的死循环。
        !self.align_fix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Layout, LayoutInsideObject};

    struct Child {
        idx: usize,
        size: f32,
    }
    struct AlignCenter;
    impl Align for AlignCenter {
        fn size(&self, _size: f32) -> f32 {
            0.0
        }
        fn position(&self, size: f32, self_width: f32) -> f32 {
            (size - self_width) / 2.0
        }
    }

    struct Stack {
        align_item: AlignItem,
        align_fix: bool,
        // 子节点自定义对齐（仅索引 0 有）
        custom: bool,
    }

    impl StackChildConvert<Child> for Stack {
        fn align(&self, c: &Child) -> Option<&dyn Align> {
            if self.custom && c.idx == 0 {
                Some(&AlignCenter)
            } else {
                None
            }
        }
        fn outer_size(&self, c: &Child) -> f32 {
            c.size
        }
        fn ignore(&self, _c: &Child) -> bool {
            false
        }
    }
    impl StackObject<Child> for Stack {
        fn align_item(&self) -> AlignItem {
            self.align_item
        }
        fn align_fix(&self) -> bool {
            self.align_fix
        }
    }

    fn children() -> Vec<Child> {
        vec![
            Child { idx: 0, size: 10.0 },
            Child { idx: 1, size: 30.0 },
            Child { idx: 2, size: 20.0 },
        ]
    }

    #[test]
    fn stretch_children_fill_cross_axis() {
        let arg = Stack {
            align_item: AlignItem::Stretch,
            align_fix: false,
            custom: false,
        };
        let children = children();
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        // 无 alignFix：size = 非对齐子节点最大尺寸 = 30
        assert_eq!(l.size(), 30.0);
        assert_eq!(l.child_size(1).unwrap(), 30.0);
        assert_eq!(l.child_position(1).unwrap(), 0.0);
        assert!(l.allow_size_from_children());
    }

    #[test]
    fn align_fix_uses_inner_size() {
        let arg = Stack {
            align_item: AlignItem::Stretch,
            align_fix: true,
            custom: false,
        };
        let children = children();
        let inside = LayoutInsideObject::new(&children, 200.0);
        let l = arg.to_layout(&inside);
        assert_eq!(l.size(), 200.0);
        assert!(!l.allow_size_from_children());
    }

    #[test]
    fn center_item_centers_children_and_has_no_own_size() {
        let arg = Stack {
            align_item: AlignItem::Center,
            align_fix: false,
            custom: false,
        };
        let children = children();
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        // size = max 非对齐尺寸 = 30
        assert_eq!(l.size(), 30.0);
        // 子节点1 自身宽度30 => 居中位置 0
        assert_eq!(l.child_position(1).unwrap(), 0.0);
        // 子节点0 自身宽度10 => 居中位置 (30-10)/2=10
        assert_eq!(l.child_position(0).unwrap(), 10.0);
        // 尺寸不可查询（无自身尺寸）
        assert!(l.child_size(0).is_err());
    }

    #[test]
    fn end_item_aligns_to_bottom() {
        let arg = Stack {
            align_item: AlignItem::End,
            align_fix: false,
            custom: false,
        };
        let children = children();
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        assert_eq!(l.size(), 30.0);
        // 子节点2 自身宽度20 => 位置 30-20=10
        assert_eq!(l.child_position(2).unwrap(), 10.0);
    }

    #[test]
    fn custom_align_wins_over_align_item() {
        struct AlignRight;
        impl Align for AlignRight {
            fn size(&self, _s: f32) -> f32 {
                5.0
            }
            fn position(&self, size: f32, _self_width: f32) -> f32 {
                size - 5.0
            }
        }
        // 用 custom=true，但提供一个作用于索引0的"靠右"实现
        struct RightStack;
        impl StackChildConvert<Child> for RightStack {
            fn align(&self, c: &Child) -> Option<&dyn Align> {
                if c.idx == 0 {
                    Some(&AlignRight)
                } else {
                    None
                }
            }
            fn outer_size(&self, c: &Child) -> f32 {
                c.size
            }
            fn ignore(&self, _c: &Child) -> bool {
                false
            }
        }
        impl StackObject<Child> for RightStack {
            fn align_item(&self) -> AlignItem {
                AlignItem::Start
            }
        }
        let children = children();
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = RightStack.to_layout(&inside);
        // size = 非自定义子节点(1,2)最大尺寸 = 30
        assert_eq!(l.size(), 30.0);
        // 自定义子节点0：size=5, position=30-5=25
        assert_eq!(l.child_size(0).unwrap(), 5.0);
        assert_eq!(l.child_position(0).unwrap(), 25.0);
    }
}
