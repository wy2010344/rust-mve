//! 参数属性解析（`#[props(..)]` / `#[getter]`）与类型判定，供
//! 只服务 `#[rec]`。
//!
//! # 注解体系（三条正交原则）
//!
//! 每个字段的可传语义**只写在该字段自己身上**，不在组件级维护任何清单——
//! 属性跟着字段走，改字段时注解就在眼前，不会「声明改了、清单忘了」：
//!
//! | 注解 | 管什么 | 例子 |
//! |---|---|---|
//! | `#[getter]` | **怎么读**：每次求值重算（`Get<T>`），属性当表达式写 | `#[getter] width: f32` |
//! | `#[props(default = ..)]` | **缺省值**：没写时用什么 | `#[props(default = 16.0)] font_size: f32` |
//! | `#[props(required)]` | **强制必填**：按类型本可省略，但业务上必须给 | `#[props(required)] on_click: impl Fn()` |
//!
//! 三者正交，但 `default` 与 `required` **互斥**（有默认值就不可能是必填），
//! 同时写会直接报编译错误，而不是让其中一个静默失效。
//!
//! 「没写 `default` 就是必填」这条对**普通字段**自动成立；`required` 专门用于
//! 类型本身带「可省略」语义的四类字段：`impl Fn(..)`、`Option<T>`、`Vec<T>`、
//! 以及无默认的 `#[getter] T`（存成 `Option<Get<T>>` 以表达「未声明」）。

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::spanned::Spanned;

/// 一个字段的 `#[props(..)]` 语义。
#[derive(Default)]
pub(crate) struct PropsAttr {
    /// 缺省值表达式；`None` = 没写。
    pub default: Option<TokenStream2>,
    /// 强制必填：把「按类型可省略」的字段拉回必填。
    pub required: bool,
}

/// 解析参数上的 `#[props(..)]`：`default` / `default = expr` / `required` 可混写，
/// 但 `default` 与 `required` 互斥。
pub(crate) fn parse_props_attr(attrs: &[syn::Attribute]) -> syn::Result<PropsAttr> {
    let mut out = PropsAttr::default();
    let mut required_span: Option<proc_macro2::Span> = None;

    for attr in attrs {
        if !attr.path().is_ident("props") {
            continue;
        }
        let meta = attr.parse_args::<syn::Meta>()?;
        match meta {
            syn::Meta::Path(ref p) if p.is_ident("default") => {
                if out.default.is_some() {
                    return Err(syn::Error::new_spanned(
                        p,
                        "`#[props(default ..)]` 重复：一个字段只能有一个缺省值",
                    ));
                }
                out.default = Some(quote! { ::core::default::Default::default() });
            }
            syn::Meta::Path(ref p) if p.is_ident("required") => {
                out.required = true;
                required_span = Some(p.span());
            }
            syn::Meta::NameValue(ref nv) if nv.path.is_ident("default") => {
                if out.default.is_some() {
                    return Err(syn::Error::new_spanned(
                        &nv.path,
                        "`#[props(default ..)]` 重复：一个字段只能有一个缺省值",
                    ));
                }
                let e = nv.value.clone();
                out.default = Some(quote! { #e });
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "`#[props(..)]` 仅支持 `default` / `default = expr` / `required`；\
                     读取式语义请用独立的 `#[getter]`",
                ));
            }
        }
    }

    if out.required {
        if let Some(d) = &out.default {
            // 互斥：有默认值就不存在「必填」。与其让其中一个静默失效，不如直接报错。
            return Err(syn::Error::new(
                required_span.unwrap_or_else(proc_macro2::Span::call_site),
                format!(
                    "`#[props(required)]` 与 `#[props(default = {d})]` 互斥：\
                     有默认值的字段不可能是必填"
                ),
            ));
        }
    }
    Ok(out)
}

/// 解析参数上的 `#[getter]`：标记该 prop 为「显示值」语义。
///
/// 返回 `true` 表示标注了。接受 `#[getter]`（路径形式）；若写成 `#[getter(...)]`
/// 也容忍（忽略参数），避免调用方纠结括号。
pub(crate) fn parse_getter(attrs: &[syn::Attribute]) -> syn::Result<bool> {
    for attr in attrs {
        if attr.path().is_ident("getter") {
            return Ok(true);
        }
    }
    Ok(false)
}
