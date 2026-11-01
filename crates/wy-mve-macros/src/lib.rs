//! `wy-mve` 的声明式 UI 宏。
//!
//! 只有一个宏：[`macro@rec`]（命名参数 + 默认值 + 同名 `!` 宏）。
//!
//! 旧的 `#[component]`（`XxxProps` + builder + `rsx!` 标签）与 `rsx!`
//! （Dioxus 风格声明式 UI）已随 v1 声明层一起删除——它们生成的产物都围绕
//! 已删除的 `Node` IR，仓库里也不再有任何消费者。
//!
//! # 模块划分
//!
//! - `util`：crate 路径解析（`$crate` 在内部使用与外部使用两种情形下都成立）
//!   与命名转换（`snake_case` → `PascalCase`）。
//! - `props`：参数属性解析（`#[props(default = ..)]` / `#[getter]`）与类型判定。
//! - [`rec`]：`#[rec]` 的展开（产物：`XxxArg` + `XxxArgBuilder` + 同名 `!` 宏）。
//!   按职责拆成 `attr`（组件级属性）/ `field`（**唯一的类型语义判定处**）/
//!   `extends`（基类字段表）/ `codegen`（产物代码）/ `rules`（`!` 宏文本），
//!   `mod.rs` 只做编排。
//!
//! `#[proc_macro_attribute]` 入口必须留在 crate 根（Rust 的硬性要求），实现体全在
//! 下级模块里——`lib.rs` 只做转发，不再承载逻辑。

mod props;
mod rec;
mod util;

use proc_macro::TokenStream;

/// 命名参数化组件的声明层：**返回值就是函数签名写的返回类型**。
///
/// 产物三件套：
/// - `XxxArg`：各字段即"存储类型"（`Get<T>` / `Option<Get<T>>` / `Rc<dyn Fn..>` / …）；
/// - `XxxArgBuilder`：字段收 `Option<存储类型>`，每个字段一个 setter；
/// - 同名 `!` 宏：`flex! { direction: Layout::Row, gap: 8.0 }`，支持字段简写 `ids,`。
///
/// 参数分类（按类型驱动，不靠参数上的辅助属性）：
/// - `impl Fn(..)` → 回调，可省略（默认空实现）；
/// - `Vec<T>` → 可省略（默认空 `Vec`）；
/// - `Option<T>` → setter 收裸 `T`，自动包 `Some`；
/// - `#[getter]` → 读取式字段（见 `props.rs` 的三种形态）；
/// - 其余 → 必填，除非给了 `#[props(default)]` / `#[props(default = expr)]`。
#[proc_macro_attribute]
pub fn rec(attr: TokenStream, item: TokenStream) -> TokenStream {
    match crate::rec::expand(attr.into(), item.into()) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
