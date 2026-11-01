//! `#[rec(extends = base)]` 的基类字段表与 `!` 宏分支生成。
//!
//! 这里登记「基类的 props 类型叫什么、它有哪些字段、字段是 getter 还是值」。
//! 这是整个宏 crate 里**唯一需要跨文件手工同步**的地方：基类的字段列表写在
//! 基类组件的 `#[rec]` 签名上，而本宏跑在编译前、拿不到那份 `syn::Signature`，
//! 只能照抄一份。
//!
//! # 二级继承（`extends = rich_text_node`）
//!
//! `rich_text_node` 自己也是 `extends = node2` 的组件，它的 props
//! （`RichTextNodeArg`）内部**已经**含一份 `Node2Props`。于是
//! `wrapped_text_node` 的 `!` 宏要把 key 分三层走：
//!
//! ```text
//! wrapped_text_node! { text: .., no_wrap: true, width: 200.0 }
//!                      └ 自有      └ 基类      └ 祖父（node2）
//!
//! WrappedTextNodeArg::builder() .text(..)
//!     .rest(RichTextNodeArg::builder() .no_wrap(..)
//!             .rest(Node2Props::builder() .width(..) .build())
//!           .build())
//!     .build().build()
//! ```
//!
//! 所以 [`BaseSpec`] 把字段分成两组：`fields` 打到基类自己的 builder 上，
//! `parent` 打到最内层的 `Node2Props::builder()` 上。宏模板因此有**三个桶**
//! （own / base / node），而不是继承 `node2` 时的两个。
//!
//! `..rest` 在二级继承下展开成基类 builder 的 `.from_props($r)`（`$r` 是
//! `RichTextNodeArg`）。**同时**还写了祖父字段时，祖父那一层以新写的为准：`rest`
//! 是覆盖式 setter，不是逐字段合并（要合并就先把两者都并进一份 `Node2Props`）。
//!
//! # `omit(..)` = TS 的 `extends Omit<Base, 'k'>`
//!
//! 有些组件要把基类的某一格**换成自己的口径**再传给基类（TS 原文就是这样）：
//!
//! ```text
//! interface WrappedTextNodeArg extends Omit<RichTextNodeArg, 'spans'> {
//!   spans?: GetValue<RichTextSpan[] | undefined>   // 重新声明为“可空”
//! }
//! const { text, ..., spans, ...rest } = arg;        // spans 从 rest 里摘出来
//! richTextNode.create({ ...rest, spans: () => spans?.() ?? [singleSpan()] })
//! ```
//!
//! Rust 侧对应 `#[rec(extends = rich_text_node, omit(spans))]` + 自己再声明一个
//! `spans` 参数：`!` 宏不再把 `spans` 转发进 `RichTextNodeArg::builder()`，
//! `rest.spans` 于是只剩组件函数体一个写入点 —— 与 `{ ...rest, spans: .. }`
//! 完全同构（自有字段优先于 `..rest` 带来的那一份，因为 `rest` 是**另一个**
//! 字段，不是合并进来的）。
//!
//! # 这是一处**有护栏的耦合**（而不是随手抄的表）
//!
//! 为什么不能自动推导：`extends` 生成的 `!` 宏展开处不能按绝对路径引用
//! `#[macro_export]` 宏（rustc 限制），所以基类字段只能逐个展开成
//! `<Base>Props` 的 builder 调用——而展开发生在**编译期**，宏却跑在编译前，
//! 拿不到基类的 `syn::Signature`。
//!
//! 同步保证：本表漏了某个基类字段时，调用处一写该 key，`!` 宏就没有匹配分支，
//! 直接报 `no rules expected the token` —— **是编译错误，不是静默失效**。
//! 反之多写一个不存在的字段也会在基类 builder 上找不到 setter 而报错。
//! 所以只要编译通过，这张表就是对的；给基类加字段后跑一次 `cargo check`
//! 即可确认是否需要补这里。

use syn::Ident;

use crate::util::to_pascal_case;

/// 一个可被继承的基类：props 类型名 + 本层字段 + 再上一层字段。
pub(super) struct BaseSpec {
    /// 基类 props 的**真实类型名**（`#[rec(arg = ..)]` 决定，不一定是
    /// `<Pascal>Props`：`rich_text_node` 的 props 叫 `RichTextNodeArg`）。
    pub props: &'static str,
    /// 本层字段（打到 `<props>::builder()` 上）。
    fields: &'static [(&'static str, bool)],
    /// 再上一层字段（打到最内层 `Node2Props::builder()` 上）；空 = 没有更深一层。
    parent: &'static [(&'static str, bool)],
}

/// `node2` 的全部可传字段（与 `element.rs` 里 `#[rec(arg = Node2Props)] node2` 的
/// 签名保持同步）。`true` = getter 字段（宏包 `move ||`，属性即表达式）；
/// `false` = 值 / 回调字段（直接转交）。
const NODE2_FIELDS: &[(&str, bool)] = &[
    ("children", false),
    ("width", true),
    ("height", true),
    ("size", false),
    ("x", true),
    ("y", true),
    ("position", false),
    ("padding", true),
    ("padding_inline_start", true),
    ("padding_inline_end", true),
    ("padding_block_start", true),
    ("padding_block_end", true),
    ("layout", false),
    ("exts", false),
    ("hide", true),
    ("not_in_layout", true),
    ("selection_enabled", true),
    ("skip_draw", true),
    ("focusable", true),
    ("focus_order", true),
    ("focus_trap", true),
    ("on_focus", false),
    ("on_blur", false),
    ("on_hover", false),
    ("handle_key", false),
    ("accept_hit", false),
    ("accept_clip", false),
    ("cursor", true),
    ("draw", false),
    ("draw_at_parent", false),
    ("mouse_click", false),
    ("mouse_click_capture", false),
    ("mouse_down", false),
    ("mouse_down_capture", false),
    ("mouse_up", false),
    ("mouse_up_capture", false),
    ("mouse_move", false),
    ("mouse_move_capture", false),
    ("mouse_wheel", false),
    ("mouse_wheel_capture", false),
    ("mouse_double_click", false),
    ("mouse_double_click_capture", false),
    ("mouse_context_menu", false),
    ("mouse_context_menu_capture", false),
    ("clip", false),
    ("clip_to_parent", false),
    ("debug_name", true),
];

/// `rich_text_node` 的自有字段（与 `rich_text.rs` 里
/// `#[rec(extends = node2)] fn rich_text_node(..)` 的签名保持同步）。
///
/// `spans` 也是 getter（存 `Option<Get<Vec<RichTextSpan>>>`）：调用处写
/// `spans: vec![..]`，宏包 `move ||`。`selection_manager` / `node_ref` 是值语义，
/// 直转。
const RICH_TEXT_FIELDS: &[(&str, bool)] = &[
    ("spans", true),
    ("selection_color", true),
    ("auto_width", true),
    ("no_wrap", true),
    ("max_lines", true),
    ("ellipsis", true),
    ("text_align", true),
    ("selection_manager", false),
    ("node_ref", false),
];

/// 查基类规格；未登记返回 `None`（调用处写基类字段就会撞上
/// `no rules expected the token`，即「要支持新基类，先来本文件登记」）。
fn spec_for(base: &Ident) -> Option<BaseSpec> {
    match base.to_string().as_str() {
        "node2" => Some(BaseSpec {
            props: "Node2Props",
            fields: NODE2_FIELDS,
            parent: &[],
        }),
        "rich_text_node" => Some(BaseSpec {
            props: "RichTextNodeArg",
            fields: RICH_TEXT_FIELDS,
            parent: NODE2_FIELDS,
        }),
        _ => None,
    }
}

/// 基类 props 的类型名（注入 `rest` 字段时用）。未登记的基类回落
/// `<Pascal>Props`，让错误落在类型解析而不是这里。
pub(super) fn props_type(base: &Ident) -> Ident {
    let name = match spec_for(base) {
        Some(s) => s.props.to_string(),
        None => format!("{}Props", to_pascal_case(&base.to_string())),
    };
    Ident::new(&name, base.span())
}

/// 生成基类字段分支：
///
/// - 本层字段 `key: v` → `$base` 桶里追加 `.<key>(..)`（打在基类 builder 上）；
/// - 再上一层字段 → `$node` 桶里追加（打在 `Node2Props::builder()` 上）；
/// - `omit` 点名的字段**不生成分支**（TS `Omit<Base, 'k'>`：它已不是基类字段，
///   调用处写它只会命中组件自己重新声明的那条自有分支）。
pub(super) fn base_arms_for(base: &Ident, omit: &[Ident]) -> String {
    let Some(spec) = spec_for(base) else {
        return String::new();
    };
    let mut s = String::new();
    for &(name, is_getter) in spec.fields {
        if !is_omitted(omit, name) {
            s.push_str(&arm(name, is_getter, false));
        }
    }
    for &(name, is_getter) in spec.parent {
        if !is_omitted(omit, name) {
            s.push_str(&arm(name, is_getter, true));
        }
    }
    s
}

/// `omit(..)` 里的名字是否就是 `name`。
fn is_omitted(omit: &[Ident], name: &str) -> bool {
    omit.iter().any(|o| o == name)
}

/// 校验 `omit(..)` 点名的都是**基类真有的**字段。
///
/// 这条检查值得写：基类字段表是手工同步的（见模块头），`omit` 拼错时若不报错，
/// 表现会是「`!` 宏突然拒绝这个 key」——错误信息离真正的原因很远。这里直接把
/// 合法名字列出来。
pub(super) fn validate_omit(base: &Ident, omit: &[Ident]) -> syn::Result<()> {
    if omit.is_empty() {
        return Ok(());
    }
    let Some(spec) = spec_for(base) else {
        return Err(syn::Error::new_spanned(
            omit.first().expect("非空"),
            format!(
                "`extends = {base}` 的字段表还没登记（见 wy-mve-macros/src/rec/extends.rs），\
                 无从判断 `omit(..)` 是否有效"
            ),
        ));
    };
    for name in omit {
        let known = spec
            .fields
            .iter()
            .chain(spec.parent)
            .any(|&(f, _)| name == f);
        if !known {
            let mut names: Vec<&str> = spec
                .fields
                .iter()
                .chain(spec.parent)
                .map(|&(f, _)| f)
                .collect();
            names.sort_unstable();
            return Err(syn::Error::new_spanned(
                name,
                format!(
                    "`omit({name})`：`{base}`（{}）没有这个字段。可省略的有：{}",
                    spec.props,
                    names.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

/// 一条基类字段分支；`to_parent = true` 时推进 `$node` 桶，否则推进 `$base` 桶。
fn arm(name: &str, is_getter: bool, to_parent: bool) -> String {
    let call = if is_getter {
        format!(".{name}(move || $v)")
    } else {
        format!(".{name}($v)")
    };
    let (base, node) = if to_parent {
        (String::from("$($base)*"), format!("$($node)* {call}"))
    } else {
        (format!("$($base)* {call}"), String::from("$($node)*"))
    };
    format!(
        "    (@m [$($own:tt)*] [$($base:tt)*] [$($node:tt)*] {name} : $v:expr, $($rest:tt)*) => {{\n        \
         __NAME__!(@m [$($own)*] [{base}] [{node}] $($rest)*)\n    }};\n"
    )
}

/// 终局表达式的尾段（接在 `__PROPS__::builder() $($own)*` 之后）。
///
/// `has_node` = 本次调用是否写了祖父（`node2`）的字段。为 `false` 时**不能**
/// 再补一份空的 `.rest(Node2Props::default())`：那会把 `..rest` 带进来的 node2
/// 参数整份冲掉（`rest` 是覆盖式 setter，不是合并）。
pub(super) fn base_tail(base: &Ident, has_node: bool) -> String {
    let Some(spec) = spec_for(base) else {
        return String::new();
    };
    let props = spec.props;
    if spec.parent.is_empty() || !has_node {
        format!(".rest($crate::{props}::builder() $($base)* .build())")
    } else {
        format!(
            ".rest($crate::{props}::builder() $($base)* \
             .rest($crate::Node2Props::builder() $($node)+ .build()) .build())"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Ident {
        Ident::new(s, proc_macro2::Span::call_site())
    }

    /// `omit(k)` 之后 `k` 不再有转发进基类 builder 的分支。
    #[test]
    fn omit_drops_the_base_arm() {
        let arms = base_arms_for(&id("rich_text_node"), &[id("spans")]);
        assert!(
            !arms.contains("spans : $v:expr"),
            "被 omit 的字段不该再有转发分支，实际生成：\n{arms}"
        );
        assert!(
            arms.contains("no_wrap : $v:expr"),
            "没被 omit 的基类字段必须照旧转发"
        );
    }

    /// `omit` 也能省掉**再上一层**（`node2`）的字段——两层是同一份名单。
    #[test]
    fn omit_reaches_the_grandparent_layer() {
        let arms = base_arms_for(&id("rich_text_node"), &[id("children")]);
        assert!(!arms.contains("children : $v:expr"));
        assert!(arms.contains("width : $v:expr"), "node2 的其它字段照旧");
    }

    /// 拼错的字段名要当场报错，并把可省略的名字列出来。
    #[test]
    fn validate_omit_rejects_unknown_names() {
        let err = validate_omit(&id("rich_text_node"), &[id("span")])
            .expect_err("`span` 不是基类字段，应报错");
        let msg = err.to_string();
        assert!(msg.contains("没有这个字段"), "实际：{msg}");
        assert!(msg.contains("spans"), "应列出可省略的字段名：{msg}");
    }

    /// 合法的 `omit` 不报错（含 `node2` 那一层的名字）。
    #[test]
    fn validate_omit_accepts_known_names() {
        assert!(validate_omit(&id("rich_text_node"), &[id("spans"), id("width")]).is_ok());
        assert!(validate_omit(&id("node2"), &[id("children")]).is_ok());
    }
}
