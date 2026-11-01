//! 新体系（canvasRender2）版的 shadcn 基础组件：**一个组件一个模块**。
//!
//! 与旧 `shadcn/widgets.rs` 的**逐项对应**：
//!
//! | 旧（`#[component]` + `rsx!`） | 新（`#[rec]` + `node2!`） |
//! |---|---|
//! | `rsx! { node { .. } }.into_node()` → v1 `Node` | `node2! { .. }` → `Fc` |
//! | `draw: move \|s: &mut Scene\|` + `current_box()` | `draw: move \|s, n: &Node2\|` + `n.width()/n.height()` |
//! | `on_click` / `on_down` / `on_up` | `mouse_click` / `mouse_down` / `mouse_up` |
//! | `children: Vec<Slot>` 展开 `..children` | `children: Vec<Fc>` 直接传 |
//! | `padding: CARD_PAD` | `padding: CARD_PAD`（同一个简写字段） |
//!
//! # 三条容易踩的规则
//!
//! 1. **尺寸取 `n.width()` / `n.height()`，不要 `n.geom()`**。`geom()` 是 v1
//!    引擎在布局期回填的缓存；纯 `Node2App` 运行时根本没有 v1 引擎，那份缓存
//!    永远是 0。`width()` 走 `declared()` → `defaultSize`（父布局实时分配），
//!    是 Node2 自己的真相源。
//! 2. **要「由父分配尺寸」的容器必须 `align_fix: true`**。否则
//!    `allows_size_from_children`（列容器 + 无 grow 子节点）为真，尺寸退回
//!    内容撑开，父级的 `grow` 就落不到它身上。旧体系靠 `size: (0, 0)` 绕开，
//!    新体系对应 TS 的 `flex({ alignFix: true })`。
//!    **自己声明了 `height` 的行也要**（否则 `Center` 的基准变成最高子节点，
//!    行内元素整体偏上几像素，见 `todos::todo_row`）。
//! 3. **文本一律是子节点（`text_label!`），不在 `draw` 里 `draw_text`**。
//!    手绘的字父布局不知道宽高，居中 / 徽章宽度 / 撑尺寸全得手算；
//!    换成文本节点后，宽度由 `padding + 文本自然宽` 撑出来、位置由 flex 算
//!    （`button` / `badge` / `tabs` / `avatar` / `stat_card` 都是这个形态）。
//!    图标仍走 `draw`（矢量路径），但也是**定尺寸子节点**。
//!
//! # 宏与模块的关系（拆模块时最容易挂的地方）
//!
//! - `#[rec]` 生成的 `!` 宏体里写的是 `$crate::XxxArg`，`$crate` 指**示例 crate
//!   的根**。组件下沉到子模块后，`XxxArg` 必须由本模块再导出、`main.rs` 再
//!   `pub use` 到根（见 `main.rs` 的 `pub use widgets::{..}`）。
//! - `#[macro_use]` 的可见性**按声明顺序**：提供宏的模块要排在使用者前面。
//!   这里每个组件自成一个模块、互不引用宏，所以顺序只影响外层（`form` /
//!   `todos` / `sidebar` / `dashboard` 必须排在 `mod widgets` 之后）。

// `#[macro_use]` 全部前置：宏按声明顺序对后续模块可见。
//
// `text_label` 必须排第一：`button` / `badge` / `tabs` / `avatar` 的文案都是它的
// 子节点（**文本一律走文本组件**，不在 `draw` 里 `draw_text`）。
#[macro_use]
mod text_label;
#[macro_use]
mod avatar;
#[macro_use]
mod badge;
#[macro_use]
mod button;
#[macro_use]
mod card;
#[macro_use]
mod checkbox;
#[macro_use]
mod separator;
#[macro_use]
mod skeleton;
#[macro_use]
mod tabs;
#[macro_use]
mod toggle;

// 纯绘制辅助（非组件，不带宏）。
mod icon;

// 只导出「别人真会用的东西」：
// - `XxxArg`：`#[rec]` 的 `!` 宏要 `$crate::XxxArg`，`main.rs` 从本模块再导出到根；
// - 被当**函数**调用的那几个（`card(..)` / `separator(..)` / `tabs(..)` 等）；
// - `Btn` 变体枚举与 `draw_icon_at` 绘制辅助。
// 只经 `!` 宏使用的组件不用再导出函数体——宏在定义模块内解析，与这里无关。
pub use avatar::AvatarArg;
pub use badge::BadgeArg;
pub use button::{Btn, ButtonArg, ButtonSizedArg};
pub use card::{card, CardArg};
pub use checkbox::CheckboxArg;
pub use icon::draw_icon_at;
pub use separator::{separator, SeparatorArg};
pub use skeleton::{skeleton, SkeletonArg};
pub use tabs::{tabs, TabsArg};
pub use text_label::TextLabelArg;
pub use toggle::ToggleArg;
