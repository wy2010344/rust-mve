//! Flex 一维布局（主轴排列）。
//!
//! 复刻 Kotlin `com.wy.layout.FlexLayout`：把一组子节点沿主轴按
//! `grow` / `gap` / `reverse` / `direction_justify` 排布，返回每个子节点的
//! 一维位置与尺寸。支持"由子节点撑尺寸"（grow 主模式）与"按剩余空间 justify"。
//!
//! 本实现与 Kotlin 语义保持一致，但在构造时**立即求值**并缓存结果
//! （Kotlin 用 signal memo 惰性缓存，这里布局计算足够轻量，故构造即算）。

use crate::layout::{Layout, LayoutError, LayoutInsideObject};

/// 主轴方向的排列（justify）策略。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DirectionJustify {
    /// 靠起点。
    Start,
    /// 靠终点。
    End,
    /// 居中。
    Center,
    /// 两端对齐，首尾贴边。
    Between,
    /// 两侧留余量（余量/子节点数，首尾各一半）。
    Around,
    /// 均匀分布（余量/（子节点数+1））。
    Evenly,
    /// 由子节点撑起尺寸（无显式 justify）。
    Grow,
}

/// 仅一个子节点时 `Between` 的对齐策略。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DirectionFixBetweenWhenOne {
    /// 靠起点。
    Start,
    /// 居中。
    Center,
    /// 靠终点。
    End,
}

/// 子节点转换：提供每个子节点用于布局的标量信息。
///
/// 对应 Kotlin `FlexChildConvert<T>`。
pub trait FlexChildConvert<T> {
    /// 子节点在 `children` 中的索引。
    fn index(&self, child: &T) -> usize;
    /// 子节点的 grow 权重（`> 0` 表示由剩余空间撑开）。
    fn grow(&self, child: &T) -> f32;
    /// 子节点在主轴方向的外部尺寸。
    fn outer_size(&self, child: &T) -> f32;
    /// 子节点是否被忽略（不参与布局）。
    fn ignore(&self, child: &T) -> bool;
}

/// Flex 容器配置：兼作子节点转换（实现 [`FlexChildConvert`]）。
///
/// 对应 Kotlin `FlexObject<T>`。各配置项提供默认值，可按需覆写。
pub trait FlexObject<T>: FlexChildConvert<T> {
    /// 子节点之间的间距。
    fn gap(&self) -> f32 {
        0.0
    }

    /// 主轴排列策略，默认由子节点撑起（`Grow`）。
    fn direction_justify(&self) -> DirectionJustify {
        DirectionJustify::Grow
    }

    /// 是否反向排列子节点。
    fn reverse(&self) -> bool {
        false
    }

    /// `Between` 且仅一个子节点时的对齐。
    fn direction_fix_between_when_one(&self) -> DirectionFixBetweenWhenOne {
        DirectionFixBetweenWhenOne::Center
    }

    /// 由容器对象 + 子容器计算出 [`FlexLayout`]。
    fn to_layout(&self, inside: &LayoutInsideObject<T>) -> FlexLayout
    where
        Self: Sized,
    {
        FlexLayout::new(self, inside)
    }
}

/// Flex 布局的计算结果。
#[derive(Clone, Debug, PartialEq)]
pub struct FlexResult {
    /// 每个子节点的尺寸（按 `children` 索引；未参与布局的为 `None`）。
    pub child_lengths: Vec<Option<f32>>,
    /// 每个子节点的位置（按 `children` 索引；未参与布局的为 `None`）。
    pub positions: Vec<Option<f32>>,
    /// 子节点排列后占用的总长度。
    pub length: f32,
}

/// Flex 一维布局，立即求值并持有结果。
///
/// 对应 Kotlin `FlexLayout<T>`。计算结果是标量（与子节点类型 `T` 无关），
/// 故本类型不携带 `T` 泛型；`[`FlexLayout::new`]` 在构造时临时借用 `T`。
pub struct FlexLayout {
    result: FlexResult,
    has_grow_children: bool,
    justify: DirectionJustify,
    inner_size: f32,
}

impl FlexLayout {
    /// 根据容器配置与子容器立即计算布局。
    pub fn new<T>(arg: &dyn FlexObject<T>, inside: &LayoutInsideObject<T>) -> Self {
        let gap = arg.gap();
        let reverse = arg.reverse();
        let justify = arg.direction_justify();

        // 一次遍历：过滤 ignore 的子节点 + 是否存在 grow 子节点。
        let flex_children: Vec<&T> = inside.children.iter().filter(|c| !arg.ignore(c)).collect();
        let flex_count = flex_children.len();
        let has_grow_children = flex_children.iter().any(|c| arg.grow(c) > 0.0);

        let n = inside.children.len();
        let mut child_lengths = vec![None; n];
        let mut positions = vec![None; n];
        let mut length = 0.0f32;

        // 迭代顺序（reverse 反向）。索引指向 flex_children。
        let order: Vec<usize> = if reverse {
            (0..flex_count).rev().collect()
        } else {
            (0..flex_count).collect()
        };

        if has_grow_children {
            // 有 grow 子节点：非 grow 部分占自然尺寸，剩余空间按权重分给 grow。
            let inside_size = inside.inner_size;
            let mut grow_all = 0.0f32;
            let mut total_length = 0.0f32;
            let mut grow_of: Vec<f32> = vec![0.0; n];
            for c in &flex_children {
                let idx = arg.index(c);
                let g = arg.grow(c);
                if g > 0.0 {
                    grow_all += g;
                    grow_of[idx] = g;
                } else {
                    total_length += arg.outer_size(c);
                }
            }
            if grow_all > 0.0 {
                let remaining = inside_size - (gap * flex_count as f32 - gap) - total_length;
                for i in order {
                    let c = flex_children[i];
                    let idx = arg.index(c);
                    let g = grow_of[idx];
                    let child_length = if g > 0.0 {
                        if remaining > 0.0 {
                            remaining * g / grow_all
                        } else {
                            0.0
                        }
                    } else {
                        arg.outer_size(c)
                    };
                    child_lengths[idx] = Some(child_length);
                    positions[idx] = Some(length);
                    length += child_length + gap;
                }
            }
        } else if justify == DirectionJustify::Grow {
            // 由子节点撑起：全部自然尺寸，去掉末尾多余的 gap。
            for i in order {
                let c = flex_children[i];
                let idx = arg.index(c);
                let child_length = arg.outer_size(c);
                child_lengths[idx] = Some(child_length);
                positions[idx] = Some(length);
                length += child_length + gap;
            }
            if length > 0.0 {
                length -= gap;
            }
        } else {
            // 显式 justify：依据剩余空间决定起始偏移与间距。
            let inside_size = inside.inner_size;
            let mut t_gap = gap;
            let mut total_length = 0.0f32;
            for c in &flex_children {
                total_length += arg.outer_size(c);
            }
            let all_remaining = inside_size - total_length;
            let remaining = all_remaining - (gap * flex_count as f32 - gap);

            match justify {
                DirectionJustify::Center => length = remaining / 2.0,
                DirectionJustify::End => length = remaining,
                DirectionJustify::Around => {
                    t_gap = all_remaining / flex_count as f32;
                    length = t_gap / 2.0;
                }
                DirectionJustify::Between => {
                    if flex_count > 1 {
                        t_gap = all_remaining / (flex_count as f32 - 1.0);
                    } else if flex_count == 1 {
                        match arg.direction_fix_between_when_one() {
                            DirectionFixBetweenWhenOne::Center => length = all_remaining / 2.0,
                            DirectionFixBetweenWhenOne::End => length = all_remaining,
                            DirectionFixBetweenWhenOne::Start => {}
                        }
                    }
                }
                DirectionJustify::Evenly => {
                    t_gap = all_remaining / (flex_count as f32 + 1.0);
                    length = t_gap;
                }
                DirectionJustify::Start | DirectionJustify::Grow => {}
            }

            for i in order {
                let c = flex_children[i];
                let idx = arg.index(c);
                let child_length = arg.outer_size(c);
                child_lengths[idx] = Some(child_length);
                positions[idx] = Some(length);
                length += child_length + t_gap;
            }
        }

        Self {
            result: FlexResult {
                child_lengths,
                positions,
                length,
            },
            has_grow_children,
            justify,
            inner_size: inside.inner_size,
        }
    }

    /// 返回计算得到的完整结果。
    pub fn result(&self) -> &FlexResult {
        &self.result
    }
}

impl Layout for FlexLayout {
    fn size_from_children(&self) -> Result<f32, LayoutError> {
        if self.justify == DirectionJustify::Grow && !self.has_grow_children {
            Ok(self.result.length)
        } else {
            Ok(self.inner_size)
        }
    }

    fn child_size(&self, index: usize) -> Result<f32, LayoutError> {
        self.result
            .child_lengths
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| LayoutError::ignored(index))
    }

    fn child_position(&self, index: usize) -> Result<f32, LayoutError> {
        self.result
            .positions
            .get(index)
            .copied()
            .flatten()
            .ok_or_else(|| LayoutError::ignored(index))
    }

    fn allow_size_from_children(&self) -> bool {
        self.justify == DirectionJustify::Grow && !self.has_grow_children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Layout, LayoutInsideObject};

    /// 测试子节点：携带索引、尺寸与 grow 权重。
    struct Child {
        idx: usize,
        size: f32,
        grow: f32,
    }

    /// 通用 Flex 测试容器。
    struct Row {
        gap: f32,
        justify: DirectionJustify,
        reverse: bool,
    }

    impl FlexChildConvert<Child> for Row {
        fn index(&self, c: &Child) -> usize {
            c.idx
        }
        fn grow(&self, c: &Child) -> f32 {
            c.grow
        }
        fn outer_size(&self, c: &Child) -> f32 {
            c.size
        }
        fn ignore(&self, _c: &Child) -> bool {
            false
        }
    }

    impl FlexObject<Child> for Row {
        fn gap(&self) -> f32 {
            self.gap
        }
        fn direction_justify(&self) -> DirectionJustify {
            self.justify
        }
        fn reverse(&self) -> bool {
            self.reverse
        }
    }

    fn children(sizes: &[f32]) -> Vec<Child> {
        sizes
            .iter()
            .enumerate()
            .map(|(idx, &size)| Child {
                idx,
                size,
                grow: 0.0,
            })
            .collect()
    }

    fn row(gap: f32, justify: DirectionJustify, reverse: bool) -> Row {
        Row {
            gap,
            justify,
            reverse,
        }
    }

    #[test]
    fn grow_mode_uses_natural_size_and_allow_size() {
        let arg = row(0.0, DirectionJustify::Grow, false);
        let children = children(&[10.0, 20.0, 30.0]);
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        assert!(l.allow_size_from_children());
        assert_eq!(l.size_from_children().unwrap(), 60.0);
        assert_eq!(l.child_position(0).unwrap(), 0.0);
        assert_eq!(l.child_position(1).unwrap(), 10.0);
        assert_eq!(l.child_position(2).unwrap(), 30.0);
    }

    #[test]
    fn grow_mode_with_gap_subtracts_trailing_gap() {
        let arg = row(5.0, DirectionJustify::Grow, false);
        let children = children(&[10.0, 10.0]);
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        assert_eq!(l.size_from_children().unwrap(), 25.0);
        assert_eq!(l.child_position(1).unwrap(), 15.0);
    }

    #[test]
    fn with_grow_child_distributes_remaining() {
        let arg = row(0.0, DirectionJustify::Grow, false);
        let children = [
            Child {
                idx: 0,
                size: 10.0,
                grow: 0.0,
            },
            Child {
                idx: 1,
                size: 20.0,
                grow: 1.0,
            },
            Child {
                idx: 2,
                size: 30.0,
                grow: 3.0,
            },
        ];
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        // grow_all=4；非 grow 子节点合计 total_length=10
        // remaining = 100 - 0 - 10 = 90
        // 索引1 grow=1 => 90*1/4=22.5；索引2 grow=3 => 90*3/4=67.5
        assert_eq!(l.child_size(1).unwrap(), 22.5);
        assert_eq!(l.child_size(2).unwrap(), 67.5);
        assert_eq!(l.child_size(0).unwrap(), 10.0);
        // 位置：索引0@0、索引1@10、索引2@10+22.5=32.5
        assert_eq!(l.child_position(0).unwrap(), 0.0);
        assert_eq!(l.child_position(1).unwrap(), 10.0);
        assert_eq!(l.child_position(2).unwrap(), 32.5);
        // 有 grow 子节点则不允许由子节点撑尺寸
        assert!(!l.allow_size_from_children());
    }

    #[test]
    fn justify_center_positions_children() {
        let arg = row(0.0, DirectionJustify::Center, false);
        let children = children(&[10.0, 10.0]);
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        // all_remaining = 80, remaining = 80, 起始 length = 40
        assert_eq!(l.child_position(0).unwrap(), 40.0);
        assert_eq!(l.child_position(1).unwrap(), 50.0);
        // center 非 Grow，size_from_children 返回可用空间
        assert!(!l.allow_size_from_children());
        assert_eq!(l.size_from_children().unwrap(), 100.0);
    }

    #[test]
    fn justify_end_aligns_to_right() {
        let arg = row(0.0, DirectionJustify::End, false);
        let children = children(&[10.0, 10.0]);
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        assert_eq!(l.child_position(0).unwrap(), 80.0);
        assert_eq!(l.child_position(1).unwrap(), 90.0);
    }

    #[test]
    fn justify_between_and_evenly() {
        // between：两端贴边，余量 80 平均到中间 1 个 gap
        let arg = row(0.0, DirectionJustify::Between, false);
        let children = children(&[10.0, 10.0]);
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        assert_eq!(l.child_position(0).unwrap(), 0.0);
        assert_eq!(l.child_position(1).unwrap(), 90.0);

        // evenly：余量 80 分成 3 段，每段 ~26.67，起始等于一段
        let arg = row(0.0, DirectionJustify::Evenly, false);
        let l = arg.to_layout(&inside);
        let p0 = l.child_position(0).unwrap();
        let p1 = l.child_position(1).unwrap();
        assert!((p0 - 80.0 / 3.0).abs() < 1e-3);
        assert!((p1 - (80.0 / 3.0 + 10.0 + 80.0 / 3.0)).abs() < 1e-3);
    }

    #[test]
    fn reverse_orders_children_backwards() {
        let arg = row(0.0, DirectionJustify::Grow, true);
        let children = children(&[10.0, 20.0, 30.0]);
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = arg.to_layout(&inside);
        // 反向先放索引2(30)、索引1(20)、索引0(10)
        assert_eq!(l.child_position(2).unwrap(), 0.0);
        assert_eq!(l.child_position(1).unwrap(), 30.0);
        assert_eq!(l.child_position(0).unwrap(), 50.0);
    }

    #[test]
    fn ignored_child_position_is_error() {
        struct IgnoreRow;
        impl FlexChildConvert<Child> for IgnoreRow {
            fn index(&self, c: &Child) -> usize {
                c.idx
            }
            fn grow(&self, _c: &Child) -> f32 {
                0.0
            }
            fn outer_size(&self, c: &Child) -> f32 {
                c.size
            }
            fn ignore(&self, c: &Child) -> bool {
                c.idx == 0
            }
        }
        impl FlexObject<Child> for IgnoreRow {
            fn direction_justify(&self) -> DirectionJustify {
                DirectionJustify::Grow
            }
        }
        let children = [
            Child {
                idx: 0,
                size: 10.0,
                grow: 0.0,
            },
            Child {
                idx: 1,
                size: 20.0,
                grow: 0.0,
            },
        ];
        let inside = LayoutInsideObject::new(&children, 100.0);
        let l = IgnoreRow.to_layout(&inside);
        assert!(l.child_position(0).is_err());
        assert!(l.child_size(0).is_err());
        // 未被忽略的子节点正常
        assert_eq!(l.child_position(1).unwrap(), 0.0);
    }
}
