//! 新特性演示（对标 TS `canvas-render2/todo-demos.ts`）。
//!
//! TS 的注释把这一屏当成「复刻过程中新增特性的可视化演示」，四块对应四个能力：
//!
//! | 演示 | TS 依赖 | 本文件 |
//! |---|---|---|
//! | 修饰键 / 按压态 HUD | `ctrlSignal` / `shiftSignal` / `altSignal` / `metaSignal` / `pressedSignal` | [`modifier_hud`]：读 [`modifiers`] / [`pressed_node`] |
//! | 按键旁听 | `registerKeyPress` / `notifyKeyPress` | [`ensure_key_listener`] → [`last_key`] |
//! | `noWrap` 单行不折行 | `noWrap` + `grow` | [`nowrap_demo`] |
//! | 嵌套滚动 | `createScroll(.., parentScroll)` | [`nested_scroll_demo`]：`Scroll2::with_parent` |
//!
//! # 为什么这里没有「信号」也照样会重绘
//!
//! TS 的四个修饰键是 `createSignal` 造的信号。Rust 侧它们住在
//! [`wy_mve::canvas_render2::tree::engine_global`] 的线程局部 `Signal` 里，
//! [`modifiers`] / [`pressed_node`] 只是把这些信号读出来——**读发生在 draw 闭包
//! 内**，所以仍是响应式读取：宿主上报新快照 → 信号变了 → 读到它的绘制重跑。
//! 组件层因此不需要额外的订阅代码（与 TS 里 `pill(.., () => ctrlSignal.get())`
//! 的写法一一对应，只是 `()` 换成 [`modifiers`]）。
//!
//! # 嵌套滚动的落点
//!
//! TS `createScroll(container, 'y', undefined, outerScroll)` 的第四个参数就是
//! 「外层句柄」。Rust 是链式的 [`Scroll2::with_parent`]：滚到底的剩余量由
//! [`Scroll2::scroll`] 自己转交，滚轮事件只需挂在各自的内容节点上
//! （`scroll_content` 已代劳），不必手工串联。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::canvas_render2::{
    create_scroll, get, grow, modifiers, placeholder_color, pressed_node, register_key_press,
    text_color, Axis, KeyEvent2, RichTextSpan, Scroll2,
};
// 宏都在 crate 根（`#[macro_export]`），子模块里同名的是普通函数。
use wy_mve::{
    flex, rect_node, rich_text_node, scroll_bar, scroll_content, AlignItem, DirectionJustify, Fc,
    Layout, NodeContext,
};
use wy_render::Color;
use wy_signal::{GetValue, SetValue, Signal};
use wy_text::TextStyle;

use crate::store::palette;

/// 激活态文字（TS `0xffffffff`）。
const WHITE: Color = Color::from_u32(0xFFFF_FFFF);
/// 未激活态文字（TS `0xff334155` 的 RGBA 重排）。
const SLATE_700: Color = Color::from_u32(0x3341_55FF);

/// `noWrap` 演示用的超长单行文本：宽 360 的内容盒里必然溢出。
const NO_WRAP_TEXT: &str = "这是一段很长很长很长很长很长很长很长很长很长很长很长很长很长很长的单行文本（noWrap 会裁剪溢出部分，不会折行也不会补省略号）";

/// 嵌套滚动的说明文案。
const NESTED_TIP: &str = "嵌套滚动：内层滚到底后剩余量接续外层（parentScroll）";

/// 按键旁听器类型（单独起别名，免得 clippy 抱怨 `thread_local!` 里的类型太长）。
type KeyListener = Rc<dyn Fn(&KeyEvent2)>;

thread_local! {
    /// 最近一次「旁听」到的按键（TS 模块级 `lastKeySignal`）。
    ///
    /// 不能用 `const { .. }` 初始化：`Signal::new` 不是 const fn（`store.rs` 里
    /// 那几个信号同样是运行时初始化）。
    static LAST_KEY: RefCell<Signal<String>> = RefCell::new(Signal::new(String::new()));
    /// 最近一次构建出来的（外层, 内层）滚动句柄。
    ///
    /// 与 `list::LAST_SCROLL` 同因：壳层没有别的途径拿到组件内部的 `Scroll2`
    /// （TS 同样不暴露），而"内层滚到底、剩余量接续外层"这件事必须能被快照
    /// 固定下来。运行期没有代码读它。
    static NESTED_OUTER: RefCell<Option<Scroll2>> = const { RefCell::new(None) };
    /// 内层句柄（同上，只为出图核验）。
    static NESTED_INNER: RefCell<Option<Scroll2>> = const { RefCell::new(None) };
    /// 旁听器本体：只注册一次，且必须留着这个 `Rc`。
    ///
    /// [`register_key_press`] 按 `Rc` 的引用同一性去重——每次新建一个闭包都会算
    /// 一次新注册（TS 侧同理：它按函数引用去重）。所以句柄存在这里，重复调用
    /// [`ensure_key_listener`] 时先看槽。
    static KEY_LISTENER: RefCell<Option<KeyListener>> = const { RefCell::new(None) };
}

/// 取最近一次构建的嵌套滚动句柄（外层, 内层）；只为出图核验。
#[must_use]
pub fn last_nested() -> Option<(Scroll2, Scroll2)> {
    let outer = NESTED_OUTER.with(|s| s.borrow().clone())?;
    let inner = NESTED_INNER.with(|s| s.borrow().clone())?;
    Some((outer, inner))
}

/// 最近按键的可读文本（未按过时是空串，显示层补 `(无)`）。
#[must_use]
fn last_key() -> String {
    LAST_KEY.with(|s| s.borrow().get())
}

/// 注册全局按键旁听（TS 在模块加载时 `registerKeyPress` 一次）。
///
/// 注意把它放在「构建页面时」而不是某个 `static` 初始化里：`Signal` 是线程局部的，
/// 首次构建一定发生在 UI 线程上。
fn ensure_key_listener() {
    KEY_LISTENER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return;
        }
        let cb: KeyListener = Rc::new(|e: &KeyEvent2| {
            // TS `e.key ? e.key : e.code`
            let text = if e.key.is_empty() {
                e.code.clone()
            } else {
                e.key.clone()
            };
            LAST_KEY.with(|s| s.borrow().set(text));
        });
        // 先落槽再注册（注册表也持一份 clone，两处都指向同一个 `Rc`）。
        *slot = Some(Rc::clone(&cb));
        // 注销句柄不保存：演示页与进程同寿命，TS 同样只在模块级注册一次。
        drop(register_key_press(cb));
    });
}

/// 一段 12 号字的标签片段（TS `label(text, color)`）。
fn label(text: impl Into<String>, color: Color) -> RichTextSpan {
    RichTextSpan {
        text: text.into(),
        style: TextStyle::normal()
            .with_font_size(12.0)
            .with_color(color.to_u32()),
        url: None,
    }
}

/// 小药丸：`label` 文本 + 激活态底色（TS `pill`）。
///
/// `active` 在**绘制期**才被调用，因此它读到的信号变化会让这张卡片重绘——
/// 与 TS 里 `background: () => active() ? .. : ..` 同一个时机。
fn pill(text: &'static str, active: Rc<dyn Fn() -> bool>) -> Fc {
    let bg = Rc::clone(&active);
    let fg = active;
    rect_node! {
        radius: 6.0,
        border_width: 1.0,
        padding_inline_start: 8.0,
        padding_inline_end: 8.0,
        padding_block_start: 4.0,
        padding_block_end: 4.0,
        background: Some(if bg() { palette().accent } else { palette().card_done }),
        border_color: Some(palette().border),
        layout: flex! {},
        children: vec![rich_text_node! {
            auto_width: true,
            spans: vec![label(text, if fg() { WHITE } else { SLATE_700 })],
        }],
    }
}

/// 修饰键 + 按压态 HUD（TS `modifierHud`）。
#[must_use]
pub fn modifier_hud() -> Fc {
    ensure_key_listener();
    rect_node! {
        width: 380.0,
        radius: 8.0,
        border_width: 1.0,
        padding_inline_start: 10.0,
        padding_inline_end: 10.0,
        padding_block_start: 6.0,
        padding_block_end: 6.0,
        background: Some(palette().card),
        border_color: Some(palette().border),
        layout: flex! {
            direction: Layout::Row,
            gap: 6.0,
        },
        children: vec![
            pill("Ctrl", Rc::new(|| modifiers().ctrl)),
            pill("Shift", Rc::new(|| modifiers().shift)),
            pill("Alt", Rc::new(|| modifiers().alt)),
            pill("Meta", Rc::new(|| modifiers().meta)),
            pill("Pressed", Rc::new(|| pressed_node().is_some())),
            rich_text_node! {
                auto_width: true,
                spans: vec![label(
                    format!("最近按键: {}", last_key()),
                    palette().link,
                )],
            },
        ],
    }
}

/// `noWrap`：定宽盒里的超长文本被裁剪，既不折行也不补省略号（TS `nowrapDemo`）。
#[must_use]
pub fn nowrap_demo() -> Fc {
    rect_node! {
        width: 380.0,
        radius: 8.0,
        border_width: 1.0,
        padding_inline_start: 10.0,
        padding_inline_end: 10.0,
        padding_block_start: 8.0,
        padding_block_end: 8.0,
        background: Some(palette().card),
        border_color: Some(palette().border),
        layout: flex! {
            direction: Layout::Row,
            gap: 8.0,
        },
        children: vec![rich_text_node! {
            // 宽度由父布局分配（grow 吃掉剩余宽），再把溢出裁掉
            exts: vec![grow(1.0)],
            no_wrap: true,
            spans: vec![label(NO_WRAP_TEXT, text_color())],
        }],
    }
}

/// 内层滚动内容：12 个高 40 的条目（TS `tallItems`）。
fn tall_items() -> Vec<Fc> {
    (1..=12)
        .map(|i| {
            // 与 TS `tallItems` 一样**不写 layout**（→ 绝对定位）：交叉轴尺寸因此
            // 取自父布局分配（内容列是 `alignItem: Stretch`）。若在这里写
            // `flex! {}`，行会变成"尺寸由子节点撑开"（flex + justify:grow + 无
            // grow 子节点），`defaultSize` 就走 `sizeFromChildren`，宽度塌回文本宽。
            rect_node! {
                height: 40.0,
                radius: 6.0,
                padding_inline_start: 10.0,
                padding_inline_end: 10.0,
                background: Some(palette().card_done),
                children: vec![rich_text_node! {
                    auto_width: true,
                    spans: vec![label(format!("内层条目 {i}"), text_color())],
                }],
            }
        })
        .collect()
}

/// 嵌套滚动（TS `nestedScrollDemo`）：外层固定 180 高，内部再放一个 300 高的内层
/// 滚动区；内层滚到底后剩余滚轮量经 `parentScroll` 接续外层。
#[must_use]
pub fn nested_scroll_demo() -> Fc {
    rect_node! {
        width: 380.0,
        height: 180.0,
        padding: 8.0,
        radius: 8.0,
        background: Some(palette().card),
        border_color: Some(palette().border),
        border_width: 1.0,
        layout: flex! {
            direction: Layout::Column,
            gap: 4.0,
            align_fix: true,
            align_item: AlignItem::Stretch,
        },
        children: vec![
            rich_text_node! {
                auto_width: true,
                spans: vec![label(NESTED_TIP, placeholder_color())],
            },
            // 外层视口：占满剩余高度（TS `exts: [grow({argGrow: 1})]`）
            rect_node! {
                exts: vec![grow(1.0)],
                layout: flex! {
                    direction: Layout::Row,
                    align_item: AlignItem::Stretch,
                    align_fix: true,
                    direction_justify: DirectionJustify::Start,
                    gap: 4.0,
                },
                children: vec![outer_scroller()],
            },
        ],
    }
}

/// 外层滚动区（TS 里挂在视口下的那个 `function (outerCtx) { .. }`）。
///
/// 容器只能从构建上下文取父：`children` 是懒构建的，"滚动容器 = 我爹"这件事
/// 只有此刻问 [`NodeContext::parent`] 才知道（TS `outerCtx.getParent()`）。
fn outer_scroller() -> Fc {
    Fc::new(|cx: &mut NodeContext| {
        let container = cx
            .parent()
            .expect("外层滚动内容必须挂在视口矩形下（TS 同样依赖 getParent）");
        let outer = create_scroll(&container, Axis::Y);
        NESTED_OUTER.with(|s| *s.borrow_mut() = Some(outer.clone()));
        cx.child(scroll_content! {
            scroll: outer.clone(),
            exts: vec![grow(1.0)],
            layout: flex! {
                direction: Layout::Column,
                gap: 4.0,
                align_fix: true,
                align_item: AlignItem::Stretch,
            },
            children: vec![inner_viewport(outer.clone()), spacer()],
        });
        // 占位式滚动条（TS `scrollBar`）：与内容并列，按轨道厚度吃掉交叉轴
        cx.child(scroll_bar! {
            scroll: outer,
            track_color: get(move || Some(palette().track)),
            thumb_color: get(move || palette().thumb),
        });
    })
}

/// 内层视口：固定 300 高——它既是内层的可视区，又让外层有东西可滚
/// （TS 里那两个 `rectNode({ height: 300 })`）。
fn inner_viewport(outer: Scroll2) -> Fc {
    rect_node! {
        height: 300.0,
        layout: flex! {
            direction: Layout::Row,
            align_item: AlignItem::Stretch,
            align_fix: true,
            direction_justify: DirectionJustify::Start,
            gap: 4.0,
        },
        children: vec![Fc::new(move |cx: &mut NodeContext| {
            let container = cx
                .parent()
                .expect("内层滚动内容必须挂在视口矩形下（TS 同样依赖 getParent）");
            // TS `createScroll(parent, 'y', undefined, outerScroll)` 的第四个参数
            let inner = create_scroll(&container, Axis::Y).with_parent(outer);
            NESTED_INNER.with(|s| *s.borrow_mut() = Some(inner.clone()));
            cx.child(scroll_content! {
                scroll: inner.clone(),
                exts: vec![grow(1.0)],
                layout: flex! {
                    gap: 4.0,
                    align_fix: true,
                    align_item: AlignItem::Stretch,
                },
                children: tall_items(),
            });
            cx.child(scroll_bar! {
                scroll: inner,
                track_color: get(move || Some(palette().track)),
                thumb_color: get(move || palette().thumb),
            });
        })],
    }
}

/// 让外层「有事可滚」的空白块（TS 里第二个 `height: 300` 的矩形）。
fn spacer() -> Fc {
    rect_node! {
        height: 300.0,
    }
}
