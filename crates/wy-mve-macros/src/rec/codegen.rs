//! 产物代码生成：worker 签名 + `XxxArg` / `XxxArgBuilder` / `.build()`。
//!
//! 这里只负责「把 [`V2Field`] 照原样铺进 Rust 代码」，不再判断任何类型语义——
//! 存储类型、是否可选、缺省值都已在 [`V2Field::classify`] 里定好。

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{FnArg, Ident, ReturnType, Token, Type, Visibility};

use super::field::{is_string_type, V2Field, V2Setter};

/// 把原函数改写成 worker：参数换成存储类型、去掉宏自身的属性、返回类型原样保留。
///
/// 返回类型**不拦截**：写 `Fc` 就拿到 `Fc`，写 `LayoutDir2` 就拿到 `LayoutDir2`
/// （`flex!` 正是这样产出一条布局策略）。想包一层就自己在 body 里写。
pub(super) fn rewrite_worker(mut func: syn::ItemFn, fields: &[V2Field]) -> TokenStream2 {
    // 去掉宏自身的属性（tfc/fc/rec），避免 worker 被重复应用该宏。
    func.attrs.retain(
        |a| !matches!(a.path().get_ident(), Some(id) if id == "tfc" || id == "fc" || id == "rec"),
    );

    let mut inputs: Punctuated<FnArg, Token![,]> = Punctuated::new();
    for f in fields {
        let (id, st) = (&f.ident, &f.stored);
        let arg: FnArg = syn::parse_quote!(#id: #st);
        inputs.push(arg);
    }
    func.sig.inputs = inputs;
    quote! { #func }
}

/// `-> T` 里的 `T`；无返回类型时给 `()`。
pub(super) fn ret_ty(sig: &syn::Signature) -> TokenStream2 {
    match &sig.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, t) => quote!(#t),
    }
}

/// 生成 `XxxArg`（+ 可能有的 `Default`）+ `XxxArgBuilder` + 各 setter + `.build()`
/// + `XxxArg::builder()` / `XxxArg::build()`。
pub(super) fn gen_props(
    func: &Ident,
    props: &Ident,
    vis: &Visibility,
    pascal: &str,
    ret: &TokenStream2,
    fields: &[V2Field],
) -> TokenStream2 {
    let builder = format_ident!("{}Builder", props);
    let field_idents: Vec<&Ident> = fields.iter().map(|f| &f.ident).collect();

    let props_fields = fields.iter().map(|f| {
        let (id, t) = (&f.ident, &f.stored);
        quote! { pub #id: #t }
    });
    let builder_fields = fields.iter().map(|f| {
        let (id, t) = (&f.ident, &f.stored);
        quote! { pub #id: ::core::option::Option<#t> }
    });
    let setters = fields.iter().map(|f| {
        let base = setter_tokens(f);
        let push = push_setter_tokens(f);
        quote!(#base #push)
    });
    let build_fields = fields.iter().map(|f| {
        let id = &f.ident;
        if f.optional {
            let d = &f.default;
            quote! { #id: self.#id.unwrap_or_else(|| #d) }
        } else {
            let msg = format!("{pascal} 缺少必填字段 `{id}`");
            quote! { #id: self.#id.expect(#msg) }
        }
    });

    let has_required = fields.iter().any(|f| !f.optional);
    let default_impl = if !has_required {
        let ds = fields.iter().map(|f| {
            let (id, d) = (&f.ident, &f.default);
            quote! { #id: #d }
        });
        quote! {
            impl ::core::default::Default for #props {
                fn default() -> Self {
                    Self { #(#ds),* }
                }
            }
        }
    } else {
        quote! {}
    };

    // `rec` 唯一的对外方法：`.build()` 产出**函数签名上写的那个返回类型**。
    // 没有 of / create / mount —— 返回值怎么写完全由函数体决定。
    //
    // 两个方法都 `#[doc(hidden)]`：它们是 `xxx! { .. }` 宏展开的**机械通道**
    // （宏展开就是 `Props::builder() .. .build().build()`），不是给人和 AI
    // 手写的入口。手写 builder 链属于过程式装配，见 `docs/08` §1。
    let rec_impl = quote! {
        impl #props {
            /// 供 `xxx! { .. }` 宏展开使用，**不要手写**（写法见 `docs/08` §1）。
            #[doc(hidden)]
            #[must_use] pub fn builder() -> #builder {
                <#builder as ::core::default::Default>::default()
            }
            /// 供 `xxx! { .. }` 宏展开使用，**不要手写**。
            #[doc(hidden)]
            #[must_use] pub fn build(self) -> #ret {
                let #props { #(#field_idents),* } = self;
                #func(#(#field_idents),*)
            }
        }
    };

    quote! {
        #vis struct #props {
            #(#props_fields,)*
        }

        #default_impl

        #[derive(::core::default::Default)]
        #vis struct #builder {
            #(#builder_fields,)*
        }

        impl #builder {
            #(#setters)*

            /// 以一份现成的 props 为底（`node2! { ..rest, .. }` 的展开点）。
            ///
            /// 逐字段搬进 builder 的「已给」槽，于是后续 setter 自然覆盖、没被覆盖
            /// 的字段保持继承值——与 TS `{ ...rest, a: 1 }` 同序同义。
            #[must_use] pub fn from_props(mut self, src: #props) -> Self {
                let #props { #(#field_idents),* } = src;
                #(self.#field_idents = ::core::option::Option::Some(#field_idents);)*
                self
            }

            #[must_use] pub fn build(self) -> #props {
                #props {
                    #(#build_fields,)*
                }
            }
        }

        #rec_impl
    }
}

/// 单个字段的 setter。
fn setter_tokens(f: &V2Field) -> TokenStream2 {
    let id = &f.ident;
    match &f.setter {
        V2Setter::Plain => {
            let t = &f.stored;
            quote! {
                #[must_use] pub fn #id(mut self, v: #t) -> Self {
                    self.#id = ::core::option::Option::Some(v);
                    self
                }
            }
        }
        V2Setter::Getter(inner) => {
            let (gens, preds, conv) = getter_conv(inner);
            quote! {
                /// 收**表达式**（宏已包成 `move || 表达式`），每次求值重算。
                #[must_use] pub fn #id #gens (mut self, v: __F) -> Self
                where #preds {
                    self.#id = ::core::option::Option::Some(::std::rc::Rc::new(move || #conv));
                    self
                }
            }
        }
        V2Setter::GetterOpt(inner) => {
            // 未声明即 `None`：builder 字段是 `Option<Option<Get<T>>>`（再一层表示
            // "写没写"）。被 `#[props(required)]` 提升为必填后，存储已剥掉外层 Option，
            // 这里也就只包一层 `Some`。
            let (gens, preds, conv) = getter_conv(inner);
            let store: TokenStream2 = quote!(::std::rc::Rc::new(move || #conv));
            let wrap: TokenStream2 = if f.optional {
                quote!(::core::option::Option::Some(#store))
            } else {
                store
            };
            quote! {
                /// 收**表达式**（宏已包成 `move || 表达式`），每次求值重算。
                #[must_use] pub fn #id #gens (mut self, v: __F) -> Self
                where #preds {
                    self.#id = ::core::option::Option::Some(#wrap);
                    self
                }
            }
        }
        V2Setter::OptInto(inner) => quote! {
            #[must_use] pub fn #id<__V: ::core::convert::Into<#inner>>(mut self, v: __V) -> Self {
                self.#id = ::core::option::Option::Some(
                    ::core::option::Option::Some(::core::convert::Into::into(v)),
                );
                self
            }
        },
        V2Setter::Closure(bounds) => {
            // 可选字段的 props 类型是 `Option<Rc<dyn F>>`（builder 字段是它的
            // 再一层 Option），必填字段则剥掉外层 Option——worker 直接拿到
            // `Rc<dyn F>`，可以 `cb()` 调用，不必到处 `unwrap()`。
            let wrap: TokenStream2 = if f.optional {
                quote!(::core::option::Option::Some(::std::rc::Rc::new(v)))
            } else {
                quote!(::std::rc::Rc::new(v))
            };
            quote! {
                #[must_use] pub fn #id<__F: #bounds + 'static>(mut self, v: __F) -> Self {
                    self.#id = ::core::option::Option::Some(#wrap);
                    self
                }
            }
        }
    }
}

/// `Vec<T>` 字段的追加 setter（`<字段>_push`）。
///
/// 覆盖式 setter 只能"换掉整条列表"，而组件在继承来的列表上加自己的一项
/// （`node2! { ..rest, exts_push: e }`）必须能**追加**。收的是确切元素类型：
/// `Vec<Rc<dyn Any>>` 的元素无法从 `Rc<具体类型>` 经 `Into` 转过来（unsize 不是
/// `From`），泛型 `Into` 在这里反而挡路。
fn push_setter_tokens(f: &V2Field) -> TokenStream2 {
    let Some(elem) = &f.vec_elem else {
        return quote!();
    };
    let id = &f.ident;
    let push_id = format_ident!("{}_push", id);
    quote! {
        /// 在已有列表上**追加**一项（覆盖式 setter 之后写也能生效）。
        #[must_use] pub fn #push_id(mut self, v: #elem) -> Self {
            self.#id.get_or_insert_with(::std::vec::Vec::new).push(v);
            self
        }
    }
}

/// getter setter 的泛型声明、`where` 谓词与存储转换。
///
/// 文本字段收 `Into<String>`：`text: "标题"` 直接写字面量，不必 `"标题".to_string()`
/// （`&str → String` 在**每次求值**时才发生，不影响懒求值语义）。
///
/// 其余字段收 `Fn() -> T` **精确类型**：数值字面量靠这条约束推断成 `f32`
/// （若写成 `__S: Into<f32>`，`8.0` 会先判成 `f64` 再回退，触发
/// `float_literal_f32_fallback` —— 眼下是告警，日后是硬错）。
fn getter_conv(inner: &Type) -> (TokenStream2, TokenStream2, TokenStream2) {
    if is_string_type(inner) {
        (
            quote!(<__F, __S>),
            quote!(
                __F: ::core::ops::Fn() -> __S + 'static,
                __S: ::core::convert::Into<#inner> + 'static,
            ),
            quote!(::core::convert::Into::into(v())),
        )
    } else {
        (
            quote!(<__F>),
            quote!(__F: ::core::ops::Fn() -> #inner + 'static,),
            quote!(v()),
        )
    }
}
