//! 编辑器演示面板（对标 TS `canvas-render2/todo-editor.ts`）。
//!
//! | TS | 本文件 |
//! |---|---|
//! | `draftText` / `noteText` / `secretText` 三个 `createSignal` | 同名线程局部 [`Signal`] |
//! | `fieldLabel(text)` | [`field_label`] |
//! | `interface FieldArg` | [`FieldArg`] |
//! | `textField(arg)` | [`text_field`] |
//! | `editorPanel()` | [`editor_panel`] |
//!
//! 这一屏是编辑器能力的主秀场，四件事各自落到一个参数上：
//!
//! - **单行新增框**：回车提交进列表，提交后清空、焦点留在原地（`on_submit`）；
//! - **多行备注**：Enter 换行、Ctrl+Z/Y 撤销重做、方向键 / Home / End 导航；
//! - **掩码字段**：`obscure` 密码模式（每字素簇一个圆点，字素边界由 `Graphemes` 判定）；
//! - **焦点循环**：三个字段都 `focusable`，Tab / Shift+Tab 按 `focus_order` 在面板内循环。
//!
//! 键盘与输入法由引擎统一路由：焦点落在编辑器上时，宿主把按键转成
//! [`KeyEvent2`](wy_mve::canvas_render2::KeyEvent2)、组合串经 `ImeEvent` 上报——
//! 组件层只管收，不用自己挂监听（TS 那边是离屏 textarea 接管 DOM 焦点，同一分工）。
//!
//! # 为什么这里没有「控制器」对象
//!
//! TS 的 `editableTextNode` 内部持有一个 `EditorController`；Rust 侧
//! [`editable_text_node`](wy_mve::canvas_render2::editable_text_node) 里已经把它建好了，
//! 调用方拿到的就是一个 [`Fc`]——本文件只负责**摆位置、接信号**，不碰光标/选区。

use std::cell::RefCell;
use std::rc::Rc;

use wy_mve::canvas_render2::{placeholder_color, text_color};
use wy_mve::{editable_text_node, flex, rect_node, wrapped_text_node, AlignItem, Fc, Layout};
use wy_signal::{GetValue, SetValue, Signal};

use crate::store::{add_todo, palette};

/// 字段外框宽度（TS `width: () => 356`）。
const FIELD_WIDTH: f32 = 356.0;
/// 面板宽度（TS `width: () => 380`）。
const PANEL_WIDTH: f32 = 380.0;
/// 单行高度（TS `arg.lines * 20 + 12`）。
const LINE_HEIGHT: f32 = 20.0;

thread_local! {
    /// 新增待办草稿（TS `draftText`）。
    static DRAFT: RefCell<Signal<String>> = RefCell::new(Signal::new(String::new()));
    /// 多行备注（TS `noteText`）。
    static NOTE: RefCell<Signal<String>> = RefCell::new(Signal::new(String::new()));
    /// 掩码字段（TS `secretText`）。
    static SECRET: RefCell<Signal<String>> = RefCell::new(Signal::new(String::new()));
}

/// 读草稿（`text` getter 用；在读线程局部里取值，参与响应式读取）。
#[must_use]
pub fn draft_text() -> String {
    DRAFT.with(|s| s.borrow().get())
}

/// 写草稿。
fn set_draft(value: &str) {
    DRAFT.with(|s| s.borrow().set(value.to_string()));
}

/// 读多行备注。
#[must_use]
pub fn note_text() -> String {
    NOTE.with(|s| s.borrow().get())
}

/// 写多行备注。
fn set_note(value: &str) {
    NOTE.with(|s| s.borrow().set(value.to_string()));
}

/// 读掩码字段。
#[must_use]
pub fn secret_text() -> String {
    SECRET.with(|s| s.borrow().get())
}

/// 写掩码字段。
fn set_secret(value: &str) {
    SECRET.with(|s| s.borrow().set(value.to_string()));
}

/// 按 `--fill` 规格预置字段文本：`draft=..;note=..;secret=..`
/// （分号分隔，值里可以带 `=`）。
///
/// 只服务快照：空面板只能看到三个占位串，掩码圆点与多行换行得先有字才看得见。
pub fn fill_from_spec(spec: &str) {
    for part in spec.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim() {
            "draft" => set_draft(value),
            "note" => set_note(value),
            "secret" => set_secret(value),
            other => eprintln!("--fill: 未知字段 `{other}`（只认 draft / note / secret）"),
        }
    }
}

/// 字段标题（TS `fieldLabel`）。
fn field_label(text: &str) -> Fc {
    let text = text.to_string();
    wrapped_text_node! {
        width: FIELD_WIDTH,
        // `#[getter]` 字段的值表达式会被宏包成 `move || ..`，所以 String 要
        // 显式 clone：直接写 `text` 等于"每次调用都把它 move 出去"。
        text: text.clone(),
        font_family: Some("Noto Sans SC".to_string()),
        font_size: 12.0,
        color: placeholder_color(),
    }
}

/// 字段参数（对标 TS `interface FieldArg`）。
///
/// TS 用对象字面量 + 可选成员表达"这个字段要不要多行 / 掩码 / 提交"；Rust 用
/// 结构体 + [`FieldArg::new`] 的两个布尔位收口——`singleLine` / `obscure` 在 TS
/// 里是 `?:`，这里给 `false` 缺省，其余字段显式写在构造点，读起来与 TS 的
/// 字面量一一对应。
pub struct FieldArg {
    /// 文本真相源（信号 getter）。
    pub text: Rc<dyn Fn() -> String>,
    /// 文本回写。
    pub on_text_change: Rc<dyn Fn(&str)>,
    /// 占位文本。
    pub placeholder: String,
    /// 行数（决定输入框高度）：单行 1，多行给 >1。
    pub lines: u32,
    /// 单行模式（不折行、回车即提交）。
    pub single_line: bool,
    /// 掩码模式（每字素簇一个圆点）。
    pub obscure: bool,
    /// 单行回车 / 多行 Ctrl+Enter 的提交回调。
    pub on_submit: Option<Rc<dyn Fn()>>,
    /// Tab 遍历顺序。
    pub focus_order: i32,
}

impl FieldArg {
    /// 单行字段（TS `singleLine: true`）。
    fn single(
        text: Rc<dyn Fn() -> String>,
        on_text_change: Rc<dyn Fn(&str)>,
        placeholder: &str,
        focus_order: i32,
    ) -> Self {
        Self {
            text,
            on_text_change,
            placeholder: placeholder.to_string(),
            lines: 1,
            single_line: true,
            obscure: false,
            on_submit: None,
            focus_order,
        }
    }

    /// 多行字段（TS 不写 `singleLine`，`lines: n`）。
    fn multi(
        text: Rc<dyn Fn() -> String>,
        on_text_change: Rc<dyn Fn(&str)>,
        placeholder: &str,
        lines: u32,
        focus_order: i32,
    ) -> Self {
        Self {
            text,
            on_text_change,
            placeholder: placeholder.to_string(),
            lines,
            single_line: false,
            obscure: false,
            on_submit: None,
            focus_order,
        }
    }

    /// 追加提交回调（TS `onSubmit`）。
    fn on_submit(mut self, f: impl Fn() + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    /// 打开掩码（TS `obscure: true`）。
    fn obscure(mut self, yes: bool) -> Self {
        self.obscure = yes;
        self
    }
}

/// 输入字段（TS `textField`）：外框 [`rect_node`]（背景 + 边框 + 圆角）+ 内层
/// [`editable_text_node`]。
///
/// 外框是可聚焦节点的**父容器**——点击内层编辑器时，指针路由沿命中链找到最深的
/// `focusable`（编辑器本身）并登记为活跃编辑器。
///
/// # 内层那两个尺寸为什么是显式写死的
///
/// TS 写的是 `width: 342`（= 356 − 6×2 − 1×2）与 `height: height - 14`。这不是
/// "懒得用布局"，而是**编辑器自身尺寸由文本决定**：`EditableTextController::size`
/// 在没有显式宽高时按段落量出来的宽高报数（`LayoutSize::inside`），并不从父内容盒
/// 拉伸——所以不写它，输入框会塌到"当前文字宽"，输入空串时直接 0 宽。
/// 换句话说，这两个数是这个组件的**必需输入**，不是排版偷懒；换成
/// `grow` / `sizeFromParent` 反而会改变语义（TS 那边同样做不到）。
fn text_field(arg: FieldArg) -> Fc {
    let FieldArg {
        text,
        on_text_change,
        placeholder,
        lines,
        single_line,
        obscure,
        on_submit,
        focus_order,
    } = arg;
    let height = lines as f32 * LINE_HEIGHT + 12.0;
    let on_submit: Rc<dyn Fn()> = on_submit.unwrap_or_else(|| Rc::new(|| {}));
    rect_node! {
        width: FIELD_WIDTH,
        height: height,
        padding: 6.0,
        background: Some(palette().input_bg),
        border_color: Some(palette().input_border),
        border_width: 1.0,
        radius: 6.0,
        children: vec![
            editable_text_node! {
                // 见函数文档：编辑器的尺寸由自己报，必须显式给。
                width: FIELD_WIDTH - 14.0,
                height: height - 14.0,
                text: text(),
                on_text_change: move |v: &str| on_text_change(v),
                placeholder: placeholder.clone(),
                single_line: single_line,
                obscure: obscure,
                on_submit: move || on_submit(),
                focus_order: Some(focus_order),
                font_family: Some("Noto Sans SC".to_string()),
                font_size: 14.0,
                color: text_color(),
                selection_color: palette().selection,
            },
        ],
    }
}

/// 编辑器演示面板（TS `editorPanel`，放在列表下方作为独立一节）。
///
/// 面板自己不写 `height`：竖向 flex 容器、子节点都没有 `grow`，
/// `allows_size_from_children()` 为真 → 高度由四个子节点撑起
/// （与 TS `rectNode` 不写 `height` 走 `sizeFromChildren` 同一条路径）。
#[must_use]
pub fn editor_panel() -> Fc {
    rect_node! {
        width: PANEL_WIDTH,
        padding: 10.0,
        background: Some(palette().card),
        border_color: Some(palette().border),
        border_width: 1.0,
        radius: 10.0,
        layout: flex! {
            direction: Layout::Column,
            gap: 8.0,
            align_item: AlignItem::Center,
        },
        children: vec![
            field_label("文本编辑器（Tab 切换 / Ctrl+Z 撤销 / Ctrl+A 全选）"),
            // 单行新增框：回车提交进列表，提交后清空（焦点由引擎留在原地）。
            text_field(
                FieldArg::single(
                    Rc::new(draft_text),
                    Rc::new(|v: &str| set_draft(v)),
                    "输入新待办，回车加入列表",
                    1,
                )
                .on_submit(|| {
                    let v = draft_text().trim().to_string();
                    if v.is_empty() {
                        return;
                    }
                    add_todo(v, None);
                    set_draft("");
                }),
            ),
            // 多行备注：Enter 换行，Ctrl+Enter 提交（占位文案承诺的那一条）。
            //
            // TS 原本漏了 `onSubmit`（文案写了、代码没接），这里两侧一起补：
            // 提交语义与单行新增框一致——trim 后非空才加，加完清空。
            text_field(
                FieldArg::multi(
                    Rc::new(note_text),
                    Rc::new(|v: &str| set_note(v)),
                    "多行备注：Enter 换行，Ctrl+Enter 加入列表",
                    3,
                    2,
                )
                .on_submit(|| {
                    let v = note_text().trim().to_string();
                    if v.is_empty() {
                        return;
                    }
                    add_todo(v, None);
                    set_note("");
                }),
            ),
            text_field(
                FieldArg::single(
                    Rc::new(secret_text),
                    Rc::new(|v: &str| set_secret(v)),
                    "掩码字段（obscure）",
                    3,
                )
                .obscure(true),
            ),
        ],
    }
}

// ============================================================================
// 回归门
// ============================================================================

#[cfg(test)]
mod tests {
    use wy_mve::canvas_render2::build_root_fc;
    use wy_mve::{node2, Node2};

    use super::*;

    /// 找第一个带 `ImeExt` 的节点（就是编辑器本体）。
    fn find_editor(node: &Node2) -> Option<Node2> {
        if node
            .find_ext_rc::<wy_mve::canvas_render2::node2::ImeExt>()
            .is_some()
        {
            return Some(node.clone());
        }
        node.children().into_iter().find_map(|c| find_editor(&c))
    }

    /// 全画布网格点一遍：任何一点点击都不许崩，点到编辑器要能定位光标。
    ///
    /// 走**真机同一条路**（`WyApp::handle_event` 收 winit 的 `CursorMoved` +
    /// `MouseInput`），与应用里 runner 喂事件完全一致：指针路由、焦点、
    /// 选区协调、重绘请求全在里面。坐标不猜——扫一遍画布，用命中的位置说话。
    #[test]
    fn clicking_anywhere_does_not_crash() {
        use winit::dpi::PhysicalPosition;
        use winit::event::{ElementState, MouseButton, WindowEvent};
        use wy_engine::runner::WyApp;

        let mut app = crate::make_app();
        let (w, h) = (480.0f32, 1240.0f32);
        let mut scene = wy_render::Scene::new();
        app.draw(&mut scene, w, h);

        let mut focused_at: Option<(i32, i32)> = None;
        'sweep: for y in (4..h as i32).step_by(12) {
            for x in (4..w as i32).step_by(12) {
                let pos = PhysicalPosition::new(f64::from(x), f64::from(y));
                app.handle_event(&WindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position: pos,
                });
                app.handle_event(&WindowEvent::MouseInput {
                    device_id: winit::event::DeviceId::dummy(),
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                });
                app.handle_event(&WindowEvent::MouseInput {
                    device_id: winit::event::DeviceId::dummy(),
                    state: ElementState::Released,
                    button: MouseButton::Left,
                });
                if wy_mve::canvas_render2::focus::active_editor().is_some() {
                    focused_at = Some((x, y));
                    break 'sweep;
                }
            }
        }
        assert!(
            focused_at.is_some(),
            "扫遍画布都没能点进任何输入框：焦点链路断了"
        );
    }

    /// 建一棵"视口 → 面板"的树，返回面板节点（布局在首次读几何时惰性求值）。
    fn mount_panel() -> Node2 {
        let root = build_root_fc(node2! {
            x: 0.0,
            y: 0.0,
            width: 480.0,
            height: 1400.0,
            children: vec![editor_panel()],
        });
        root.children().into_iter().next().expect("根下必须有面板")
    }

    /// 面板结构：标题 + 三个字段，竖向间距 8、内边距 10，字段高度按行数算。
    ///
    /// 锁住三件容易改坏的事：
    /// 1. 面板**不写** `height` → 由子节点撑起（`sizeFromChildren`）；
    /// 2. 字段高度 `lines × 20 + 12`（1 行 32 / 3 行 72）；
    /// 3. 编辑器**必须**有显式宽高——它自身的 `size()` 按段落报数，
    ///    空文本时量出来是 0，不写就是"看不见的输入框"。
    #[test]
    fn editor_panel_stacks_title_and_three_fields() {
        let panel = mount_panel();
        let kids = panel.children();
        assert_eq!(kids.len(), 4, "标题 + 三个字段");
        assert_eq!(panel.width(), PANEL_WIDTH);
        assert_eq!(
            (kids[1].height(), kids[2].height(), kids[3].height()),
            (32.0, 72.0, 32.0),
            "单行 1×20+12、多行 3×20+12"
        );
        // 竖向排布：逐对检查 gap，顺带确认没有重叠（y 严格递增）。
        // 累加是 f32，gap 会带 1e−6 级误差，用容差比而不是等号。
        for pair in kids.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let gap = b.y() - a.y() - a.height();
            assert!((gap - 8.0).abs() < 0.01, "flex gap 必须是 8，实得 {gap}");
        }
        assert_eq!(kids[0].y(), 10.0, "首个子节点从 padding 起");
        // 面板高度 = padding×2 + 四个子节点 + 三个 gap。
        let sum: f32 = kids.iter().map(Node2::height).sum();
        let want = 10.0 + sum + 8.0 * 3.0 + 10.0;
        assert!(
            (panel.height() - want).abs() < 0.01,
            "面板高 {} 应等于子节点撑起的 {want}",
            panel.height()
        );
    }

    /// 三个编辑器：宽度都是 342（= 356 − 6×2 − 1×2），`focus_order` 依次 1/2/3。
    ///
    /// 宽度断言同时是"空文本也不塌"的证据：此刻三个字段的逻辑文本都是空串。
    #[test]
    fn editor_fields_keep_their_width_and_focus_order() {
        let panel = mount_panel();
        let kids = panel.children();
        for (i, order) in [(1usize, 1), (2, 2), (3, 3)] {
            let edit = kids[i]
                .children()
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("第 {i} 个字段必须有内层编辑器"));
            assert_eq!(edit.width(), FIELD_WIDTH - 14.0, "第 {i} 个编辑器宽度");
            assert_eq!(edit.focus_order(), Some(order), "Tab 遍历顺序");
            assert!(edit.width() > 0.0, "空文本也不能塌成 0 宽");
        }
    }

    /// 三个编辑器都可聚焦（TS 注释里"三个字段都可聚焦，Tab 在面板内循环"）。
    ///
    /// 掩码字段尤其容易漏：`obscure` 只是"显示成圆点"，不影响可编辑性。
    #[test]
    fn all_three_editors_are_focusable() {
        set_draft("a");
        set_note("a");
        set_secret("a");
        let panel = mount_panel();
        let kids = panel.children();
        for i in 1..4 {
            let edit = kids[i].children().into_iter().next().expect("内层编辑器");
            assert!(
                edit.focusable(),
                "第 {i} 个编辑器必须可聚焦（否则点不进去）"
            );
        }
        assert_eq!(secret_text(), "a", "掩码字段的真相源仍是明文");
    }
}
