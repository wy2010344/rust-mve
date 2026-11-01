//! 富文本节点（对标 TS `canvasRender2/RichTextNode.ts`）。
//!
//! TS 原文的结构：
//! ```ignore
//! export const richTextNode = tfc(function (ctx, node, arg: RichTextNodeArg) {
//!   const { spans = .., selectionColor = .., autoWidth = .., maxLines = ..,
//!           ellipsis = .., textAlign = .., ...rest } = arg;
//!   const fullText = () => spans().map(s => expandTabs(s.text)).join('');
//!   const paragraph = memo(() => ..buildParagraph(.., innerSize(node, 'x'), ..));
//!   ..Selectable2 / linkRanges..
//!   node2.create({
//!     ...rest,
//!     mouseClick(e) { rest.mouseClick?.(e); const url = linkAt(positionForPoint(..)); if (url) openUrl(url); },
//!     size(d) { .. },
//!     draw(ctx2, n) { rest.draw?.(ctx2, n); ..高亮..; drawParagraph(p, ctx2, padInlineStart, padBlockStart); },
//!   })(node)(ctx);
//! });
//! ```
//! 这里逐句对照，节点构造走 [`Node2::build`]（= `node2.create`）。
//!
//! # 与 TS 的机制映射（不是打折，是底层原语不同）
//!
//! - **整段绘制**：TS `drawParagraph(p.paragraph, ctx2, padX, padY)` 由 CanvasKit
//!   整段栅格化。Rust 侧对应 `Scene::draw_paragraph`（同样把一份 Parley 排版结果
//!   整段交给后端），行内偏移/行基线/跨 span 字距都来自排版结果本身——**不再**
//!   是"平均行高 × 行号 + 逐 span 量宽推进"。
//! - **`size('x')` 且 `!autoWidth`**：TS 返回
//!   `layoutValue(parent,'x').childSize(layoutIndex())` 且 `fromInside: false`，
//!   即"外包宽 = 父分配宽"。Rust 引擎把父分配的外包宽写回 `NodeGeom.width`，
//!   读它即为 `childSize`；主轴（父为 `Row`）例外，见 `size` 处的注释。
//! - `memo(...)` → [`ParaCache`]（键：显示文本 + 排版宽 + 最大行数 + 省略号 +
//!   对齐 + 各 span 样式）。
//! - `ctx.consume(selectionManagerContext)` → Rust 侧没有 context 读取通道，暂由
//!   调用方显式注入（见 [`RichTextNodeArg::selection_manager`]）。

use std::cell::RefCell;
use std::rc::Rc;

use wy_render::{Color, Point, Rect, Scene};
use wy_text::{
    build_paragraph, FontContext, RectStyle, TextAlign, TextParagraph, TextSpan, TextStyle,
};

use crate::canvas_render2::fc::{Fc, Ref2};
use crate::canvas_render2::node2::{always, Axis, Get, LayoutSize, Node2};
use crate::canvas_render2::protocol::MouseEvent2;
use crate::canvas_render2::selection::{Selectable2, SelectionManager2};
use crate::canvas_render2::tab::expand_tabs;
use crate::canvas_render2::text_defaults::text_color;
use crate::canvas_render2::url_opener::open_url;
use crate::Layout;
use crate::{rec, Node2Props};

// 跨调用复用的字体上下文（Parley 需要可变引用，用 thread_local 复用）。
thread_local! {
    static FC: RefCell<FontContext> = RefCell::new(FontContext::new());
}

/// 富文本片段（对标 TS `RichTextSpan`）：文本 + 样式 + 可选链接。
#[derive(Clone, Debug, PartialEq)]
pub struct RichTextSpan {
    /// 文本内容（可含 `'\t'`，排版前统一展开为空格）。
    pub text: String,
    /// 样式（字号 / 字重 / 斜体 / 颜色 / 间距 / 行高）。
    pub style: TextStyle,
    /// 链接地址（`Some` = 可点击，点击经 [`open_url`] 打开）。
    pub url: Option<String>,
}

impl RichTextSpan {
    /// 纯文本片段（给定颜色）。
    #[must_use]
    pub fn text(text: impl Into<String>, color: impl Into<Color>) -> Self {
        let color: Color = color.into();
        Self {
            text: text.into(),
            style: TextStyle::normal().with_color(color.to_u32()),
            url: None,
        }
    }

    /// 可点击链接片段。
    #[must_use]
    pub fn link(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::normal().with_color(Color::from_u32(0x2F81F7FF).to_u32()),
            url: Some(url.into()),
        }
    }

    /// 覆盖样式。
    #[must_use]
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }
}

/// 排版缓存键。
#[derive(Clone, PartialEq)]
struct ParaKey {
    text: String,
    styles: Vec<TextStyle>,
    width_bits: u32,
    max_lines: usize,
    ellipsis: String,
    align: TextAlign,
}

/// 富文本节点的 [`Selectable2`] 实现（TS 里这些方法直接挂在 `Node2` 上）。
///
/// 节点是纯数据面：只做坐标换算与文本查询，选区真相由管理器派生。
/// `range` 是"本节点上一帧被分配到的范围"的镜像——管理器派生出范围后由绘制
/// 回写，仅供 [`Selectable2::selection_rect`] 求包围盒（TS 的 `selectionRect()`
/// 同样需要自身范围，这里不另设真相）。
struct RichTextSelectable {
    /// 当前排版（`None` = 无文本）。直读，不做镜像——TS `selectionRect()` 同样
    /// 现读 `paragraph()`，镜像只会让"包围盒"慢一帧。
    para: Rc<dyn Fn() -> Option<Rc<TextParagraph>>>,
    /// 显示文本长度。
    len: Rc<dyn Fn() -> usize>,
    /// 本节点（求绝对坐标用）。
    ///
    /// 不能用 `NodeRef` 的 `NodeGeom`：那份 cell 是 v1 引擎的布局回填口，
    /// Node2 原生管线（离屏快照 / `render_node2`）从不写它，读出来恒为 0——
    /// 命中换算会把全局坐标当局部坐标，选区永远落错位置。
    /// TS 用的是 `absoluteX(node)`（沿父链消息求值），这里同源。
    node: Node2,
    /// 节点内 `(x, y)` → 文本偏移。
    local_position: Rc<dyn Fn(f32, f32) -> usize>,
    /// 内边距起点（正文相对节点原点的偏移）。
    origin: Rc<dyn Fn() -> (f32, f32)>,
    /// 文本快照（切片用）。
    text: Rc<dyn Fn() -> String>,
    /// 取当前选区管理器（TS 构造期 `ctx.consume(selectionManagerContext)`，
    /// 这里延后成闭包：管理器可能在组件装配之后才装上）。
    manager: Rc<dyn Fn() -> Option<Rc<SelectionManager2>>>,
}

impl Selectable2 for RichTextSelectable {
    /// 选中区域的包围盒（全局坐标）：段落内矩形 + 节点绝对位置。
    fn selection_rect(&self) -> Option<crate::canvas_render2::selection::Rect2> {
        let p = (self.para)()?;
        // TS 同样是现读 `selectionManager.rangeOf(selectable)`，不存镜像。
        let (s, e) = (self.manager)()?.range_of_node(&self.node)?;
        if s >= e {
            return None;
        }
        let (ax, ay) = (self.node.abs_x(), self.node.abs_y());
        let (ox, oy) = (self.origin)();
        let mut result: Option<crate::canvas_render2::selection::Rect2> = None;
        for r in p.rects_for_range(s, e, RectStyle::Tight) {
            let rect = crate::canvas_render2::geometry::RectF::new(
                ax + ox + r.left,
                ay + oy + r.top,
                ax + ox + r.right,
                ay + oy + r.bottom,
            );
            result = Some(match result {
                Some(prev) => crate::canvas_render2::geometry::RectF::new(
                    prev.left.min(rect.left),
                    prev.top.min(rect.top),
                    prev.right.max(rect.right),
                    prev.bottom.max(rect.bottom),
                ),
                None => rect,
            });
        }
        result
    }

    fn text_length(&self) -> usize {
        (self.len)()
    }

    fn position_for_point(&self, global_x: f64, global_y: f64) -> usize {
        let (ax, ay) = (self.node.abs_x(), self.node.abs_y());
        (self.local_position)(global_x as f32 - ax, global_y as f32 - ay)
    }

    fn text_in_range(&self, start: usize, end: usize) -> String {
        let t = (self.text)();
        t.get(start..end.min(t.len()))
            .unwrap_or_default()
            .to_string()
    }

    /// 包含 `offset` 的词边界（双击选词）。TS 用 `wordRangeAtIn(fullText, off)`。
    ///
    /// 偏移空间统一到**字节**：段落（Parley）给的偏移是字节，而 [`Words`] 按
    /// 字符索引返回（对齐 TS 的 UTF-16 索引）。CJK 上两者数值不同，直接混用会让
    /// 双击选词落在半个字中间——所以这里做一次双向换算。
    fn word_range_at(&self, offset: usize) -> Option<(usize, usize)> {
        let t = (self.text)();
        let off = offset.min(t.len());
        let off_char = t[..off].chars().count();
        let (cs, ce) = crate::canvas_render2::graphemes::Words::word_range_at(&t, off_char)?;
        let char_to_byte = |c: usize| t.char_indices().nth(c).map_or(t.len(), |(b, _)| b);
        let (bs, be) = (char_to_byte(cs), char_to_byte(ce));
        (be > bs).then_some((bs, be))
    }

    /// 包含 `offset` 的逻辑段落范围（三击选段，`'\n'` 分隔）。
    fn paragraph_range_at(&self, offset: usize) -> Option<(usize, usize)> {
        let t = (self.text)();
        if t.is_empty() {
            return None;
        }
        // 点在换行符上归前段，末段延伸到文末（与 TS 同）。
        let d = offset.min(t.len());
        let start = if d == 0 {
            0
        } else {
            t[..d].rfind('\n').map_or(0, |i| i + 1)
        };
        let end = t[d..].find('\n').map_or(t.len(), |i| d + i);
        Some((start, end))
    }
}

/// 排版缓存（对标 TS `memo(paragraph)`）。
#[derive(Default)]
struct ParaCache {
    key: Option<ParaKey>,
    para: Option<Rc<TextParagraph>>,
}

/// 把片段转成 Parley 的 `TextSpan` + 各自字节区间 + 显示文本（tab 已展开）。
fn build_spans(raw: &[RichTextSpan]) -> (Vec<TextSpan>, Vec<(usize, usize)>, String) {
    let mut spans = Vec::with_capacity(raw.len());
    let mut ranges = Vec::with_capacity(raw.len());
    let mut full = String::new();
    for r in raw {
        let text = expand_tabs(&r.text);
        let start = full.len();
        full.push_str(&text);
        spans.push(TextSpan {
            text,
            style: r.style.clone(),
        });
        ranges.push((start, full.len()));
    }
    (spans, ranges, full)
}

/// 构建排版（带兜底：空片段也返回一个可用的空排版）。
///
/// `pub(crate)`：`text_metric` 的墨迹补偿测试要造一段真排版来量紧致盒。
pub(crate) fn build_para(
    spans: &[TextSpan],
    max_width: Option<f32>,
    max_lines: usize,
    ellipsis: &str,
    align: TextAlign,
) -> TextParagraph {
    FC.with(|fc| {
        build_paragraph(
            &mut fc.borrow_mut(),
            spans,
            max_width,
            max_lines,
            ellipsis,
            align,
        )
        .unwrap_or_else(|_| {
            build_paragraph(
                &mut fc.borrow_mut(),
                &[TextSpan {
                    text: String::new(),
                    style: TextStyle::normal().with_font_size(12.0).with_color(0),
                }],
                None,
                usize::MAX,
                "",
                align,
            )
            .unwrap()
        })
    })
}

/// 取（或按需重建）本帧排版，返回 `(排版, spans, 各 span 字节区间)`。
///
/// 缓存按 [`ParaKey`] 失效：文本 / 样式 / 排版宽 / 最大行数 / 省略号 / 对齐
/// 任一变化都重排——对标 TS `memo` 的依赖集合。
fn ensure_para(
    cache: &Rc<RefCell<ParaCache>>,
    raw: &[RichTextSpan],
    width: Option<f32>,
    max_lines: usize,
    ellipsis: &str,
    align: TextAlign,
) -> (Rc<TextParagraph>, Vec<TextSpan>, Vec<(usize, usize)>) {
    let (spans, ranges, full) = build_spans(raw);
    let key = ParaKey {
        text: full,
        styles: spans.iter().map(|s| s.style.clone()).collect(),
        width_bits: width.unwrap_or(0.0).to_bits(),
        max_lines,
        ellipsis: ellipsis.to_string(),
        align,
    };
    {
        let c = cache.borrow();
        if c.key.as_ref() == Some(&key) {
            if let Some(p) = c.para.as_ref() {
                return (Rc::clone(p), spans, ranges);
            }
        }
    }
    let p = Rc::new(build_para(&spans, width, max_lines, ellipsis, align));
    let mut c = cache.borrow_mut();
    c.key = Some(key);
    c.para = Some(Rc::clone(&p));
    (p, spans, ranges)
}

/// 绘制排版：整段交给后端栅格化（TS `drawParagraph(p.paragraph, ctx2, padX, padY)`）。
///
/// 这里**必须**整段画。曾经是"平均行高 × 行号 + 逐 span `measureText` 推进 x"，
/// 有两个必然的错：
///
/// - `textAlign: Center/End` 的行内偏移由排版器给出（`cluster.visual_offset()`），
///   自己从 `origin.0` 起画就丢掉了它——于是"选区高亮居中、文字靠左"。
/// - 混排字号时行高不相等，`p.height()/行数` 只是平均值，行位置会漂、行间会重叠。
///
/// 现在行内偏移、行基线、跨 span 字距全部来自**同一份** `TextParagraph`，和
/// [`TextParagraph::rects_for_range`]（选区高亮）用的是同一套几何。
fn draw_para(s: &mut Scene, p: &TextParagraph, origin: (f32, f32)) {
    s.draw_paragraph(Point::new(origin.0, origin.1), p.shared_layout());
}

/// 富文本节点（对标 TS `richTextNode`）。
///
/// 文本选择能力放在本节点：只负责把节点内坐标换算成文本偏移、绘制被分配的
/// 选区高亮；选区真相由 [`SelectionManager2`] 持有（见 TS 同名注释）。
#[rec(extends = node2)]
#[must_use]
// 参数就是 TS `RichTextArg` 的逐字段摊平（18 个），拆成结构体只会多一层中间类型。
#[allow(clippy::too_many_arguments)]
pub fn rich_text_node(
    /// 文本片段，每个片段可带独立样式（默认空）。
    #[getter]
    spans: Vec<RichTextSpan>,
    /// 选区颜色，默认 `rgba(100, 100, 200, 60)`。
    #[getter]
    selection_color: Color,
    /// 宽度是否由内容决定；`false` 时由父布局分配。
    #[getter]
    auto_width: bool,
    /// 单行不折行：按无限宽排版、溢出裁到内容盒。
    ///
    /// 与 `max_lines == 1` 的区别是**截断策略**：`max_lines = 1` 仍按盒宽折行、
    /// 只留首行并补省略号（断点位置受盒宽影响）；`no_wrap` 完全不折行，超出部分
    /// 靠裁剪隐藏。`auto_width` 时宽度本来就由内容决定，裁剪无意义，故不裁
    /// （对标 Kotlin `RichTextNode.noWrap`，TS `RichTextNode.ts` 同名参数）。
    #[getter]
    no_wrap: bool,
    /// 最大行数，默认 `usize::MAX`。
    #[getter]
    max_lines: usize,
    /// 省略号，默认 `'…'`。
    #[getter]
    ellipsis: String,
    /// 对齐方式，默认 [`TextAlign::Start`]。
    #[getter]
    text_align: TextAlign,
    /// 选区管理器。
    ///
    /// TS 从 `ctx.consume(selectionManagerContext)` 取；Rust 构造期只拿到节点、
    /// 拿不到 `ctx`，因此暂由调用方显式注入（等 `SelectionManager` 改写成 TS 的
    /// 派生模型时，这一项会随 `consume` 一起消失）。
    selection_manager: Option<Rc<SelectionManager2>>,
    /// 节点句柄槽（对齐 TS `arg.nodeRef`）：**普通参数**，不传则内部新建、
    /// 调用方拿不到该节点；需要句柄时显式传一个 `Ref2<Node2>`。
    #[props(default = Ref2::new())]
    node_ref: Ref2<Node2>,
) -> Fc {
    let mut rest = rest;
    let node = Node2::empty();

    let spans: Get<Vec<RichTextSpan>> = match spans {
        Some(f) => f,
        None => {
            let color = text_color();
            Rc::new(move || vec![RichTextSpan::text(String::new(), color)])
        }
    };
    let selection_color = selection_color.unwrap_or_else(|| always(Color::rgba(100, 100, 200, 60)));
    let auto_width = auto_width.unwrap_or_else(|| always(false));
    let no_wrap = no_wrap.unwrap_or_else(|| always(false));
    let max_lines = max_lines.unwrap_or_else(|| always(usize::MAX));
    let ellipsis = ellipsis.unwrap_or_else(|| always("\u{2026}".to_string()));
    let text_align = text_align.unwrap_or_else(|| always(TextAlign::Start));

    // 全文（tab 展开后的显示文本）——TS `fullText`。
    let full_text: Rc<dyn Fn() -> String> = {
        let spans = Rc::clone(&spans);
        Rc::new(move || spans().iter().map(|s| expand_tabs(&s.text)).collect())
    };
    // TS `maxFontSizeInSpans`：至少 1，用于无排版时的行高兜底。
    let max_font_size: Rc<dyn Fn() -> f32> = {
        let spans = Rc::clone(&spans);
        Rc::new(move || {
            spans()
                .iter()
                .map(|s| s.style.font_size)
                .fold(0.0f32, f32::max)
                .max(1.0)
        })
    };

    let cache: Rc<RefCell<ParaCache>> = Rc::new(RefCell::new(ParaCache::default()));
    let geom = node.node_ref();

    // TS `const paragraph = memo(() => ..)`。
    let paragraph: Rc<dyn Fn() -> Option<Rc<TextParagraph>>> = {
        let cache = Rc::clone(&cache);
        let geom = geom.clone();
        let spans = Rc::clone(&spans);
        let full_text = Rc::clone(&full_text);
        let auto_width = Rc::clone(&auto_width);
        let no_wrap = Rc::clone(&no_wrap);
        let max_lines = Rc::clone(&max_lines);
        let ellipsis = Rc::clone(&ellipsis);
        let text_align = Rc::clone(&text_align);
        let me = node.clone();
        Rc::new(move || {
            if full_text().is_empty() {
                return None;
            }
            // TS: `const infinite = autoWidth() || noWrap()`；
            // 无限宽排版下"要不要折行"由节点语义定，不由盒宽定。
            let infinite = auto_width() || no_wrap();
            // 折行宽 = **声明宽 - 两侧 padding**（对标 TS `innerSize(ref.get(),'x')`）。
            //
            // 不能直接用 `geom.inner()`：`geom` 是最终几何 cell，由引擎在布局后写回，
            // 布局/离屏快照阶段常常还是 0——那样 `max_width` 会退化成"无限宽"，
            // 定宽富文本整行溢出、永不折行。优先读显式声明的宽度（`explicit`），
            // 拿不到时再退回几何 cell（拉伸/父分配的场景）。
            let max_width = if infinite {
                None
            } else {
                let (ps, pe) = (me.padding_inline_start(), me.padding_inline_end());
                let inner = match me.explicit(Axis::X) {
                    Some(outer) => outer - ps - pe,
                    None => geom.inner().0,
                };
                if inner > 0.0 {
                    Some(inner)
                } else {
                    None
                }
            };
            let (p, _, _) = ensure_para(
                &cache,
                &spans(),
                max_width,
                max_lines(),
                &ellipsis(),
                // 无限宽时若仍传 END/CENTER，SkParagraph 会把对齐烘进无穷宽坐标系、
                // 字形被推出盒外（整段消失）。统一按 START 排，对齐靠原点平移补。
                if infinite {
                    TextAlign::Start
                } else {
                    text_align()
                },
            );
            Some(p)
        })
    };

    // 无限宽排版下的行对齐平移量（Kotlin/TS `noWrapAlignShift`）：
    // 段落按 START 排，CENTER/END 得平移原点补回来，绘制/选区/命中同源。
    let align_shift: Rc<dyn Fn() -> f32> = {
        let paragraph = Rc::clone(&paragraph);
        let auto_width = Rc::clone(&auto_width);
        let no_wrap = Rc::clone(&no_wrap);
        let text_align = Rc::clone(&text_align);
        let n = node.clone();
        Rc::new(move || {
            if auto_width() || !no_wrap() {
                return 0.0;
            }
            let Some(p) = paragraph() else {
                return 0.0;
            };
            let box_w = n.inner_size(Axis::X);
            match text_align() {
                TextAlign::Center => (box_w - p.width()) / 2.0,
                TextAlign::End => box_w - p.width(),
                _ => 0.0,
            }
        })
    };

    // 文本原点（节点坐标系）= padding 起点 + 单行墨迹补偿（TS `textOriginX/Y`）。
    // 绘制、选区矩形、命中全走它，保证"看到的 = 点到的"。
    let text_origin: Rc<dyn Fn() -> (f32, f32)> = {
        let paragraph = Rc::clone(&paragraph);
        let full_text = Rc::clone(&full_text);
        let auto_width = Rc::clone(&auto_width);
        let no_wrap = Rc::clone(&no_wrap);
        let max_lines = Rc::clone(&max_lines);
        let align_shift = Rc::clone(&align_shift);
        let n = node.clone();
        Rc::new(move || {
            let p = paragraph();
            // 单行墨迹补偿的条件：`autoWidth || noWrap || maxLines == 1`。
            // `noWrap` 必须是单行——它按无限宽排、永不产生软换行，行盒里只有一行墨迹；
            // 漏掉它会让单行标签整体下沉 1~2px（Kotlin 原版把这三种都算上）。
            let ink = crate::canvas_render2::text_metric::ink_shift_y_of(
                p.as_deref(),
                full_text().len(),
                n.inner_size(Axis::Y),
                auto_width() || no_wrap() || max_lines() == 1,
            );
            (
                n.padding_inline_start() + align_shift(),
                n.padding_block_start() + ink,
            )
        })
    };

    // 节点内坐标 → 文本偏移（TS `Selectable2.positionForPoint`）。
    let position_for_point: Rc<dyn Fn(f32, f32) -> usize> = {
        let paragraph = Rc::clone(&paragraph);
        let full_text = Rc::clone(&full_text);
        let origin = Rc::clone(&text_origin);
        Rc::new(move |x: f32, y: f32| {
            let Some(p) = paragraph() else {
                return 0;
            };
            let (ox, oy) = origin();
            let off = p.glyph_position_at_coordinate(x - ox, y - oy);
            off.min(full_text().len())
        })
    };

    // 命中偏移处的链接（TS `linkAt`）。
    let link_at: Rc<dyn Fn(usize) -> Option<String>> = {
        let spans = Rc::clone(&spans);
        Rc::new(move |offset: usize| {
            let mut off = 0usize;
            for s in spans() {
                let len = expand_tabs(&s.text).len();
                if len > 0 && s.url.is_some() && offset >= off && offset < off + len {
                    return s.url.clone();
                }
                off += len;
            }
            None
        })
    };

    // TS `size(d)`。
    let size = {
        let paragraph = Rc::clone(&paragraph);
        let auto_width = Rc::clone(&auto_width);
        let max_font_size = Rc::clone(&max_font_size);
        let me = node.clone();
        Rc::new(move |d: Axis| -> LayoutSize {
            let p = paragraph();
            match d {
                // TS 原文：
                // ```ts
                // if (d == 'x' && !autoWidth()) {
                //   const x = layoutValue(node, d);
                //   if (x.allowSizeFromChildren()) { ..fromInside: true.. }
                //   return { value: layoutValue(parent, d).childSize(node.layoutIndex()),
                //            fromInside: false };
                // }
                // ```
                // 叶子节点的 `layout` 是 `absoluteLayoutObject`，
                // `allowSizeFromChildren()` 恒 `false`，所以**一定**走父分配分支，
                // 且 `fromInside` 为 `false`：外包宽 = 父分给这个节点的宽。
                //
                // 旧实现返回 `inside(min(段落宽, 父内区宽))`——那是把"父分配"
                // 换成了"段落自然宽夹到父宽"，于是父给 320、文字只有 60 时，
                // 节点缩成 60 而不是占满 320（`docs/05 §4.4` 曾错称二者逐像素一致）。
                Axis::X if !auto_width() => {
                    // TS 的 `childSize` = 父布局分配给本节点的外包宽。Rust 侧没有
                    // 布局对象的写回（`NodeGeom` 在 Node2 原生管线下从不被写），
                    // 于是取**声明宽**（`width: ..`）——"宽度交给父布局"时它就是
                    // 父分到的那个值；拿不到再退回段落自然宽。
                    let allotted = me.explicit(Axis::X).unwrap_or(0.0);
                    // 主轴例外：主轴长度由 flex 在**自然尺寸之后**分配，
                    // 用上一帧分配值会反过来抬高自然尺寸总和，形成正反馈。
                    // 这里取"主轴用段落自然宽、交叉轴用父分配宽"。
                    let is_main = me
                        .parent()
                        .is_some_and(|parent| parent.layout().dir() == Layout::Row);
                    if !is_main && allotted > 0.0 {
                        LayoutSize::outer(allotted)
                    } else {
                        LayoutSize::inside(p.as_ref().map_or(0.0, |p| p.width()))
                    }
                }
                Axis::X => LayoutSize::inside(p.as_ref().map_or(0.0, |p| p.width())),
                Axis::Y => LayoutSize::inside(
                    p.as_ref()
                        .map_or_else(|| max_font_size() * 1.4, |p| p.height()),
                ),
            }
        })
    };
    rest.size = Some(size);

    // TS `mouseClick`：用户回调先行，命中链接再打开。
    let rest_click = rest.mouse_click.take();
    {
        let position_for_point = Rc::clone(&position_for_point);
        let link_at = Rc::clone(&link_at);
        rest.mouse_click = Some(Rc::new(move |ev: &mut MouseEvent2| {
            if let Some(f) = &rest_click {
                f(ev);
            }
            if let Some(url) = link_at(position_for_point(ev.x, ev.y)) {
                open_url(&url);
            }
        }));
    }

    // 可选能力经 `exts` 里的 [`SelectableExt`] 标记声明（TS 同）：节点不持有选区
    // 真相，范围由管理器按文档序纯派生，绘制时读回。
    //
    // **这里刻意不装 `mouse_down/move/up`**：TS 的 `richTextNode` 也没有这些。
    // 选区会话由指针路由（`tree/pointer` 的 press + moveHitTest）驱动，落点是
    // `SelectionManager`——只有这样才能"从 A 节点拖到 B 节点"；节点自装拖选
    // 只可能在同一节点内选（父节点拿不到另一个节点的坐标），那正是旧实现
    // "跨组件选择不行"的根因。
    let manager_of: Rc<dyn Fn() -> Option<Rc<SelectionManager2>>> = {
        let explicit = selection_manager.clone();
        Rc::new(move || {
            explicit
                .clone()
                .or_else(crate::canvas_render2::selection::current_selection_manager)
        })
    };
    let selectable: (Rc<RichTextSelectable>, Rc<dyn Selectable2>) = {
        let s = Rc::new(RichTextSelectable {
            para: Rc::clone(&paragraph),
            len: {
                let full_text = Rc::clone(&full_text);
                Rc::new(move || full_text().len())
            },
            node: node.clone(),
            local_position: Rc::clone(&position_for_point),
            origin: Rc::clone(&text_origin),
            text: Rc::clone(&full_text),
            manager: Rc::clone(&manager_of),
        });
        let dyn_sel = Rc::clone(&s) as Rc<dyn Selectable2>;
        rest.exts
            .push(Rc::new(crate::canvas_render2::selection::SelectableExt(
                Rc::clone(&dyn_sel),
            )));
        (s, dyn_sel)
    };

    // TS `draw(ctx2, n)`。
    let rest_draw = rest.draw.take();
    {
        let selection_color = Rc::clone(&selection_color);
        let auto_width = Rc::clone(&auto_width);
        let no_wrap = Rc::clone(&no_wrap);
        let paragraph = Rc::clone(&paragraph);
        let text_origin = Rc::clone(&text_origin);
        let sel_ref: Rc<dyn Selectable2> = Rc::clone(&selectable.1);
        let manager_of = Rc::clone(&manager_of);
        rest.draw = Some(Rc::new(move |s: &mut Scene, n: &Node2| {
            if let Some(f) = &rest_draw {
                f(s, n);
            }
            // 排版与原点都走装配期那两份闭包：绘制 / 选区 / 命中同源，
            // 不会出现"画的时候用一套宽度、命中时用另一套"。
            let Some(p) = paragraph() else { return };
            let origin = text_origin();
            // 单行不折行：溢出裁到内容盒（对齐平移后的字形也不该画到盒外）。
            let clip = no_wrap() && !auto_width();
            if clip {
                s.push_clip(Rect::new(
                    n.padding_inline_start(),
                    n.padding_block_start(),
                    n.inner_size(Axis::X).max(0.0),
                    n.inner_size(Axis::Y).max(0.0),
                ));
            }
            // 高亮：范围由 SelectionManager 按文档序纯派生后读回
            // （TS `selectionManager.rangeOf(selectable)`）。
            if let Some(manager) = manager_of() {
                if let Some((ss, se)) = manager.range_of(&sel_ref) {
                    let color = selection_color();
                    for r in p.rects_for_range(ss, se, RectStyle::Tight) {
                        s.fill_rect(
                            Rect::new(
                                r.left + origin.0,
                                r.top + origin.1,
                                r.right - r.left,
                                r.bottom - r.top,
                            ),
                            color,
                        );
                    }
                }
            }
            draw_para(s, &p, origin);
            if clip {
                s.pop_clip();
            }
        }));
    }

    // 无排版时不产生任何尺寸：`size` 会退回 `max_font_size * 1.4`。
    let _ = paragraph;
    let node = node.build(rest);
    node_ref.set(node.clone());
    Fc::from(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_of(text: Rc<RefCell<String>>) -> Rc<dyn Fn() -> Vec<RichTextSpan>> {
        Rc::new(move || vec![RichTextSpan::text(text.borrow().clone(), Color::BLACK)])
    }

    /// 缓存必须按 (文本, 宽度) 失效：宽度变窄要换行、文本变长要重排。
    ///
    /// 这条测试盯的是两个曾经的真 bug：
    /// 1. 排版宽度写死构造期 `max_width`，父容器重分宽度后不重排；
    /// 2. 缓存无条件复用（`Option::take` 后无脑回写），文本变了还画旧排版。
    #[test]
    fn para_cache_invalidates_on_width_and_text() {
        let cell: Rc<RefCell<ParaCache>> = Rc::new(RefCell::new(ParaCache::default()));
        let text = Rc::new(RefCell::new(
            "the quick brown fox jumps over the lazy dog".to_string(),
        ));
        let get_spans = spans_of(Rc::clone(&text));

        let (wide, _, _) = ensure_para(
            &cell,
            &get_spans(),
            Some(400.0),
            usize::MAX,
            "…",
            TextAlign::Start,
        );
        let (same, _, _) = ensure_para(
            &cell,
            &get_spans(),
            Some(400.0),
            usize::MAX,
            "…",
            TextAlign::Start,
        );
        assert!(
            Rc::ptr_eq(&wide, &same),
            "同宽度同文本应命中缓存（同一个排版实例）"
        );

        let (narrow, _, _) = ensure_para(
            &cell,
            &get_spans(),
            Some(100.0),
            usize::MAX,
            "…",
            TextAlign::Start,
        );
        assert!(!Rc::ptr_eq(&wide, &narrow), "宽度变化必须重建排版");
        assert!(
            narrow.height() > wide.height(),
            "窄宽度应换行成更多行：wide={} narrow={}",
            wide.height(),
            narrow.height()
        );

        let (before_text, _, _) = ensure_para(
            &cell,
            &get_spans(),
            Some(100.0),
            usize::MAX,
            "…",
            TextAlign::Start,
        );
        *text.borrow_mut() = "短".to_string();
        let (after_text, _, _) = ensure_para(
            &cell,
            &get_spans(),
            Some(100.0),
            usize::MAX,
            "…",
            TextAlign::Start,
        );
        assert!(
            !Rc::ptr_eq(&before_text, &after_text),
            "文本变化必须重建排版"
        );
        assert!(after_text.height() < before_text.height());
    }

    /// `maxLines` + `ellipsis`：超行必须截断（对标 TS `buildParagraph`）。
    #[test]
    fn max_lines_truncates_with_ellipsis() {
        let cell: Rc<RefCell<ParaCache>> = Rc::new(RefCell::new(ParaCache::default()));
        let text = Rc::new(RefCell::new(
            "the quick brown fox jumps over the lazy dog and keeps running far away".to_string(),
        ));
        let get_spans = spans_of(text);
        let (full, _, _) = ensure_para(
            &cell,
            &get_spans(),
            Some(100.0),
            usize::MAX,
            "…",
            TextAlign::Start,
        );
        let (cut, _, _) = ensure_para(&cell, &get_spans(), Some(100.0), 1, "…", TextAlign::Start);
        assert_eq!(cut.line_metrics().len(), 1, "截断后必须只有 1 行");
        assert!(cut.height() < full.height());
        assert!(cut.text().ends_with('…'), "截断文本末尾要有省略号");
    }

    /// tab 必须在 `rich_text_node` 内展开（TS `expandTabs` 就在 RichTextNode 里）。
    #[test]
    fn tabs_expand_inside_rich_text() {
        let (spans, ranges, full) = build_spans(&[RichTextSpan::text("a\tb", Color::BLACK)]);
        assert_eq!(full, "a    b");
        assert_eq!(ranges, vec![(0, 6)]);
        assert_eq!(spans[0].text, "a    b");
    }
    /// `no_wrap`：单行不折行（无限宽排版），溢出裁到内容盒。
    ///
    /// 与 `max_lines == 1` 的区别是**截断策略**（折行后补省略号 vs 完全不折行靠裁剪）。
    /// 这条测试盯两件事：
    /// 1. 同样一段长文本，`no_wrap` 走无限宽排版 → 单行；给了窄盒宽 → 多行。
    ///    前者高度应低于后者（也不依赖布局时机，直接用 `ensure_para` 比）。
    /// 2. 绘制有 clip push/pop 包住 —— 否则溢出的字会画到盒外。
    ///
    /// `no_wrap` 也计入单行光学校正（见 Kotlin `inkShiftY` 的
    /// `autoWidth || noWrap || maxLines == 1`），与 TS / Kotlin 对齐。
    #[test]
    fn no_wrap_lays_out_single_line_and_clips() {
        let text = "no wrap single line that is much wider than the box";
        let spans = vec![RichTextSpan::text(text, Color::BLACK)];

        // 固定盒宽 160：应折成多行（高）。
        let cw: Rc<RefCell<ParaCache>> = Rc::new(RefCell::new(ParaCache::default()));
        let (wrapped, _, _) =
            ensure_para(&cw, &spans, Some(160.0), usize::MAX, "…", TextAlign::Start);
        // 无限宽（`no_wrap` 的排版口径）：单行。
        let ci: Rc<RefCell<ParaCache>> = Rc::new(RefCell::new(ParaCache::default()));
        let (single, _, _) = ensure_para(&ci, &spans, None, usize::MAX, "…", TextAlign::Start);

        assert_eq!(single.line_metrics().len(), 1, "no_wrap 必须单行");
        assert!(wrapped.line_metrics().len() > 1, "固定盒宽应折成多行");
        assert!(
            wrapped.height() > single.height(),
            "固定盒宽的多行应比单行高：{} vs {}",
            wrapped.height(),
            single.height()
        );

        // 渲染带 no_wrap 的节点：绘制必须成对 clip 到内容盒。
        // 声明式写法（与产品代码同构），不写 `RichTextNodeArg::builder()` 链。
        let root = crate::canvas_render2::build_root_fc(node2! {
            x: 0.0,
            y: 0.0,
            width: 420.0,
            height: 200.0,
            children: vec![rich_text_node! {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                no_wrap: true,
                spans: vec![RichTextSpan::text(
                    "no wrap single line that is much wider than the box",
                    Color::BLACK,
                )],
            }],
        });
        let mut scene = Scene::new();
        root.render_node2(&mut scene);

        let mut clips = 0;
        let mut pops = 0;
        for p in scene.iter() {
            match p {
                wy_render::Primitive::ClipPush { .. } => clips += 1,
                wy_render::Primitive::ClipPop => pops += 1,
                _ => {}
            }
        }
        assert_eq!((clips, pops), (1, 1), "no_wrap 必须成对裁到内容盒");
    }

    /// 跨组件选择必须**真的画出高亮**（"选中状态没有渲染"的回归门）。
    ///
    /// 三个断言连成一条完整链路：
    /// 1. 富文本节点带 `SelectableExt`（`as_selectable2` 取得到）；
    /// 2. 管理器按文档序分配 `A[2..] + B[..3]`（跨节点连续）；
    /// 3. 绘制端读回范围，**两个节点上各铺一块选中底色**。
    ///
    /// 第 3 条是关键：范围算对但没画，用户看到的就是"选择完全没反应"。
    #[test]
    fn highlight_paints_across_two_rich_text_nodes() {
        use crate::canvas_render2::selection::{
            as_selectable2, HitestResult2, PointerSelect2, SelectionManager2, SelectionSource2,
        };

        /// 最小派生源：只给根节点（程序化选区不需要指针会话）。
        struct RootOnly(Rc<RefCell<Option<Node2>>>);
        impl SelectionSource2 for RootOnly {
            fn pointer_select(&self) -> Option<PointerSelect2> {
                None
            }
            fn move_hit_test(&self) -> Option<HitestResult2> {
                None
            }
            fn root_node(&self) -> Option<Node2> {
                self.0.borrow().clone()
            }
            fn clear_pointer_select(&self) {}
        }

        const SEL_BG: Color = Color::rgba(100, 100, 200, 60);

        // 管理器要先于组件存在（组件装配期就捕获它），根节点稍后回填。
        let slot: Rc<RefCell<Option<Node2>>> = Rc::new(RefCell::new(None));
        let manager = Rc::new(SelectionManager2::new(
            Rc::new(RootOnly(Rc::clone(&slot))) as Rc<dyn SelectionSource2>
        ));

        let root = crate::canvas_render2::build_root_fc(node2! {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 200.0,
            children: vec![
                rich_text_node! {
                    x: 80.0,
                    y: 10.0,
                    width: 240.0,
                    height: 24.0,
                    selection_manager: Rc::clone(&manager),
                    spans: vec![RichTextSpan::text("hello", Color::BLACK)],
                },
                rich_text_node! {
                    x: 80.0,
                    y: 50.0,
                    width: 240.0,
                    height: 24.0,
                    selection_manager: Rc::clone(&manager),
                    spans: vec![RichTextSpan::text("world", Color::BLACK)],
                },
            ],
        });
        *slot.borrow_mut() = Some(root.clone());

        let kids = root.children();
        assert_eq!(kids.len(), 2, "根应有两个富文本子节点");
        let (a, b) = (kids[0].clone(), kids[1].clone());
        let sel_a = as_selectable2(&a).expect("A 应带 SelectableExt");
        let sel_b = as_selectable2(&b).expect("B 应带 SelectableExt");

        // 跨节点：A 从第 2 字符到末尾 + B 从开头到第 3 字符。
        assert!(manager.select(&a, 2, &b, 3), "两个节点都应在可选集合内");
        assert_eq!(manager.range_of(&sel_a), Some((2, 5)), "A：锚点到末尾");
        assert_eq!(manager.range_of(&sel_b), Some((0, 3)), "B：开头到焦点");
        assert_eq!(manager.selected_text(), Some("llowor".to_string()));

        // 命中换算必须用**绝对坐标**（节点在 x=80 / y=50，不是原点）。
        assert_eq!(
            sel_a.position_for_point(80.0 + 2.0, 10.0 + 5.0),
            0,
            "A 首字在前几个像素内：局部坐标要减掉节点绝对位置"
        );
        assert!(
            sel_b.position_for_point(80.0 + 600.0, 50.0 + 5.0) >= 3,
            "越过 B 右边界应落在文本末尾附近"
        );

        // 画：两个节点都必须铺出选中底色。
        let mut scene = Scene::new();
        root.render_node2(&mut scene);
        let mut fills = 0;
        for p in scene.iter() {
            if let wy_render::Primitive::Rect { color, .. } = p {
                if *color == SEL_BG {
                    fills += 1;
                }
            }
        }
        assert!(
            fills >= 2,
            "跨组件选区必须在两个节点上都画出高亮，实际 {fills} 块"
        );
    }
}
