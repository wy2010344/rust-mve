//! 数据层（对标 TS `canvas-render2/todo-store.ts`）。
//!
//! 五个状态 + 一组变更入口：
//!
//! | TS | 本文件 |
//! |---|---|
//! | `todos` / `filter` / `theme` 三个 `createSignal` | 同名线程局部 [`Signal`] |
//! | `past` / `future` 快照栈 | 同一组（变更前压栈、重做栈清空） |
//! | `commit(mutate)` | [`commit`] |
//! | `addTodo` / `toggleTodo` / `removeTodo` / `clearDone` | 同名函数 |
//! | `undoTodo` / `redoTodo` / `canUndo` / `canRedo` | 同名函数 |
//! | `stats()` / `visibleTodos()` | 同名函数 |
//! | `palette()` / `applyTheme()` / `toggleTheme()` | 同名函数 |
//!
//! # 为什么用线程局部而不是模块级 `let`
//!
//! TS 的模块级 `const todos = createSignal([])` 在 Rust 里对应 `static`，但
//! `Signal` 不是 `Sync`（UI 单线程），所以落点是 `thread_local! + RefCell`——与
//! [`text_defaults`](wy_mve::canvas_render2::text_defaults) 用的是同一个形状。
//!
//! # 撤销/重做为什么是快照式
//!
//! 引擎层的 [`UndoRedo`](wy_mve::canvas_render2::UndoRedo) 是**动作式**的
//! （`TextEditAction`，服务编辑器键盘编辑）。列表数据结构与文本动作不匹配，
//! TS 那边就用同语义的快照栈演示，这里照办——不是"简化"，而是那个组件管不到
//! 列表。

use std::cell::{Cell, RefCell};

use wy_mve::canvas_render2::{set_placeholder_color, set_text_color};
use wy_render::Color;
use wy_signal::{GetValue, SetValue, Signal};

/// 一条待办。
#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    /// 稳定 id（列表 keyed 复用/销毁靠它）。
    pub id: u64,
    /// 正文。
    pub text: String,
    /// 是否完成。
    pub done: bool,
    /// 可选链接：条目内渲染链接片段，点击经 `UrlOpener` 打开
    /// （演示 [`RichTextSpan::url`](wy_mve::canvas_render2::RichTextSpan)）。
    pub link: Option<String>,
}

/// 过滤条件（对标 TS `TodoFilter`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodoFilter {
    /// 全部。
    All,
    /// 未完成。
    Active,
    /// 已完成。
    Done,
}

thread_local! {
    /// 全部待办（信号驱动：变化时列表区域重算、绘制自动更新）。
    static TODOS: RefCell<Signal<Vec<Todo>>> = RefCell::new(Signal::new(Vec::new()));
    /// 当前过滤条件。
    static FILTER: RefCell<Signal<TodoFilter>> = RefCell::new(Signal::new(TodoFilter::All));
    /// 当前主题。
    static THEME: RefCell<Signal<Theme>> = RefCell::new(Signal::new(Theme::Light));
    /// 撤销栈（变更前的快照）。
    static PAST: RefCell<Vec<Vec<Todo>>> = const { RefCell::new(Vec::new()) };
    /// 重做栈（撤销过的快照）。
    static FUTURE: RefCell<Vec<Vec<Todo>>> = const { RefCell::new(Vec::new()) };
    /// 下一个 id。
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    /// 示例文案取样器的状态（替代 `Math.random`，保证出图可复现）。
    static SAMPLE_CURSOR: Cell<u64> = const { Cell::new(0) };
}

/// 主题（对标 TS `Theme`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// 浅色。
    Light,
    /// 深色。
    Dark,
}

// ============================================================================
// 读取
// ============================================================================

/// 全部待办（读信号 → 参与响应式派生）。
#[must_use]
pub fn todos() -> Vec<Todo> {
    TODOS.with(|s| s.borrow().get())
}

/// 当前过滤条件。
#[must_use]
pub fn filter() -> TodoFilter {
    FILTER.with(|s| s.borrow().get())
}

/// 当前主题。
#[must_use]
pub fn theme() -> Theme {
    THEME.with(|s| s.borrow().get())
}

/// 过滤后的可见待办（`todo-list` 直接消费）。
#[must_use]
pub fn visible_todos() -> Vec<Todo> {
    let f = filter();
    let list = todos();
    match f {
        TodoFilter::Active => list.into_iter().filter(|t| !t.done).collect(),
        TodoFilter::Done => list.into_iter().filter(|t| t.done).collect(),
        TodoFilter::All => list,
    }
}

/// 统计：总数与完成数。
#[must_use]
pub fn stats() -> (usize, usize) {
    let list = todos();
    let done = list.iter().filter(|t| t.done).count();
    (list.len(), done)
}

/// 是否可撤销。
#[must_use]
pub fn can_undo() -> bool {
    PAST.with(|p| !p.borrow().is_empty())
}

/// 是否可重做。
#[must_use]
pub fn can_redo() -> bool {
    FUTURE.with(|f| !f.borrow().is_empty())
}

// ============================================================================
// 变更入口（快照式撤销/重做）
// ============================================================================

/// 统一变更入口：变更前快照入撤销栈、清空重做栈。
fn commit(mutate: impl FnOnce(Vec<Todo>) -> Vec<Todo>) {
    let before = todos();
    PAST.with(|p| p.borrow_mut().push(before.clone()));
    FUTURE.with(|f| f.borrow_mut().clear());
    set_todos(mutate(before));
}

fn set_todos(list: Vec<Todo>) {
    TODOS.with(|s| s.borrow().set(list));
}

/// 切换过滤条件。
pub fn set_filter(f: TodoFilter) {
    FILTER.with(|s| s.borrow().set(f));
}

/// 追加一条待办。
pub fn add_todo(text: impl Into<String>, link: Option<String>) {
    let id = NEXT_ID.with(|c| {
        let v = c.get();
        c.set(v + 1);
        v
    });
    let text = text.into();
    commit(move |mut list| {
        list.push(Todo {
            id,
            text,
            done: false,
            link,
        });
        list
    });
}

/// 切换完成状态。
pub fn toggle_todo(id: u64) {
    commit(move |list| {
        list.into_iter()
            .map(|mut t| {
                if t.id == id {
                    t.done = !t.done;
                }
                t
            })
            .collect()
    });
}

/// 移除一条。
pub fn remove_todo(id: u64) {
    commit(move |list| list.into_iter().filter(|t| t.id != id).collect());
}

/// 清除已完成。
pub fn clear_done() {
    commit(|list| list.into_iter().filter(|t| !t.done).collect());
}

/// 撤销一步。
pub fn undo_todo() {
    let Some(prev) = PAST.with(|p| p.borrow_mut().pop()) else {
        return;
    };
    let cur = todos();
    FUTURE.with(|f| f.borrow_mut().insert(0, cur));
    set_todos(prev);
}

/// 重做一步。
pub fn redo_todo() {
    let next = FUTURE.with(|f| {
        let mut f = f.borrow_mut();
        if f.is_empty() {
            None
        } else {
            Some(f.remove(0))
        }
    });
    let Some(next) = next else { return };
    let cur = todos();
    PAST.with(|p| p.borrow_mut().push(cur));
    set_todos(next);
}

/// 示例文案（TS `SAMPLES`）。
const SAMPLES: [&str; 10] = [
    "写本周周报",
    "买牛奶和鸡蛋",
    "健身 30 分钟",
    "回复评审邮件",
    " review 渲染管线 PR",
    "读 10 页书",
    "整理桌面文件",
    "预约牙医",
    "给妈妈打电话",
    "修复滚动条拖拽",
];

/// 追加一条示例待办。
///
/// TS 用 `Math.random()` 取文案；这里改成一个固定种子的 LCG——**出图可复现**
/// 比"真的随机"更重要（否则快照每次都不一样，没法比像素）。
pub fn add_sample_todo() {
    let idx = SAMPLE_CURSOR.with(|c| {
        // 数值来自 Numerical Recipes 的常见常数，够打散即可。
        let next = c.get().wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        c.set(next);
        ((next >> 16) as usize) % SAMPLES.len()
    });
    add_todo(SAMPLES[idx], None);
}

/// 种子数据：12 条（超出列表视口，便于演示滚动）。
///
/// 混入链接待办 / Tab 文本 / 迁移笔记，演示 `RichTextSpan.url` 与 TabRender。
pub fn seed() {
    let mut list = Vec::new();
    for i in 0..12usize {
        let id = NEXT_ID.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });
        let text = if i == 4 {
            "Tab 文本:\t制表位展开为 4 空格".to_string()
        } else if i == 7 {
            "查看 WorkBuddy 文档".to_string()
        } else {
            SAMPLES[i % SAMPLES.len()].to_string()
        };
        list.push(Todo {
            id,
            text,
            done: i % 4 == 0,
            link: (i == 7).then(|| "https://www.workbuddy.cn/docs/workbuddy/Overview".to_string()),
        });
    }
    // 种子数据不压撤销栈（TS 同样是直接 `todos.set(...)`）。
    set_todos(list);
}

// ============================================================================
// 主题（演示 TextDefaults 的 setter 回写）
// ============================================================================

/// 主题配色（对标 TS `Palette`）。
///
/// ⚠️ 端口陷阱：TS 的字面量是 `0xAARRGGBB`，Rust 的 [`Color::from_u32`] 收
/// **`0xRRGGBBAA`**。下面全部按 RGBA 重排过，末两位都是 `FF`（不透明）。
///
/// 有几个字段本示例暂时用不到（`track` / `input_*` / `selection` / `accent` 是
/// 编辑器面板与占位式滚动条用的）——**照抄整张调色板**而不是删到只剩用到的，
/// 免得后面移植 `todo-editor.ts` 时又得回头去 TS 里抄一遍。
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Palette {
    /// 页面底色。
    pub page: Color,
    /// 卡片底色。
    pub card: Color,
    /// 已完成条目的卡片底色。
    pub card_done: Color,
    /// 描边。
    pub border: Color,
    /// 滚动条轨道（浮层滚动条不用，留作占位式备用）。
    pub track: Color,
    /// 滑块。
    pub thumb: Color,
    /// 复选框描边。
    pub checkbox_border: Color,
    /// 链接。
    pub link: Color,
    /// 按钮底色。
    pub btn_bg: Color,
    /// 按钮文字。
    pub btn_text: Color,
    /// 激活态按钮底色。
    pub btn_active_bg: Color,
    /// 激活态按钮文字。
    pub btn_active_text: Color,
    /// 禁用态按钮底色。
    pub btn_disabled_bg: Color,
    /// 禁用态按钮文字。
    pub btn_disabled_text: Color,
    /// 输入框底色。
    pub input_bg: Color,
    /// 输入框描边（聚焦时改用 accent）。
    pub input_border: Color,
    /// 编辑器选区高亮。
    pub selection: Color,
    /// 聚焦强调色（光标 / 聚焦边框）。
    pub accent: Color,
}

/// 浅色配色（TS `LIGHT`）。
pub const LIGHT: Palette = Palette {
    page: Color::from_u32(0xF8FAFCFF),
    card: Color::from_u32(0xFFFFFFFF),
    card_done: Color::from_u32(0xF1F5F9FF),
    border: Color::from_u32(0xE2E8F0FF),
    track: Color::from_u32(0xF1F5F9FF),
    thumb: Color::from_u32(0xCBD5E1FF),
    checkbox_border: Color::from_u32(0x94A3B8FF),
    link: Color::from_u32(0x2563EBFF),
    btn_bg: Color::from_u32(0xF1F5F9FF),
    btn_text: Color::from_u32(0x475569FF),
    btn_active_bg: Color::from_u32(0xDBEAFEFF),
    btn_active_text: Color::from_u32(0x1D4ED8FF),
    btn_disabled_bg: Color::from_u32(0xF8FAFCFF),
    btn_disabled_text: Color::from_u32(0xCBD5E1FF),
    input_bg: Color::from_u32(0xF8FAFCFF),
    input_border: Color::from_u32(0xCBD5E1FF),
    selection: Color::rgba(59, 130, 246, 90),
    accent: Color::from_u32(0x2563EBFF),
};

/// 深色配色（TS `DARK`）。
pub const DARK: Palette = Palette {
    page: Color::from_u32(0x0F172AFF),
    card: Color::from_u32(0x1E293BFF),
    card_done: Color::from_u32(0x172033FF),
    border: Color::from_u32(0x334155FF),
    track: Color::from_u32(0x0F172AFF),
    thumb: Color::from_u32(0x475569FF),
    checkbox_border: Color::from_u32(0x64748BFF),
    link: Color::from_u32(0x60A5FAFF),
    btn_bg: Color::from_u32(0x334155FF),
    btn_text: Color::from_u32(0xCBD5E1FF),
    btn_active_bg: Color::from_u32(0x1E3A8AFF),
    btn_active_text: Color::from_u32(0x93C5FDFF),
    btn_disabled_bg: Color::from_u32(0x1E293BFF),
    btn_disabled_text: Color::from_u32(0x475569FF),
    input_bg: Color::from_u32(0x0F172AFF),
    input_border: Color::from_u32(0x475569FF),
    selection: Color::rgba(96, 165, 250, 90),
    accent: Color::from_u32(0x60A5FAFF),
};

/// 当前配色（随主题信号）。
#[must_use]
pub fn palette() -> Palette {
    if theme() == Theme::Dark {
        DARK
    } else {
        LIGHT
    }
}

/// 应用主题：经 `TextDefaults` 的 setter 回写核心中性默认色。
///
/// 对标 Kotlin：组件包切主题时同步引擎默认色，昼/夜自动换肤。默认文本色随
/// 信号失效，读取方（`wrapped_text_node` 的默认 `color`）重绘即更新。
pub fn apply_theme() {
    let dark = theme() == Theme::Dark;
    set_text_color(if dark {
        Color::rgb(226, 232, 240)
    } else {
        Color::rgb(30, 41, 59)
    });
    set_placeholder_color(if dark {
        Color::rgb(100, 116, 139)
    } else {
        Color::rgb(148, 163, 184)
    });
}

/// 切换主题并回写默认色。
pub fn toggle_theme() {
    let next = if theme() == Theme::Dark {
        Theme::Light
    } else {
        Theme::Dark
    };
    THEME.with(|s| s.borrow().set(next));
    apply_theme();
}
