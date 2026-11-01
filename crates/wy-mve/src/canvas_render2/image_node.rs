//! 图片节点（逐句对标 TS `canvasRender2/ImageNode.ts`）。
//!
//! 尺寸恒由图片内容推导：可用 `image_size` 约束单轴，另一轴按原图比例计算；
//! 绘制落在 padding 内，`radius > 0` 时先做圆角裁剪再画图片。
//! 节点仍走 [`Node2::build`]（= TS `node2.create`），不接旧图片组件或旧 `Node`。
//!
//! # 继承 `node2`（`#[rec(extends = node2)]`）
//!
//! 节点字段（`width` / `padding` / `layout` / `children` …）经宏内联转发进自动注入的
//! `rest: Node2Props`；组件体就是 `node2! { ..rest, size: .., draw: .. }`——尺寸与
//! 绘制是**覆盖项**，其余原样继承。调用处写 `image_node! { image: .., width: .. }`，
//! 与 `node2!` 同构。节点句柄是**普通参数** `node_ref`（对齐 TS `arg.nodeRef`）。

use wy_render::{PlatformImage, Rect, Scene};
use wy_signal::{create_memo, GetValue};

use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::node2::{Axis, LayoutSize, LayoutSizeDirection, Node2};
use crate::{rec, Node2Props};

/// canvasRender2 图片节点（对标 TS `imageNode`）。
#[rec(extends = node2)]
#[must_use]
pub fn image_node(
    /// 图片源，支持读取式更新。
    #[getter]
    image: PlatformImage,
    /// TS 同名参数；当前只解构、未参与尺寸计算，Rust 保持相同行为。
    #[getter]
    original_width: f32,
    /// TS 同名参数；当前只解构、未参与尺寸计算，Rust 保持相同行为。
    #[getter]
    original_height: f32,
    /// 圆角半径，默认 0。
    #[getter]
    #[props(default = 0.0f32)]
    radius: f32,
    /// 单方向尺寸约束；另一方向按原图比例推算。
    #[getter]
    image_size: Option<LayoutSizeDirection>,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）：**普通参数**，不传则内部新建、
    /// 调用方拿不到该节点；需要句柄时显式传一个 `Ref2<Node2>`。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    // 与 TS 一致：接受这两个参数但不参与尺寸计算（避免 unused 告警）。
    let _ = (original_width, original_height);

    // TS `const contentSize = memo<readonly [number, number]>(...)`。memo 里要读
    // 本节点的 padding，而节点此刻还没造出来——所以读句柄槽，等它被
    // `node2!` 写完（`size` / `draw` 都是布局期、绘制期才求值）。
    let size_ref = node_ref.clone();
    let content_size = {
        let image = image.clone();
        let image_size = image_size.clone();
        create_memo(move || {
            let Some(img) = image.as_ref().map(|get| get()) else {
                return (0.0, 0.0);
            };
            let n = size_ref.get();
            let pad_x = n.padding_inline_start() + n.padding_inline_end();
            let pad_y = n.padding_block_start() + n.padding_block_end();
            // 对标 TS/Kotlin：比例换算除数至少为 1，避免 Infinity/NaN。
            let ow = img.width.max(1.0);
            let oh = img.height.max(1.0);
            let Some(s) = image_size() else {
                return (img.width, img.height);
            };
            match s.direction {
                Axis::X => {
                    let w = if s.from_inside {
                        s.value
                    } else {
                        (s.value - pad_x).max(0.0)
                    };
                    (w, w * img.height / ow)
                }
                Axis::Y => {
                    let h = if s.from_inside {
                        s.value
                    } else {
                        (s.value - pad_y).max(0.0)
                    };
                    (h * img.width / oh, h)
                }
            }
        })
    };

    let size_memo = content_size.clone();
    let draw_image = image.clone();
    let draw_memo = content_size;

    node2! {
        ..rest,
        node_ref: node_ref.clone(),
        // 图片节点尺寸恒由图片内容推导；调用方传的 `size` 被这里覆盖。
        size: move |d: Axis| {
            let (w, h) = size_memo.get();
            LayoutSize::inside(match d {
                Axis::X => w,
                Axis::Y => h,
            })
        },
        // TS 对象展开后用本组件的 draw 覆盖 rest.draw；这里同样不调用旧 draw。
        draw: move |scene: &mut Scene, n: &Node2| {
            let Some(img) = draw_image.as_ref().map(|get| get()) else {
                return;
            };
            let (w, h) = draw_memo.get();
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let ox = n.padding_inline_start();
            let oy = n.padding_block_start();
            let rect = Rect::new(ox, oy, w, h);
            let r = radius();
            if r > 0.0 {
                scene.push_round_clip(rect, r);
                scene.draw_image(img, rect);
                scene.pop_clip();
            } else {
                scene.draw_image(img, rect);
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 测试也用组件作者那套写法：`image_node! { .. }`（本文件定义的宏走文本作用域），
    // 不写 `ImageNodeArg::builder()` 链。
    use crate::canvas_render2::node2::layout_size_direction;

    /// 把组件的 `Fc` 真挂进一棵树：`node2!` 是延迟构造（`Fc` 等父上下文），
    /// 句柄槽要等挂载那一刻才写。这与 TS `node2(arg)(ctx)` 同序。
    fn mount(fc: Fc) -> Node2 {
        let root =
            crate::canvas_render2::node2::build_root_node2(Node2Props::default(), move |cx| {
                cx.child(fc);
            });
        root.raw_children().first().expect("组件应挂在根下").clone()
    }

    fn image(width: u32, height: u32) -> PlatformImage {
        PlatformImage::from_rgba8(
            width,
            height,
            vec![255; width as usize * height as usize * 4],
        )
        .unwrap()
    }

    #[test]
    fn natural_size_comes_from_image_content() {
        // 节点由工厂自己造（TS `node2(..)` 那层），于是测试改为显式传一个
        // `node_ref` 槽、再从槽里读回产物——与真实用法同一条路径。
        let r = Ref2::new();
        let _node = mount(image_node! {
            node_ref: r.clone(),
            image: image(40, 20),
        });

        let inspect = r.get();
        assert_eq!(inspect.width(), 40.0);
        assert_eq!(inspect.height(), 20.0);
    }

    #[test]
    fn outside_constraint_subtracts_padding_before_aspect_ratio() {
        let r = Ref2::new();
        let _node = mount(image_node! {
            node_ref: r.clone(),
            image: image(200, 100),
            image_size: Some(layout_size_direction(Axis::X, 120.0, false)),
            padding_inline_start: 10.0,
            padding_inline_end: 10.0,
            padding_block_start: 5.0,
            padding_block_end: 5.0,
        });

        // 外包宽 120 → 扣横向 padding 20 得内容宽 100；2:1 比例得内容高 50，
        // 再加纵向 padding 10 得外包高 60。
        let inspect = r.get();
        assert_eq!(inspect.width(), 120.0);
        assert_eq!(inspect.height(), 60.0);
    }

    #[test]
    fn rounded_draw_records_clip_image_pop() {
        let r = Ref2::new();
        let _node = mount(image_node! {
            node_ref: r.clone(),
            image: image(2, 1),
            radius: 4.0,
        });
        let node = r.get();
        let mut scene = Scene::new();
        node.render_node2(&mut scene);
        let items: Vec<_> = scene.iter().collect();
        assert!(matches!(
            items[0],
            wy_render::Primitive::RoundClipPush { .. }
        ));
        assert!(matches!(items[1], wy_render::Primitive::Image { .. }));
        assert!(matches!(items[2], wy_render::Primitive::ClipPop));
    }
}
