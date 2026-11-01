//! `#[rec(..)]` 自身属性的解析。
//!
//! 组件级只有三条指令：`arg = XxxProps` / `extends = base` / `omit(a, b)`。
//! 前两条带 `=`；`omit` 收一份**基类字段名**清单（它必须点名，因为 TS 的
//! `Omit<Base, 'k'>` 本身就是点名）。除这三条外，任何写法都会被 [`V2Attr`] 的
//! `Parse` 实现拒绝——字段语义一律写在字段自己身上（见 [`crate::props`]），
//! 避免「声明改了、清单忘了」。

use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    Ident, Token,
};

/// `#[rec(..)]` 的组件级指令。
#[derive(Default)]
pub(super) struct V2Attr {
    /// 产物改名：`#[rec(arg = Node2Props)]`。默认取 `XxxArg`——与既有手写
    /// 结构体撞名时才用得上（`node2` 现在就叫 `Node2Props`，用不上它）。
    pub arg_name: Option<Ident>,
    /// 继承：`#[rec(extends = node2)]`。组件自动获得基类的全部 props，
    /// 调用处写基类字段时由 `#[rec]` 生成的 `!` 宏**内联转发**给基类：基类
    /// 字段被直接展开成基类 props（如 `Node2Props`）的 builder 调用，收口成
    /// 一份 `Node2Props` 塞进自动注入的 `rest`，复用基类 getter
    /// 「属性即表达式」语义，等价于 TS 的 `...rest`。
    ///
    /// 组件内部再用 `node2! { ..rest, 覆盖项 }` 把这堆字段原样展开回去。
    pub extends: Option<Ident>,
    /// 继承之上的排除：`#[rec(extends = base, omit(spans))]`。
    ///
    /// 语义与 TS `interface A extends Omit<Base, 'k'>` 一致：`k` **不再是**基类
    /// 字段，`!` 宏不再把它转发进基类 builder，`rest` 里那一格也不再由调用方
    /// 或 `..rest` 决定——组件自己重新声明一份（可选：重声明为更宽松的类型），
    /// 并像 TS 的 `{ ...rest, k: .. }` 那样在自己的函数体里落地。
    pub omit: Vec<Ident>,
}

impl Parse for V2Attr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut a = V2Attr::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key == "omit" {
                // `omit(a, b)`：唯一的分组写法，因为 `Omit` 必须点名。
                if !input.peek(syn::token::Paren) {
                    return Err(syn::Error::new_spanned(
                        key,
                        "`omit` 要带括号并列出基类字段名：`omit(spans)`、`omit(spans, node_ref)`",
                    ));
                }
                let content;
                syn::parenthesized!(content in input);
                a.omit = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?
                    .into_iter()
                    .collect();
                if a.omit.is_empty() {
                    return Err(syn::Error::new_spanned(
                        key,
                        "`omit()` 是空的：要么写出字段名，要么删掉这一条",
                    ));
                }
            } else if input.peek(Token![=]) {
                input.parse::<Token![=]>()?;
                if key == "arg" {
                    a.arg_name = Some(input.parse()?);
                } else if key == "extends" {
                    a.extends = Some(input.parse()?);
                } else {
                    return Err(syn::Error::new_spanned(
                        key,
                        "`#[rec]` 仅支持 `arg = XxxProps` / `extends = base` / `omit(..)` 三种写法",
                    ));
                }
            } else {
                // 组件级不再有 `required(..)` 这类分组清单：字段的可传语义
                // （`#[getter]` / `#[props(default = ..)]` / `#[props(required)]`）
                // 一律写在**字段自己身上**，避免「声明改了、清单忘了」。
                return Err(syn::Error::new_spanned(
                    key,
                    "`#[rec]` 只接 `arg = XxxProps` / `extends = base` / `omit(..)`；\
                     字段语义请写在字段上：`#[getter]`、`#[props(default = ..)]`、\
                     `#[props(required)]`",
                ));
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(a)
    }
}
