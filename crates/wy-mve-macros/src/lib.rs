//! `rsx!` 声明式 UI 宏：对齐 Dioxus `rsx!` 风格。
//!
//! 语法速览：
//! - 容器：`vbox { child1 child2 }` —— 子块内平铺，无需逗号/括号
//! - 属性：`vbox { gap: 12.0, width: w.get(), hide: empty.get(), btn { .. } }`
//!   —— `name: expr` 是属性（通用布局属性直接映射 [`Node`] 字段），其余是子节点。
//!   非字面量表达式由宏包成 `move || expr` 闭包，求值发生在布局/绘制期，
//!   读信号自动追踪（复刻 Kotlin「属性是 getter」的形态）。
//! - 组件：`btn { label: "x", on_click: move || { ... } }` —— 字段语法绑事件
//! - 动态文本：`label(expr)` —— 自动包 `move || expr.to_string()`，
//!   draw 期求值，信号变化自动追踪重绘
//! - 列表：`for_each(list, key_fn) { |item, cx| { ... } }`
//! - 条件：`if cond { ... } else { ... }` —— 转 `if/else`，false 分支不产出节点
//!
//! 可用布局属性：`x` `y` `width` `height` `gap` `hide` `size`（返回 `(w, h)`）
//! `focusable` `skip_draw` `focus_trap` `selection_enabled` `focus_order`。
//!
//! 全部编译到 `wy_mve::fc` 的等价函数；宏只省掉闭包/逗号/`.into()` 样板。
//! 所有子节点统一生成为 `wy_mve::fc::Slot`，容器/条件产出 `Vec<Slot>`。

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use proc_macro_crate::{crate_name, FoundCrate};
use quote::{format_ident, quote, ToTokens};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    Expr, Ident, LitStr, Stmt, Token,
};

/// 解析 `wy_mve` 的真实 crate 路径（宏在本 crate 内部使用时解析为 `crate`）。
fn mve() -> TokenStream2 {
    match crate_name("wy-mve") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name);
            quote! { ::#ident }
        }
        Err(_) => quote! { ::wy_mve },
    }
}

/// 空白容忍：跳过 `syn` 可能留下的空 token。
fn skip_junk(input: ParseStream) {
    while input.peek(Token![,]) {
        let _ = input.parse::<Token![,]>();
    }
}

// ============================================================================
// 数据结构
// ============================================================================

/// 一个 rsx 节点。
enum Node {
    /// 容器：`vbox { ... }` / `hbox { ... }`
    ///
    /// `fields` 是通用布局属性（`gap`/`width`/`hide` 等），直接映射到
    /// [`Node`] 的静态字段或动态 fn 字段（复刻 Kotlin getter 语义）。
    Container {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
        children: Vec<Node>,
    },
    /// 带位置参数的容器：`vbox_at(300.0, 50.0) { ... }`
    PositionalContainer {
        name: Ident,
        args: Vec<Expr>,
        fields: Vec<(Ident, Expr)>,
        children: Vec<Node>,
    },
    /// 组件：`btn { label: "...", on_click: ... }`
    Component {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
    },
    /// 静态文本节点（裸字面量）
    Text(LitStr),
    /// 动态组件/文本调用：`label(expr)`
    Dynamic(Expr),
    /// 条件：`if cond { .. } else { .. }`
    Conditional {
        cond: Expr,
        then_branch: Vec<Node>,
        else_branch: Option<Vec<Node>>,
    },
    /// 列表：`for_each(list, key_fn) { |item, cx| { body } }`
    ForEach {
        source: Expr,
        key_fn: Expr,
        params: Vec<Ident>,
        prelude: Vec<Stmt>,
        body: Vec<Node>,
    },
    /// 原始 Rust 语句（`let` 绑定等），原样透传。
    Raw(Vec<Stmt>),
}

// ============================================================================
// 解析
// ============================================================================

impl Parse for Node {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        skip_junk(input);

        // if cond { .. } else { .. }
        if input.peek(Token![if]) {
            let _: Token![if] = input.parse()?;
            // cond 必须止于 `{`：用 parse_without_eager_brace 避免
            // syn 把 `{ .. }` 当成 struct 字面量吞掉。
            let cond: Expr = syn::Expr::parse_without_eager_brace(input)?;
            let then_branch = parse_nodes_block(input)?;
            let else_branch = if input.peek(Token![else]) {
                let _: Token![else] = input.parse()?;
                Some(parse_nodes_block(input)?)
            } else {
                None
            };
            return Ok(Node::Conditional {
                cond,
                then_branch,
                else_branch,
            });
        }

        // 裸文本字面量
        if input.peek(LitStr) {
            let lit: LitStr = input.parse()?;
            return Ok(Node::Text(lit));
        }

        let name: Ident = input.parse()?;

        // for_each(list, key_fn) { |item, cx| { body } }
        if name == "for_each" {
            let args = if input.peek(syn::token::Paren) {
                parse_paren_args(input)?
            } else {
                return Err(syn::Error::new(
                    name.span(),
                    "for_each 需要 `for_each(list, key_fn) { |item, cx| { ... } }`",
                ));
            };
            if args.len() != 2 {
                return Err(syn::Error::new(
                    name.span(),
                    "for_each 需要两个参数：`for_each(list, |item| item.id)`",
                ));
            }
            let (params, nodes) = parse_for_each_tail(input)?;
            // 分离 prelude（let 语句）与 rsx 节点
            let mut prelude: Vec<Stmt> = Vec::new();
            let mut body: Vec<Node> = Vec::new();
            for n in nodes {
                match n {
                    Node::Raw(stmts) => prelude.extend(stmts),
                    other => body.push(other),
                }
            }
            return Ok(Node::ForEach {
                source: args[0].clone(),
                key_fn: args[1].clone(),
                params,
                prelude,
                body,
            });
        }

        // `Tag(args)`
        if input.peek(syn::token::Paren) {
            let args = parse_paren_args(input)?;
            // 后跟 `{` → 带定位参数的容器（如 `vbox_at(1.0, 2.0) { .. }`）
            if input.peek(syn::token::Brace) {
                let (fields, children) = parse_element_block(input)?;
                return Ok(Node::PositionalContainer {
                    name,
                    args,
                    fields,
                    children,
                });
            }
            return Ok(Node::Dynamic(if args.is_empty() {
                syn::parse_quote!(#name)
            } else {
                syn::parse_quote!(#name(#(#args),*))
            }));
        }

        // `Tag { .. }`
        if input.peek(syn::token::Brace) {
            let (fields, children) = parse_element_block(input)?;
            // 无子节点且有字段 → 组件（btn/label 的字段语法）；
            // 否则 → 容器（字段是通用布局属性，可与子节点共存）
            if children.is_empty() && !fields.is_empty() {
                Ok(Node::Component { name, fields })
            } else {
                Ok(Node::Container {
                    name,
                    fields,
                    children,
                })
            }
        } else {
            Err(syn::Error::new(
                name.span(),
                format!("`{name}` 后面需要 `{{ ... }}` 或 `(...)`，例如 `{name} {{ ... }}`"),
            ))
        }
    }
}

/// 解析 `{ ... }` 为子节点列表。
///
/// 允许混入普通 Rust 语句（`let` 绑定等）—— 以 [`Node::Raw`] 原样透传，
/// 这样 `for_each` 的 body 里可以先 `let item = create_signal(..)` 再建节点。
fn parse_nodes_block(input: ParseStream) -> syn::Result<Vec<Node>> {
    let content;
    syn::braced!(content in input);
    let mut nodes: Vec<Node> = Vec::new();
    let mut raw: Vec<Stmt> = Vec::new();

    while !content.is_empty() {
        skip_junk(&content);
        if content.is_empty() {
            break;
        }
        // Rust 语句（let / 赋值等）→ 原样透传
        if content.peek(Token![let]) {
            raw.push(content.parse::<Stmt>()?);
            continue;
        }
        // 节点起始：if / 字面量 / `Ident {` / `Ident (` / `Ident:`
        let is_node = content.peek(Token![if])
            || content.peek(LitStr)
            || (content.peek(Ident)
                && (content.peek2(syn::token::Brace)
                    || content.peek2(syn::token::Paren)
                    || content.peek2(Token![:])));
        if is_node {
            nodes.push(content.parse::<Node>()?);
        } else {
            raw.push(content.parse::<Stmt>()?);
        }
    }
    if !raw.is_empty() {
        nodes.push(Node::Raw(raw));
    }
    Ok(nodes)
}

/// 元素块的解析结果：(字段列表, 子节点列表)。
type ElementBlock = (Vec<(Ident, Expr)>, Vec<Node>);

/// 解析元素块 `{ ... }`：区分 `name: expr` 字段与子节点。
///
/// 允许混入普通 Rust 语句（`let` 绑定等）—— 以 [`Node::Raw`] 原样透传，
/// 且按出现顺序落盘（如逐按钮 `let c = Rc::clone(..)` 必须先于按钮求值）。
fn parse_element_block(input: ParseStream) -> syn::Result<ElementBlock> {
    let content;
    syn::braced!(content in input);
    let mut fields = Vec::new();
    let mut children: Vec<Node> = Vec::new();
    let mut raw: Vec<Stmt> = Vec::new();

    while !content.is_empty() {
        skip_junk(&content);
        if content.is_empty() {
            break;
        }
        // 字段：`ident : expr`
        if content.peek(Ident) && content.peek2(Token![:]) {
            let name: Ident = content.parse()?;
            let _: Token![:] = content.parse()?;
            let value: Expr = content.parse()?;
            fields.push((name, value));
            continue;
        }
        // Rust 语句（let / 赋值等）→ 暂存，遇到节点时按序落盘
        if content.peek(Token![let]) {
            raw.push(content.parse::<Stmt>()?);
            continue;
        }
        // 子节点：先把积累的语句落盘，保持执行顺序
        if !raw.is_empty() {
            children.push(Node::Raw(std::mem::take(&mut raw)));
        }
        children.push(content.parse::<Node>()?);
    }
    if !raw.is_empty() {
        children.push(Node::Raw(raw));
    }
    Ok((fields, children))
}

fn parse_paren_args(input: ParseStream) -> syn::Result<Vec<Expr>> {
    let content;
    parenthesized!(content in input);
    let mut args = Vec::new();
    while !content.is_empty() {
        args.push(content.parse::<Expr>()?);
        if content.peek(Token![,]) {
            let _: Token![,] = content.parse()?;
        }
    }
    Ok(args)
}

/// 解析 for_each 尾块 `{ |item, cx| { body } }`。
fn parse_for_each_tail(input: ParseStream) -> syn::Result<(Vec<Ident>, Vec<Node>)> {
    let outer;
    syn::braced!(outer in input);
    // 起始 `|`
    let _: Token![|] = outer.parse()?;
    let mut params = Vec::new();
    while !outer.peek(Token![|]) {
        params.push(outer.parse::<Ident>()?);
        if outer.peek(Token![,]) {
            let _: Token![,] = outer.parse()?;
        } else if !outer.peek(Token![|]) {
            return Err(syn::Error::new(
                Span::call_site(),
                "for_each 参数列表应为 `|item, cx|`",
            ));
        }
    }
    // 闭合 `|`
    let _: Token![|] = outer.parse()?;
    let body = parse_nodes_block(&outer)?;
    Ok((params, body))
}

// ============================================================================
// 代码生成
// ============================================================================

/// 生成一个子节点表达式的 TokenStream，结果类型恒为 `Slot`。
fn gen_child(node: &Node) -> syn::Result<TokenStream2> {
    let m = mve();
    match node {
        Node::Text(lit) => Ok(quote! {
            #m::fc::Slot::One(#m::text(move || #lit.to_string()))
        }),
        Node::Dynamic(expr) => Ok(gen_dynamic(expr)),
        Node::Conditional {
            cond,
            then_branch,
            else_branch,
        } => {
            let then_expr = gen_children_vec(then_branch)?;
            let else_expr = match else_branch {
                Some(b) => gen_children_vec(b)?,
                None => quote! { ::std::vec![] },
            };
            Ok(quote! {
                if #cond {
                    #then_expr
                } else {
                    #else_expr
                }
            })
        }
        Node::ForEach {
            source,
            key_fn,
            params,
            prelude,
            body,
        } => gen_for_each(source, key_fn, params, prelude, body),
        Node::Container {
            name,
            fields,
            children,
        } => {
            let f = format_ident!("fc__{}", name);
            let arr = gen_children_vec(children)?;
            let attrs = gen_attr_assigns(fields, name)?;
            if attrs.is_empty() {
                Ok(quote! { #m::fc::Slot::One(#m::fc::#f(#arr)) })
            } else {
                Ok(quote! { #m::fc::Slot::One({ let mut __n = #m::fc::#f(#arr); #(#attrs)* __n }) })
            }
        }
        Node::PositionalContainer {
            name,
            args,
            fields,
            children,
        } => {
            let f = format_ident!("fc__{}", name);
            let arr = gen_children_vec(children)?;
            let attrs = gen_attr_assigns(fields, name)?;
            if attrs.is_empty() {
                Ok(quote! { #m::fc::Slot::One(#m::fc::#f(#(#args,)* #arr)) })
            } else {
                Ok(
                    quote! { #m::fc::Slot::One({ let mut __n = #m::fc::#f(#(#args,)* #arr); #(#attrs)* __n }) },
                )
            }
        }
        Node::Component { name, fields } => Ok(gen_component(name, fields)),
        // 原始 Rust 语句块：求值后不产出节点
        Node::Raw(stmts) => {
            let m = mve();
            Ok(quote! {{
                #(#stmts)*
                ::std::vec::Vec::<#m::fc::Slot>::new()
            }})
        }
    }
}

/// `label(expr)` → 闭包包装的动态文本；`label("lit")` → 静态文本。
fn gen_dynamic(expr: &Expr) -> TokenStream2 {
    let m = mve();
    let Expr::Call(call) = expr else {
        return quote! { #m::fc::Slot::One(#expr) };
    };
    let fname = call.func.to_token_stream().to_string();
    let fname = fname.rsplit("::").next().unwrap_or(&fname).to_string();
    if !matches!(fname.as_str(), "label" | "caption" | "text") {
        return quote! { #m::fc::Slot::One(#expr) };
    }
    let Some(first) = call.args.first() else {
        return quote! { #m::fc::Slot::One(#expr) };
    };
    // 调用方自备闭包 → 透传
    if matches!(first, Expr::Closure(_)) {
        return quote! { #m::fc::Slot::One(#expr) };
    }
    let inner = &call.args[0];
    quote! {
        #m::fc::Slot::One(#m::text(move || ::std::string::ToString::to_string(&(#inner))))
    }
}

/// 生成 `Vec<Slot>`。
///
/// `if` 分支产出 `Vec<Slot>`（多元素），需展开拼接；
/// 其它节点产出单个 `Slot`，push 即可。
fn gen_children_vec(nodes: &[Node]) -> syn::Result<TokenStream2> {
    let m = mve();
    let mut parts: Vec<TokenStream2> = Vec::new();
    for n in nodes {
        match n {
            // if 分支 → Vec<Slot>，展开拼接
            Node::Conditional { .. } => {
                let one = gen_child(n)?;
                parts.push(quote! { __out.extend(#one); });
            }
            // 原始 Rust 语句 → 只求值不产出
            Node::Raw(stmts) => {
                parts.push(quote! { #(#stmts)* });
            }
            _ => {
                let one = gen_child(n)?;
                parts.push(quote! { __out.push(#one); });
            }
        }
    }
    Ok(quote! {{
        let mut __out: ::std::vec::Vec<#m::fc::Slot> = ::std::vec::Vec::new();
        #(#parts)*
        __out
    }})
}

fn gen_for_each(
    source: &Expr,
    key_fn: &Expr,
    params: &[Ident],
    prelude: &[Stmt],
    body: &[Node],
) -> syn::Result<TokenStream2> {
    let m = mve();
    let item = params
        .first()
        .cloned()
        .unwrap_or_else(|| Ident::new("__item", Span::call_site()));
    let cx_ident = params
        .get(1)
        .cloned()
        .unwrap_or_else(|| Ident::new("__cx", Span::call_site()));
    let body_vec = gen_children_vec(body)?;
    Ok(quote! {
        #m::fc::for_each_slots(
            move |__emit| {
                for __it in #source.get() {
                    __emit((#key_fn)(&__it), __it);
                }
            },
            move |#item, #cx_ident| {
                // body 内的 let 语句在 creater 作用域执行，节点随后构建
                #(#prelude)*
                let __slots: ::std::vec::Vec<#m::fc::Slot> = #body_vec;
                #m::fc::push_all(__slots, #cx_ident);
            },
        )
    })
}

/// 通用布局属性名 → 赋值语句列表。
///
/// 复刻 Kotlin「属性是 getter」的形态：非字面量表达式统一包成
/// `move || expr` 闭包赋给 `*_fn` 字段，求值发生在布局/绘制期
/// （信号 track 上下文内），读信号自动追踪；f32/bool 字面量直接赋
/// 静态字段，零闭包开销。
///
/// | 属性 | 静态字段 | 动态 fn 字段 | Kotlin 对应 |
/// |---|---|---|---|
/// | `x`/`y` | `x`/`y` | `x_fn`/`y_fn` | `argPosition` |
/// | `width`/`height` | `width`/`height` | `width_fn`/`height_fn` | `argWidth`/`argHeight` |
/// | `gap` | `gap` | `gap_fn` | `FlexParam.gap` |
/// | `hide` | `hidden` | `hide_fn` | `hide` |
/// | `size` | — | `size_fn`（闭包返回 `(w, h)`） | `argSize` |
///
/// 其余布尔字段（`focusable`/`skip_draw`/`focus_trap`/`selection_enabled`）
/// 构建期直接赋值；`focus_order` 赋 `Option<i32>`。
fn gen_attr_assigns(fields: &[(Ident, Expr)], container: &Ident) -> syn::Result<Vec<TokenStream2>> {
    let mut out = Vec::new();
    for (fname, value) in fields {
        let Some(stmt) = gen_attr_assign(fname, value) else {
            return Err(syn::Error::new(
                fname.span(),
                format!(
                    "容器 `{container}` 不支持属性 `{fname}`；可用：x, y, width, height, \
                     gap, hide, size, focusable, skip_draw, focus_trap, \
                     selection_enabled, focus_order"
                ),
            ));
        };
        out.push(stmt);
    }
    Ok(out)
}

/// 单个属性 → 赋值语句。
fn gen_attr_assign(fname: &Ident, value: &Expr) -> Option<TokenStream2> {
    match fname.to_string().as_str() {
        "x" => Some(num_attr(quote! { x }, quote! { x_fn }, value)),
        "y" => Some(num_attr(quote! { y }, quote! { y_fn }, value)),
        "width" => Some(num_attr(quote! { width }, quote! { width_fn }, value)),
        "height" => Some(num_attr(quote! { height }, quote! { height_fn }, value)),
        "gap" => Some(num_attr(quote! { gap }, quote! { gap_fn }, value)),
        "hide" => Some(bool_attr(quote! { hidden }, quote! { hide_fn }, value)),
        // 整体尺寸覆盖（Kotlin `override fun argSize`），闭包返回 (w, h)
        "size" => Some(quote! {
            __n.size_fn = Some(::std::rc::Rc::new(move || #value));
        }),
        // 构建期布尔字段（Kotlin 里这些是构造配置而非 getter）
        "focusable" => Some(quote! { __n.focusable = #value; }),
        "skip_draw" => Some(quote! { __n.skip_draw = #value; }),
        "focus_trap" => Some(quote! { __n.focus_trap = #value; }),
        "selection_enabled" => Some(quote! { __n.selection_enabled = #value; }),
        "focus_order" => Some(quote! { __n.focus_order = #value; }),
        _ => None,
    }
}

/// f32 属性：字面量直赋静态字段，其余包闭包给 fn 字段。
fn num_attr(static_field: TokenStream2, fn_field: TokenStream2, value: &Expr) -> TokenStream2 {
    if is_float_lit(value) {
        quote! { __n.#static_field = #value; }
    } else {
        quote! { __n.#fn_field = Some(::std::rc::Rc::new(move || #value)); }
    }
}

/// bool 属性：字面量直赋静态字段，其余包闭包给 fn 字段。
fn bool_attr(static_field: TokenStream2, fn_field: TokenStream2, value: &Expr) -> TokenStream2 {
    if matches!(
        value,
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(_),
            ..
        })
    ) {
        quote! { __n.#static_field = #value; }
    } else {
        quote! { __n.#fn_field = Some(::std::rc::Rc::new(move || #value)); }
    }
}

fn is_float_lit(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(_),
            ..
        })
    )
}

fn gen_component(name: &Ident, fields: &[(Ident, Expr)]) -> TokenStream2 {
    let m = mve();
    let mut on_click = None;
    let mut label_expr = None;
    for (fname, fexpr) in fields {
        match fname.to_string().as_str() {
            "on_click" => on_click = Some(fexpr.clone()),
            "label" => label_expr = Some(fexpr.clone()),
            _ => {}
        }
    }
    let f = format_ident!("fc__{}", name);

    // `fc__btn` / `fc__label` 本身就返回 Slot，不要再包一层。
    if name == "btn" || name == "button" {
        let label_tok = label_expr
            .map(|e| to_label_closure(&e))
            .unwrap_or_else(|| quote! { || ::std::string::String::new() });
        let click_tok = on_click.unwrap_or_else(|| syn::parse_quote!(|| {}));
        return quote! { #m::fc::#f(#label_tok, #click_tok) };
    }
    if let Some(l) = label_expr {
        let ltok = to_label_closure(&l);
        return quote! { #m::fc::#f(#ltok) };
    }
    quote! { #m::fc::#f() }
}

fn to_label_closure(e: &Expr) -> TokenStream2 {
    match e {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => quote! { move || #s.to_string() },
        Expr::Closure(_) => quote! { #e },
        other => quote! { move || ::std::string::ToString::to_string(&(#other)) },
    }
}

// ============================================================================
// proc-macro 入口
// ============================================================================

/// 声明式 UI 宏，编译到 `wy_mve::fc`。
///
/// # Examples
///
/// ```ignore
/// use wy_mve::rsx;
///
/// rsx! {
///     vbox {
///         btn { label: "+ Add", on_click: move || { count.set(1); } }
///         label(count.get())
///         if count.get() > 0 {
///             label("有内容")
///         }
///     }
/// }
/// ```
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    let node = match syn::parse::<Node>(input) {
        Ok(n) => n,
        Err(e) => return e.to_compile_error().into(),
    };
    match gen_child(&node) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
