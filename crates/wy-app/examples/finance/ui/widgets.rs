//! 基础组件：面板外壳、表头行、文本、小按钮、键值行。
//!
//! 全部是 `#[component]`：文案/颜色是**绘制期求值的闭包**，`children` 是插槽。
//! 在 `rsx!` 里可以给任何组件再挂 `grow` / `width` / `draw` / `on_click` 这类
//! 通用属性——组件和容器一样是布局节点，不必为每个组件单独开 `w` / `h` 参数。
//!
//! 构造时**不算任何坐标**：位置和尺寸交给父容器的 flex 排布，绘制期用
//! [`wy_mve::current_box`] 读自己分到的尺寸。这是本示例与「手算矩形再整体绘制」
//! 的分水岭。
//!
//! 视觉语言：8px 圆角面板、35px 标题栏、1px 边框、斑马纹行、表头用 FAINT 小字。

use std::rc::Rc;

use wy_mve::{component, current_box, rsx, AlignItem, Node, Slot};
use wy_render::{Color, Point, Rect, Scene};
use wy_signal::{GetValue, SetValue, Signal};

use crate::theme;

/// 面板标题栏高度。
pub const HEADER_H: f32 = theme::HEADER_H;
/// 自绘图形的回调：`(scene, 宽, 高, 字形色)`。
///
/// 提成别名是因为它出现在 `chip_custom` 的签名里——四参闭包直接写在参数位置
/// 会触发 clippy 的 `type_complexity`，可读性也差。
type GlyphFn = Rc<dyn Fn(&mut Scene, f32, f32, Color)>;

/// 面板内边距。
pub const PAD: f32 = theme::PAD;
/// 表头行高度。
pub const HEADER_ROW_H: f32 = 20.0;

/// 盘口 / 成交三列的列边界比例（与 [`col_header`] 的 `cells` 同一套）：
/// 价格左对齐、数量收在 60%、总额贴右。
///
/// 抽成常量是因为它过去在 `book.rs` / `trades.rs` 里各写一遍 `0.60`——
/// 两处漂移一格，列就对不齐表头。
pub const COLS_3: (f32, f32, f32) = (0.0, 0.60, 1.0);

/// 一个列单元格：文本 / 字号 / 颜色，**全部绘制期求值**。
///
/// 列边界语义与 [`col_header`] 完全一致：`at` 是右边界占内容宽的比例
/// （`0.0` = 左对齐贴 [`PAD`]），所以表头与数据行永远对齐。
pub struct ColCell {
    /// 右边界比例（0.0 = 左对齐）。
    pub at: f32,
    /// 文本（getter）。
    pub text: Rc<dyn Fn() -> String>,
    /// 字号。
    pub size: f32,
    /// 颜色（getter）。
    pub color: Rc<dyn Fn() -> Color>,
}

impl ColCell {
    /// 左对齐列（价格 / 符号）。
    pub fn left(
        text: impl Fn() -> String + 'static,
        size: f32,
        color: impl Fn() -> Color + 'static,
    ) -> Self {
        Self {
            at: 0.0,
            text: Rc::new(text),
            size,
            color: Rc::new(color),
        }
    }

    /// 右对齐到「内容宽 × `at`」的列（数量 / 总额 / 时间）。
    pub fn right(
        at: f32,
        text: impl Fn() -> String + 'static,
        size: f32,
        color: impl Fn() -> Color + 'static,
    ) -> Self {
        Self {
            at,
            text: Rc::new(text),
            size,
            color: Rc::new(color),
        }
    }
}

/// 按列比例画一行数据（[`theme::header_row`] 的行版），整行在 `h` 里垂直居中。
///
/// 这是所有列表行共用的列绘制：位置全部由 `w` 与 `at` 现算，调用方不写坐标。
pub fn cols_row(s: &mut Scene, w: f32, h: f32, cells: &[ColCell]) {
    for c in cells {
        let y = (h - c.size) / 2.0;
        if c.at <= 0.0 {
            theme::text(s, PAD, y, &(c.text)(), c.size, (c.color)());
        } else {
            let edge = (PAD + (w - 2.0 * PAD) * c.at).min(w - PAD);
            theme::text_right(s, edge, y, &(c.text)(), c.size, (c.color)());
        }
    }
}

/// 同 [`cols_row`]，但文字顶部 y 由调用方给（双行排版的第二行等）。
pub fn cols_row_at(s: &mut Scene, w: f32, y: f32, cells: &[ColCell]) {
    for c in cells {
        if c.at <= 0.0 {
            theme::text(s, PAD, y, &(c.text)(), c.size, (c.color)());
        } else {
            let edge = (PAD + (w - 2.0 * PAD) * c.at).min(w - PAD);
            theme::text_right(s, edge, y, &(c.text)(), c.size, (c.color)());
        }
    }
}

/// 布局属性的链式写法。
///
/// `rsx!` 标签里能直接写 `grow: sig.get()`，但**已经在函数里拼好的节点**
/// （如 `stacked(..)` 的返回值）再改宽度就得有个入口。用它就不必为了改一个
/// 宽度在外面多套一层 vbox——那层多余的 flex 正是以前 grow 行塌成 0 的根源。
pub trait LayoutAttr {
    /// 动态宽度（拖动分割条时跟着变）。
    fn with_width(self, w: Signal<f32>) -> Self;
}

impl LayoutAttr for Node {
    fn with_width(mut self, w: Signal<f32>) -> Self {
        self.width_fn = Some(Rc::new(move || w.get()));
        self
    }
}

/// 纯文本节点：字号/颜色绘制期求值，读信号自动追踪。
///
/// 这是整套组件里唯一"自己写绘制"的原语（下面的组件都在它之上组合），
/// 也是唯一需要 `measure` 的地方——文字宽度必须实测，否则父容器按 0 宽排布。
#[component]
pub fn text_label(
    /// 文本内容。用 `#[getter]` 标注：调用处传裸表达式（`"BUY"` / `model.x.get()`）
    /// 或闭包皆可，框架自动按帧重算——不必再写 `move || ...`。
    #[getter]
    content: String,
    /// 字号。不能叫 `size`：`size` 是通用布局属性（`(w, h)`），会撞车。
    font_size: f32,
    /// 文本颜色（getter：裸 `Color` 或闭包均可）。
    #[getter]
    color: Color,
) -> Node {
    let draw_content = Rc::clone(&content);
    let measure_content = Rc::clone(&content);
    rsx! {
        node {
            draw: move |s: &mut Scene| {
                let b = current_box();
                s.draw_text(
                    Point::new(0.0, (b.height - font_size) / 2.0),
                    &draw_content(),
                    font_size,
                    color(),
                );
            },
            // `measure` 收的是「求值时返回 (w, h) 的表达式」，宏会包成闭包
            measure: wy_render::text_measure::measure_text(&measure_content(), font_size),
        }
    }
    .into_node()
}

/// 面板外壳：圆角底 + 边框 + 标题栏，内容区自动占据剩余高度。
#[component]
pub fn panel(
    /// 面板标题（getter：裸字符串或闭包）。
    #[getter]
    title: String,
    /// 面板徽标（getter）。
    #[getter]
    badge: String,
    children: Vec<Slot>,
) -> Node {
    panel_with(title, badge, Vec::new(), children)
}

/// 面板（带标题栏右侧动作区，如缩放按钮、菜单）。
///
/// 标题、徽标、动作区是 **flex 里的兄弟节点**：`[标题(grow:1)] [徽标] [动作…]`。
/// 三者的左右位置由布局保证，不会出现"按钮压在徽标文字上"——早先的做法是
/// 在 `draw_fn` 里按 `w - PAD` 反算徽标位置，一加按钮就重叠。
///
/// 动作区是调用方给的 `Vec<Slot>`（数量不定），所以这一层用命令式拼装；
/// 里面每个按钮各自是 `rsx!` 组件，仍然不碰坐标。
pub fn panel_with(
    title: Rc<dyn Fn() -> String>,
    badge: Rc<dyn Fn() -> String>,
    actions: Vec<Slot>,
    children: Vec<Slot>,
) -> Node {
    let b_draw = Rc::clone(&badge);
    let b_hide = Rc::clone(&badge);

    // 标题占满中间剩余宽度；徽标内容为空时整体隐藏（`hide` 不占位）
    let title_cell = rsx! {
        text_label {
            content: title,
            font_size: 12.5,
            color: theme::TEXT,
            grow: 1.0,
        }
    };
    let badge_cell = rsx! {
        text_label {
            content: b_draw,
            font_size: 9.5,
            color: theme::MUTED,
            hide: b_hide().is_empty(),
        }
    };

    let mut header_items = vec![title_cell, badge_cell];
    header_items.extend(actions);

    let header = rsx! {
        hbox {
            gap: 6.0,
            height: HEADER_H,
            // 四向内边距：左右 12（对齐行文 PAD）、上下 0。
            // 旧写法 `padding: 12.0` 是四向同值——20 高的标题栏内区被压成
            // 0（20-24<0），文字靠 Center 的负偏移"碰巧"落进栏内；
            // 现在内区高就是 HEADER_H，文字真正居中。
            pad_x: PAD,
            pad_y: 0.0,
            align_items: AlignItem::Center,
            skip_draw: true,
            ..header_items
        }
    }
    .into_node();
    let content = rsx! {
        vbox {
            gap: 0.0,
            grow: 1.0,
            align_items: AlignItem::Stretch,
            ..children
        }
    }
    .into_node();

    rsx! {
        vbox {
            gap: 0.0,
            align_items: AlignItem::Stretch,
            // 只画底/边框/标题栏分隔线；标题与徽标由上面的兄弟节点自己画
            draw: |s: &mut Scene| {
                let b = current_box();
                theme::panel_bg(s, b.width, b.height);
            },
            // 标题栏与内容区在这里展开；`..` 把动态列表拼进容器
            ..vec![Slot::One(header), Slot::One(content)]
        }
    }
    .into_node()
}

/// 表头行：列标题 + 底部细线。
///
/// `cells` 里每项是 `(标题, 右边界比例)`；比例 `0.0` 表示左对齐。
/// 行组件与表头用同一套比例，列才对得齐。
#[component]
pub fn col_header(cells: Vec<(&'static str, f32)>) -> Node {
    rsx! {
        node {
            height: HEADER_ROW_H,
            draw: move |s: &mut Scene| {
                let b = current_box();
                theme::header_row(s, b.width, b.height, &cells);
            },
        }
    }
    .into_node()
}

/// 标准列表行：**定高** + 悬停/点击 + 调用方绘制。
///
/// 定高是硬约束：列表整体进滚动容器（见 [`wy_mve`] 的 `scroll`），行高若
/// 再随面板 `grow` 均分，矮面板会把一行压成几像素、高面板又把行拉出
/// 夸张行距——参考实现全部是定高行（wy-helper 自选 38 / 成交 20、
/// ooc 盘口 18），超出视口由滚动条兜底。
///
/// `body` 收 `(scene, 宽, 高)`：悬停底、深度条、列文字全在这里画，
/// 列文字用 [`cols_row`]（不手算 x）。
///
/// 普通函数而非 `#[component]`：四个参数里两个是 `impl Fn`，且 `body` 是
/// 几十行的绘制闭包——`rsx!` 标签属性装不下它（与 `tab_btn` / `chip_custom`
/// 同款的"命令式版"入口）。
pub fn list_row(
    // 行高（px）。
    h: f32,
    // 悬停进入/离开（写自己的信号即可）。
    on_hover: impl Fn(bool) + 'static,
    // 点击（不交互就传 `|| {}`）。
    on_click: impl Fn() + 'static,
    // 行内容（scene, 宽, 高）。
    body: impl Fn(&mut Scene, f32, f32) + 'static,
    // 语义标签：测试按它认行（`book-row` / `watch-row` / `trade-row`）。
    tag: &'static str,
) -> Node {
    rsx! {
        node {
            tag: tag,
            height: h,
            on_hover: on_hover,
            on_click: on_click,
            draw: move |s: &mut Scene| {
                let b = current_box();
                if b.height <= 0.0 {
                    return;
                }
                body(s, b.width, b.height);
            },
        }
    }
    .into_node()
}

/// 弹性空白：吃掉主轴剩余空间（对应 ooc 的 `flex-1` 占位）。
#[component]
pub fn flex_spacer() -> Node {
    rsx! {
        node {
            grow: 1.0,
            skip_draw: true,
        }
    }
    .into_node()
}

/// 竖线分隔（顶栏分组之间用）。
#[component]
pub fn vsep() -> Node {
    rsx! {
        node {
            width: 1.0,
            height: 16.0,
            draw: |s: &mut Scene| {
                let b = current_box();
                s.fill_rect(Rect::new(0.0, 0.0, 1.0, b.height), theme::BORDER);
            },
        }
    }
    .into_node()
}

/// 标签按钮：选中态 / 悬停态自己画，点击回调由调用方给。
///
/// 宽度跟着文案实测走（切标的时不会被裁字，也不留一大截空白）——
/// `width:` 直接写成表达式，宏会包成布局期求值的闭包。
#[component]
pub fn tab(label: &'static str, active: impl Fn() -> bool, on_pick: impl Fn()) -> Node {
    // 组件体里拿到的是 `Rc<dyn Fn>`：能直接调用，但不满足 `impl Fn` 约束，
    // 所以这里包一层再往 `tab_btn` 递。
    tab_btn(label, move || active(), move || on_pick())
}

/// 命令式版 [`tab`]：需要在 `rsx!` 之外拼一组 tab（如顶栏循环生成的标的/周期
/// 按钮）时用。`#[component]` 会把函数签名改写成 `fn(Props)`，所以循环里
/// 没法直接位置调用。
pub fn tab_btn(
    label: &'static str,
    active: impl Fn() -> bool + 'static,
    on_pick: impl Fn() + 'static,
) -> Node {
    tab_of(
        Rc::new(move || String::from(label)),
        Rc::new(active),
        Rc::new(on_pick),
    )
}

/// 同 [`tab`]，但标题在绘制期求值（标的可能来自自选，不在固定 tab 里）。
pub fn tab_of(
    label: Rc<dyn Fn() -> String>,
    active: Rc<dyn Fn() -> bool>,
    on_pick: Rc<dyn Fn()>,
) -> Node {
    const H: f32 = 21.0;
    let hover = Signal::new(false);

    let w_click = Rc::clone(&label);
    let w_layout = Rc::clone(&label);
    let draw_label = Rc::clone(&label);

    rsx! {
        node {
            width: width_of(&w_layout(), 10.0),
            height: H,
            on_hover: move |on| hover.set(on),
            on_click: move || on_pick(),
            draw: move |s: &mut Scene| {
                let w = width_of(&w_click(), 10.0);
                let on = active();
                if on {
                    s.fill_round_rect(Rect::new(0.0, 0.0, w, H), 4.0, theme::ACCENT_SOFT);
                    s.stroke_round_rect(
                        Rect::new(0.5, 0.5, w - 1.0, H - 1.0),
                        4.0,
                        theme::ACCENT,
                        1.0,
                    );
                } else if hover.get() {
                    s.fill_round_rect(Rect::new(0.0, 0.0, w, H), 4.0, theme::ROW_ALT);
                }
                theme::text_center(
                    s,
                    w / 2.0,
                    (H - 10.0) / 2.0,
                    &draw_label(),
                    10.0,
                    if on { theme::TEXT } else { theme::MUTED },
                );
            },
        }
    }
    .into_node()
}

/// tab 的最小宽度：文案实测 + 左右各 8px 内边距，下限 28px。
fn width_of(text: &str, size: f32) -> f32 {
    (theme::width(text, size) + 16.0).max(28.0)
}

/// 方块按钮的共用外观：`(底色, 描边色)`。
///
/// 选中 / 悬停 / 常态三档只有一处定义——面板标题栏上的小按钮
/// （图表缩放、盘口模式图标、更多菜单）全走这里，才不会出现
/// "有的带框、有的不带框"这种参差。
fn chip_bg(hot: bool, active: bool) -> (Color, Color) {
    if active {
        (theme::ACCENT_SOFT, theme::ACCENT)
    } else if hot {
        (theme::ROW_ACTIVE, theme::HANDLE)
    } else {
        (theme::ROW_ALT, theme::BORDER)
    }
}

/// 方块按钮：画字形，外观与状态由 [`make_chip`] 统一给出。
#[component]
pub fn chip(
    label: &'static str,
    w: f32,
    #[props(default = theme::MUTED)] base: Color,
    on_click: impl Fn(),
) -> Node {
    make_chip(
        label,
        w,
        base,
        Rc::new(|| false),
        Rc::new(move || on_click()),
    )
}

/// 同 [`chip`]，但带选中态（图表缩放档位、盘口模式）。
#[component]
pub fn chip_on(
    label: &'static str,
    w: f32,
    #[props(default = theme::MUTED)] base: Color,
    active: impl Fn() -> bool,
    on_click: impl Fn(),
) -> Node {
    make_chip(
        label,
        w,
        base,
        Rc::new(move || active()),
        Rc::new(move || on_click()),
    )
}

/// 文字字形的小方块按钮。
fn make_chip(
    label: &'static str,
    w: f32,
    base: Color,
    active: Rc<dyn Fn() -> bool>,
    on_click: Rc<dyn Fn()>,
) -> Node {
    chip_custom(
        w,
        base,
        active,
        on_click,
        Rc::new(move |s, w, h, color| {
            theme::text_center(s, w / 2.0, (h - 9.5) / 2.0, label, 9.5, color);
        }),
    )
}

/// 方块按钮（字形自绘）：字符画不出来的图标（三点菜单、加减号）用它。
///
/// `base` 是常态字形色；悬停与选中时统一提到 `TEXT`，
/// 保证"这里能点"在两个通道（底色变亮 + 字形变亮）上都读得出来。
pub fn chip_custom(
    w: f32,
    base: Color,
    active: Rc<dyn Fn() -> bool>,
    on_click: Rc<dyn Fn()>,
    glyph: GlyphFn,
) -> Node {
    const H: f32 = 20.0;
    let hover = Signal::new(false);
    rsx! {
        node {
            width: w,
            height: H,
            on_hover: move |on| hover.set(on),
            on_click: move || on_click(),
            draw: move |s: &mut Scene| {
                let on = active();
                let hot = hover.get();
                let (bg, edge) = chip_bg(hot, on);
                s.fill_round_rect(Rect::new(0.0, 0.0, w, H), 4.0, bg);
                s.stroke_round_rect(
                    Rect::new(0.5, 0.5, w - 1.0, H - 1.0),
                    4.0,
                    edge,
                    1.0,
                );
                glyph(s, w, H, if hot || on { theme::TEXT } else { base });
            },
        }
    }
    .into_node()
}

/// 小方按钮（图表缩放、面板动作区用）：悬停高亮，点击回调。
#[component]
pub fn icon_btn(label: &'static str, w: f32, on_click: impl Fn()) -> Node {
    make_chip(
        label,
        w,
        theme::MUTED,
        Rc::new(|| false),
        Rc::new(move || on_click()),
    )
}

/// 更多操作按钮（自绘三点），可带选中态（菜单展开时）。
///
/// 不用 `⋯` 字符：字体里没有这个码位，Windows 上会掉成豆腐块 □。
#[component]
pub fn dots_chip(w: f32, active: impl Fn() -> bool, on_click: impl Fn()) -> Node {
    chip_custom(
        w,
        theme::MUTED,
        Rc::new(move || active()),
        Rc::new(move || on_click()),
        Rc::new(|s, w, h, color| {
            let (cx, cy) = (w / 2.0, h / 2.0);
            for k in -1..=1 {
                s.fill_round_rect(
                    Rect::new(cx + k as f32 * 4.5 - 1.0, cy - 1.0, 2.0, 2.0),
                    1.0,
                    color,
                );
            }
        }),
    )
}

/// 小开关（盘口设置条用）：选中态描边高亮。
#[component]
pub fn toggle_chip(label: &'static str, active: impl Fn() -> bool, on_click: impl Fn()) -> Node {
    const H: f32 = 17.0;
    let w = (theme::width(label, 9.0) + 14.0).max(34.0);
    let hover = Signal::new(false);
    rsx! {
        node {
            width: w,
            height: H,
            on_hover: move |on| hover.set(on),
            on_click: move || on_click(),
            draw: move |s: &mut Scene| {
                let on = active();
                let bg = if on {
                    theme::ACCENT_SOFT
                } else if hover.get() {
                    theme::ROW_ACTIVE
                } else {
                    theme::ROW_ALT
                };
                s.fill_round_rect(Rect::new(0.0, 0.0, w, H), 3.0, bg);
                s.stroke_round_rect(
                    Rect::new(0.5, 0.5, w - 1.0, H - 1.0),
                    3.0,
                    if on { theme::ACCENT } else { theme::BORDER },
                    1.0,
                );
                theme::text_center(
                    s,
                    w / 2.0,
                    (H - 9.0) / 2.0,
                    label,
                    9.0,
                    if on { theme::TEXT } else { theme::FAINT },
                );
            },
        }
    }
    .into_node()
}

/// 一行"标签 —— 值"，值靠右。宽度由父容器 Stretch 给。
///
/// 行高**定值**（默认 26）：统计行进滚动容器，不再随面板 `grow` 均分——
/// 均分会把矮面板里的行压成几像素、高面板里的行拉出夸张行距。
/// 行够高时补一条极浅的分隔线，让"一列读数"看起来是表格而不是散落的文字。
#[component]
pub fn kv_row(
    /// 左侧标签（getter：裸字符串或闭包）。
    #[getter]
    label: String,
    /// 右侧数值（getter）。
    #[getter]
    value: String,
    /// 数值色调（getter）。
    #[getter]
    tone: Color,
    /// 行高（px）。
    #[props(default = 26.0f32)]
    h: f32,
) -> Node {
    rsx! {
        node {
            height: h,
            draw: move |s: &mut Scene| {
                let b = current_box();
                if b.height <= 0.0 {
                    return;
                }
                theme::text(s, PAD, (b.height - 9.5) / 2.0, &label(), 9.5, theme::FAINT);
                theme::text_right(
                    s,
                    b.width - PAD,
                    (b.height - 10.5) / 2.0,
                    &value(),
                    10.5,
                    tone(),
                );
                if b.height >= 22.0 {
                    s.fill_rect(
                        Rect::new(PAD, b.height - 1.0, (b.width - 2.0 * PAD).max(0.0), 1.0),
                        theme::GRID,
                    );
                }
            },
        }
    }
    .into_node()
}
