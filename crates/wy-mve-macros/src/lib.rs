//! `rsx!` 声明式 UI 宏：对齐 Dioxus `rsx!` 风格。
//!
//! 语法速览：
//! - 容器：`vbox { child1 child2 }` —— 子块内平铺，无需逗号/括号
//! - 属性：`vbox { gap: 12.0, width: w.get(), hide: empty.get(), btn { .. } }`
//!   —— `name: expr` 是属性（通用布局属性直接映射 [`Node`] 字段），其余是子节点。
//!   非字面量表达式由宏包成 `move || expr` 闭包，求值发生在布局/绘制期，
//!   读信号自动追踪（复刻 Kotlin「属性是 getter」的形态）。
//! - 组件：`btn { label: "x", on_click: move || { ... } }` —— 字段语法绑事件
//! - 自定义组件：`#[component] fn foo(..)` 后写 `foo { field: v, .. }`
//!   会展开为 `foo(FooProps { field: v, .. })`（见 [`component`]）
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
    ///
    /// `stmts` 是块内混入的 `let` 等语句（不产出节点，组件调用前求值）。
    Component {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
        stmts: Vec<Stmt>,
    },
    /// 静态文本节点（裸字面量）
    Text(LitStr),
    /// 动态组件/文本调用：`label(expr)`
    Dynamic(Expr),
    /// 条件：`if cond { .. } else { .. }`（**构建期**静态分支）
    Conditional {
        cond: Expr,
        then_branch: Vec<Node>,
        else_branch: Option<Vec<Node>>,
    },
    /// 反应式条件分支：`when(cond) { .. } else { .. }`（`cond` 为信号读取，动态切换）
    When {
        cond: Expr,
        then_branch: Vec<Node>,
        else_branch: Vec<Node>,
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

        // when(cond) { A } else { B }：反应式条件分支（cond 为信号读取）
        if name == "when" {
            let args = if input.peek(syn::token::Paren) {
                parse_paren_args(input)?
            } else {
                return Err(syn::Error::new(
                    name.span(),
                    "when 需要 `when(cond) { .. } else { .. }`",
                ));
            };
            if args.len() != 1 {
                return Err(syn::Error::new(
                    name.span(),
                    "when 只接受一个条件：`when(cond) { .. } else { .. }`",
                ));
            }
            let then_branch = parse_nodes_block(input)?;
            let else_branch = if input.peek(Token![else]) {
                let _: Token![else] = input.parse()?;
                if input.peek(syn::token::Brace) {
                    parse_nodes_block(input)?
                } else {
                    // `else if ..` / `else when(..)`：单节点子分支
                    vec![input.parse::<Node>()?]
                }
            } else {
                Vec::new()
            };
            return Ok(Node::When {
                cond: args[0].clone(),
                then_branch,
                else_branch,
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
            // 分离 let 等原始语句：只有真子节点参与 Component/Container 判定
            let mut real_children = Vec::new();
            let mut stmts = Vec::new();
            for c in &children {
                match c {
                    Node::Raw(s) => stmts.extend(s.iter().cloned()),
                    other => real_children.push(other),
                }
            }
            // 无真子节点且有字段 → 组件（btn/label 的字段语法）；
            // 否则 → 容器（字段是通用布局属性，可与子节点共存）
            if real_children.is_empty() && !fields.is_empty() {
                Ok(Node::Component {
                    name,
                    fields,
                    stmts,
                })
            } else {
                // 容器路径：语句保持在 children 中的原位（已按序落盘）
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
        Node::When {
            cond,
            then_branch,
            else_branch,
        } => {
            let then_expr = gen_children_vec(then_branch)?;
            let else_expr = if else_branch.is_empty() {
                quote! { ::std::vec::Vec::<#m::fc::Slot>::new() }
            } else {
                gen_children_vec(else_branch)?
            };
            Ok(quote! {
                #m::fc::when(
                    move || #cond,
                    move |__cond, __cx| {
                        let __slots: ::std::vec::Vec<#m::fc::Slot> = if __cond {
                            #then_expr
                        } else {
                            #else_expr
                        };
                        #m::fc::push_all(__slots, __cx);
                    },
                )
            })
        }
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
        Node::Component {
            name,
            fields,
            stmts,
        } => gen_component(name, fields, stmts),
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

/// 组件参数规格：必填位置参数 + 可选 opts 记录式参数。
///
/// 复刻 Kotlin「具名参数 + 默认值」。宏无法从字段名推断 Rust 类型，故用
/// 静态表声明：哪些字段是必填位置参数，其余字段收集进 `opts` 记录式结构体
/// （`..Default::default()` 兜底，复用各 Opts 的 `impl Default`）。
struct ComponentSpec {
    /// 必填位置参数字段名（按顺序传给 `fc__<name>`）。
    ///
    /// 值在生成时经 [`to_param_closure`] 转成闭包。
    required: &'static [&'static str],
    /// 可选 opts 结构体类型路径（记录式参数收集桶）；`None` 表示无 opts。
    ///
    /// 为 `Some` 时，`fc__<name>` 签名末位必须是该 opts 类型。
    opts_ty: Option<&'static str>,
}

/// 组件名 → 规格。未注册的标签仍按旧规则（label 闭包 / 无参）生成。
fn component_spec(name: &str) -> Option<ComponentSpec> {
    match name {
        "btn" | "button" => Some(ComponentSpec {
            required: &["label", "on_click"],
            opts_ty: None,
        }),
        "text_field" => Some(ComponentSpec {
            required: &["value", "on_change"],
            opts_ty: Some("TextFieldOpts"),
        }),
        "rich_text" => Some(ComponentSpec {
            required: &["value", "on_change"],
            opts_ty: Some("RichTextOpts"),
        }),
        _ => None,
    }
}

/// 生成组件调用表达式。
///
/// - 已注册组件（[`component_spec`]）：按规格收集必填位置参数与 opts 记录字段；
///   未知字段报编译错误（不再静默丢弃）。
/// - 未注册组件：旧规则（有 `label` 则传 label 闭包，否则无参），未知字段报错。
/// - `stmts` 是块内混入的 `let` 等语句，先求值再构造组件。
fn gen_component(
    name: &Ident,
    fields: &[(Ident, Expr)],
    stmts: &[Stmt],
) -> syn::Result<TokenStream2> {
    let m = mve();
    let fname = name.to_string();

    // 已注册组件：按规格生成
    if let Some(spec) = component_spec(&fname) {
        let f = format_ident!("fc__{}", name);
        let mut positionals: Vec<TokenStream2> = Vec::new();
        let mut opts_fields: Vec<(Ident, Expr)> = Vec::new();

        for req in spec.required {
            let Some((_, expr)) = fields.iter().find(|(n, _)| n == req) else {
                // btn/button 历史兼容：缺 label/on_click 用空闭包兜底
                if matches!(fname.as_str(), "btn" | "button") {
                    positionals.push(match *req {
                        "label" => quote! { || ::std::string::String::new() },
                        "on_click" => quote! { || {} },
                        _ => unreachable!("btn 必填参数只有 label/on_click"),
                    });
                    continue;
                }
                return Err(syn::Error::new(
                    name.span(),
                    format!("组件 `{name}` 缺少必填参数 `{req}`"),
                ));
            };
            positionals.push(to_param_closure(req, expr));
        }

        for (n, e) in fields {
            if spec.required.contains(&n.to_string().as_str()) {
                continue;
            }
            opts_fields.push((n.clone(), e.clone()));
        }

        let call = match spec.opts_ty {
            Some(opts_ty) => {
                let opts_ty_path = format_ident!("{}", opts_ty);
                let opts_init = if opts_fields.is_empty() {
                    quote! { #m::#opts_ty_path::default() }
                } else {
                    let assigns = opts_fields.iter().map(|(n, e)| quote! { #n: #e });
                    quote! { #m::#opts_ty_path { #(#assigns,)* ..::std::default::Default::default() } }
                };
                quote! { #m::fc::#f(#(#positionals,)* #opts_init) }
            }
            None => quote! { #m::fc::#f(#(#positionals),*) },
        };

        // 所有 fc__* 别名均返回 Slot，无需再包。
        if stmts.is_empty() {
            return Ok(call);
        }
        return Ok(quote! {{
            #(#stmts)*
            #call
        }});
    }

    // 未注册组件：按 `#[component]` 约定调用用户组件。
    //
    // `#[component] fn stat_card(..)` 会生成 `struct StatCardProps` + 同名函数
    // `fn stat_card(props: StatCardProps) -> Slot`。故 `statCard { .. }` 展开为
    // `stat_card(StatCardProps { .. })`。字段名/类型由用户签名的 Props 决定。
    let props_name = format_ident!("{}Props", to_pascal_case(&fname));
    let assigns = fields.iter().map(|(n, e)| quote! { #n: #e });
    let call = quote! { #name(#props_name { #(#assigns,)* }) };

    if stmts.is_empty() {
        return Ok(call);
    }
    Ok(quote! {{
        #(#stmts)*
        #call
    }})
}

/// `snake_case` → `PascalCase`（用于组件名 → Props 结构体名）。
///
/// 例：`stat_card` → `StatCard`，`text_field` → `TextField`。
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// 必填参数 → 闭包：`label`/`value` 走字符串字面量优化，其余透传闭包。
fn to_param_closure(field: &str, e: &Expr) -> TokenStream2 {
    match field {
        "label" => to_label_closure(e),
        // value 闭包：字面量包 to_string，闭包透传，其余包 move ||
        "value" => to_label_closure(e),
        // on_change 等回调：必须是闭包，透传
        _ => quote! { #e },
    }
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
    // 根级支持混入普通 Rust 语句（`let` 等）：解析前缀语句 + 单个根节点，
    // 生成块先求值语句再产出节点。对齐 Kotlin `composable` 内的局部变量。
    let (stmts, node) = match syn::parse::<RsxRoot>(input) {
        Ok(r) => (r.stmts, r.node),
        Err(e) => return e.to_compile_error().into(),
    };
    let body = match gen_child(&node) {
        Ok(ts) => ts,
        Err(e) => return e.to_compile_error().into(),
    };
    if stmts.is_empty() {
        body.into()
    } else {
        quote! {{
            #(#stmts)*
            #body
        }}
        .into()
    }
}

/// `rsx!` 根：前缀 Rust 语句 + 单个根节点。
struct RsxRoot {
    stmts: Vec<Stmt>,
    node: Node,
}

impl Parse for RsxRoot {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut stmts = Vec::new();
        loop {
            skip_junk(input);
            if input.is_empty() {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "rsx! 至少需要一个根节点",
                ));
            }
            // `let` 语句 → 前缀；否则进入节点解析
            if input.peek(Token![let]) {
                stmts.push(input.parse::<Stmt>()?);
                continue;
            }
            break;
        }
        let node = input.parse::<Node>()?;
        Ok(RsxRoot { stmts, node })
    }
}

// ============================================================================
// #[component] 属性宏
// ============================================================================

/// 把普通函数包装成可在 `rsx!` 中按 `name { field: value, .. }` 使用的组件。
///
/// 生成两部分：
/// - `struct <Name>Props { .. }`：字段来自函数参数（同名同类型，`pub`）。
/// - `fn <name>(props: <Name>Props) -> Slot`：解构 Props 后执行原函数体，
///   返回值经 `Into<Slot>` 转换，故原函数可返回 `Node` 或 `Slot`。
///
/// `rsx!` 里写 `stat_card { title: "x", value: 1.0, children: vec![..] }`
/// 会展开为 `stat_card(StatCardProps { title: "x", value: 1.0, children: vec![..] })`。
///
/// # 约束
/// - 参数须有**具体类型**（不支持 `impl Trait`/泛型/生命周期）；闭包请用
///   `Rc<dyn Fn() -> String>` 之类具体类型。
/// - 不支持 `self`、`async`、`const`、`unsafe`、泛型。
/// - `rsx!` 组件语法无「裸子节点」，children 需作为字段显式传入
///   （如 `children: vec![label("x")]`）。
///
/// # Examples
///
/// ```ignore
/// use wy_mve::{component, rsx, vbox, Node, Slot};
///
/// #[component]
/// fn stat_card(title: String, value: f32, children: Vec<Slot>) -> Node {
///     let mut row = vbox(children);
///     row.gap = value;
///     row
/// }
///
/// let ui = rsx! {
///     stat_card {
///         title: "总访问".to_string(),
///         value: 8.0,
///         children: Vec::<Slot>::new(),
///     }
/// };
/// ```
#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    match expand_component(item.into()) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_component(item: TokenStream2) -> syn::Result<TokenStream2> {
    let m = mve();
    let func: syn::ItemFn = syn::parse2(item)?;
    let sig = &func.sig;

    if sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            sig,
            "#[component] 不支持 async 函数",
        ));
    }
    if sig.constness.is_some() {
        return Err(syn::Error::new_spanned(
            sig,
            "#[component] 不支持 const 函数",
        ));
    }
    if sig.unsafety.is_some() {
        return Err(syn::Error::new_spanned(
            sig,
            "#[component] 不支持 unsafe 函数",
        ));
    }
    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            "#[component] 不支持泛型/生命周期参数；请用具体类型",
        ));
    }

    let vis = &func.vis;
    let name = &sig.ident;
    let props_name = format_ident!("{}Props", to_pascal_case(&name.to_string()));

    // 收集参数：字段名 + 类型（拒绝 self / impl Trait / 复杂模式）。
    let mut field_names: Vec<Ident> = Vec::new();
    let mut field_tys: Vec<syn::Type> = Vec::new();
    for arg in &sig.inputs {
        let pat_ty = match arg {
            syn::FnArg::Receiver(r) => {
                return Err(syn::Error::new_spanned(
                    r,
                    "#[component] 不支持 self 参数（组件是自由函数）",
                ));
            }
            syn::FnArg::Typed(pt) => pt,
        };
        let ident = match &*pat_ty.pat {
            syn::Pat::Ident(pi) if pi.subpat.is_none() => pi.ident.clone(),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "#[component] 参数须是简单标识符（如 `title: String`）",
                ));
            }
        };
        if matches!(&*pat_ty.ty, syn::Type::ImplTrait(_)) {
            return Err(syn::Error::new_spanned(
                &pat_ty.ty,
                "#[component] 暂不支持 `impl Trait` 参数；请改用具体类型\
                 （如 `Rc<dyn Fn() -> String>`）",
            ));
        }
        field_names.push(ident);
        field_tys.push((*pat_ty.ty).clone());
    }

    let attrs = &func.attrs;
    let body = &func.block;

    Ok(quote! {
        #(#attrs)*
        #vis struct #props_name {
            #( pub #field_names: #field_tys, )*
        }

        #(#attrs)*
        #vis fn #name(props: #props_name) -> #m::Slot {
            let #props_name { #(#field_names),* } = props;
            ::core::convert::Into::<#m::Slot>::into(#body)
        }
    })
}
