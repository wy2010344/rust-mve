//! 同名 `!` 宏的生成：把 `{ k: v, .. }` 摊成 builder 链式调用。
//!
//! # 为什么这里拼字符串而不是 quote
//!
//! `macro_rules!` 的分支要按字段名逐条生成（getter 字段的右值必须包
//! `move || $v`，且**字面量 ident 分支要排在通用 `$k:ident : $v:expr` 之前**
//! 才一定先命中），而分支里的 `$` 在 `quote!` 里需要转义、可读性极差。
//! 拼好字符串最后 `parse()` 一次，比逐 token 构造更直白。
//!
//! # 为什么不生成独立的 `<base>_arg!` 宏
//!
//! `extends` 的基类字段转发**内联**在下面两段式模板里（由
//! [`base_arms_for`](super::extends::base_arms_for) 把基类字段展开成
//! `<Base>Props` 的 builder 调用，`.build()` 就是那份 props）：不引用任何
//! `#[macro_export]` 宏，规避「宏展开内绝对路径引用 `#[macro_export]` 宏」的
//! rustc 限制。
//!
//! # `..rest`（TS 的 `...rest`）
//!
//! 每个 `!` 宏都认一条 `..rest,` 分支：把一份现成的 props 展开成 builder 的
//! 起点（`from_props`），后面写的字段自然覆盖它。于是 `#[rec(extends = node2)]`
//! 的组件体可以只写差异：
//!
//! ```ignore
//! node2! { ..rest, focusable: true, draw: .. }
//! ```
//!
//! `..rest` 必须写在**最前面**：它按值展开整份 props，写在中间会被它右边、
//! 但更早出现的 setter 抢先覆盖，语义就反了。

use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use super::extends::{base_arms_for, base_tail};
use super::field::V2Field;

/// 生成同名 `!` 宏。
pub(super) fn gen(
    func: &Ident,
    props: &Ident,
    fields: &[V2Field],
    extends: Option<&Ident>,
    omit: &[Ident],
) -> syn::Result<TokenStream2> {
    let template = match extends {
        // 继承：三个桶（own / base / node）。基类若自己也 extends（如
        // `wrapped_text_node` → `rich_text_node` → `node2`），祖父字段落进第三桶；
        // 只有 `node2` 这一层时 `$node` 恒空，第三条终局分支不会被匹配。
        Some(base) => r#"
#[macro_export]
macro_rules! __NAME__ {
    (@m [$($own:tt)*] [$($base:tt)*] []) => {
        $crate::__PROPS__::builder() $($own)* __TAIL_EMPTY__ .build().build()
    };
    (@m [$($own:tt)*] [$($base:tt)*] [$($node:tt)+]) => {
        $crate::__PROPS__::builder() $($own)* __TAIL_NODE__ .build().build()
    };
    (@m [$($own:tt)*] [$($base:tt)*] [$($node:tt)*] , $($rest:tt)*) => {
        __NAME__!(@m [$($own)*] [$($base)*] [$($node)*] $($rest)*)
    };
    (@m [$($own:tt)*] [$($base:tt)*] [$($node:tt)*] .. $r:expr, $($rest:tt)*) => {
        __NAME__!(@m [$($own)*] [$($base)* .from_props($r)] [$($node)*] $($rest)*)
    };
__OWN_ARMS____BASE_ARMS__
    ($($t:tt)*) => {{ __NAME__!(@m [] [] [] $($t)* ,) }};
}
"#
        .replace("__TAIL_EMPTY__", &base_tail(base, false))
        .replace("__TAIL_NODE__", &base_tail(base, true))
        .replace("__OWN_ARMS__", &own_arms(fields, Some(base)))
        .replace("__BASE_ARMS__", &base_arms_for(base, omit))
        .to_string(),
        None => r#"
#[macro_export]
macro_rules! __NAME__ {
    (@m [$($acc:tt)*] ) => {
        $crate::__PROPS__::builder() $($acc)* .build().build()
    };
    (@m [$($acc:tt)*] , $($rest:tt)*) => { __NAME__!(@m [$($acc)*] $($rest)*) };
    (@m [$($acc:tt)*] .. $r:expr, $($rest:tt)*) => {
        __NAME__!(@m [$($acc)* .from_props($r)] $($rest)*)
    };
__OWN_ARMS__
    (@m [$($acc:tt)*] $k:ident, $($rest:tt)*) => {
        __NAME__!(@m [$($acc)* .$k($k)] $($rest)*)
    };
    (@m [$($acc:tt)*] $k:ident : $v:expr, $($rest:tt)*) => {
        __NAME__!(@m [$($acc)* .$k($v)] $($rest)*)
    };
    ($($t:tt)*) => { __NAME__!(@m [] $($t)* ,) };
}
"#
        .replace("__OWN_ARMS__", &own_arms(fields, None))
        .to_string(),
    };

    Ok(template
        .replace("__NAME__", &func.to_string())
        .replace("__PROPS__", &props.to_string())
        .parse()
        .expect("生成的 macro_rules 模板应当是合法 token"))
}

/// 自有字段的分支。
///
/// - 非继承：只给 getter 字段生成字面量 ident 分支，其余走通用
///   `$k:ident : $v:expr`。
/// - 继承：给**全部自有字段**（除 `rest`）生成分支；外加一条「其余 key 转发给
///   基类」的兜底分支，从而把未知 key 原样喂给基类的 builder。
///
/// 简写（`gap,`）**不给** getter 字段开洞：简写要转写出 `.gap(move || gap)`，
/// 而这条分支里的 `gap` 是宏定义侧生成的 ident（proc-macro 的 span），落到
/// 调用处会被 hygienic 挡住（E0425「not accessible due to macro hygiene」）；
/// 通用分支能用 `$k` 是因为它来自**调用处**的 token。所以写全 `gap: gap`。
fn own_arms(fields: &[V2Field], extends: Option<&Ident>) -> String {
    let wrap = |f: &V2Field| {
        if f.is_getter() {
            "(move || $v)"
        } else {
            "($v)"
        }
    };
    match extends {
        Some(_) => fields
            .iter()
            .filter(|f| f.ident != "rest")
            .map(|f| {
                let (id, call) = (&f.ident, wrap(f));
                format!(
                    "    (@m [$($own:tt)*] [$($base:tt)*] [$($node:tt)*] {id} : $v:expr, $($rest:tt)*) => {{\n        \
                     __NAME__!(@m [$($own)* .{id}{call}] [$($base)*] [$($node)*] $($rest)*)\n    }};\n"
                )
            })
            .collect(),
        None => fields
            .iter()
            .filter(|f| f.is_getter())
            .map(|f| {
                let id = &f.ident;
                format!(
                    "    (@m [$($acc:tt)*] {id} : $v:expr, $($rest:tt)*) => {{\n        \
                     __NAME__!(@m [$($acc)* .{id}(move || $v)] $($rest)*)\n    }};\n"
                )
            })
            .collect(),
    }
}
