//! 一个参数如何变成字段：类型判定 + 存储形态推导。
//!
//! 这是 `#[rec]` 唯一的「语义判定」处——函数签名里写什么类型，就决定了
//! `XxxArg` 的存储类型、builder setter 收什么、缺省值是什么。其余模块只做
//! 代码生成，不再解释类型。

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{GenericArgument, Ident, Pat, PathArguments, Token, Type, TypeParamBound};

use crate::props::{parse_getter, parse_props_attr};

/// 签名里的一个参数（推导完成后）。
pub(super) struct V2Field {
    pub ident: Ident,
    /// 在 `XxxProps` 与 worker 函数里的存储类型（body 按此类型使用）。
    pub stored: Type,
    pub optional: bool,
    pub default: TokenStream2,
    pub setter: V2Setter,
    /// `Vec<T>` 的元素类型：`Some` 时 builder 额外生成 `<字段>_push`（追加而不是
    /// 覆盖），`node2! { ..rest, exts_push: e }` 这类"在继承来的列表上再加一项"
    /// 才有落点。
    pub vec_elem: Option<Type>,
}

impl V2Field {
    /// 该字段在 `!` 宏里要不要包 `move ||`（即是否为 getter 语义）。
    pub(super) fn is_getter(&self) -> bool {
        matches!(self.setter, V2Setter::Getter(_) | V2Setter::GetterOpt(_))
    }
}

pub(super) enum V2Setter {
    /// setter 泛型收 `impl Fn..` 闭包。
    Closure(TokenStream2),
    /// getter（强制必填）：setter 泛型收 `F: Fn() -> Inner`，字段存 `Get<Inner>`。
    Getter(Type),
    /// getter（可选，默认 `None`）：字段存 `Option<Get<Inner>>`，缺失即 `None`
    /// （让引擎走"自动"兜底，而非冻结成默认值）。
    GetterOpt(Type),
    /// `Option<T>` 字段：setter 收**裸 `T`**（`impl Into<T>`），自动包一层 `Some`。
    ///
    /// 这样调用点写 `layout: flex!{..}` 而不是 `layout: Some(flex!{..})`——
    /// `Option` 只是「没写」的内部表示，不该泄漏到调用语法里。
    /// `Into` 让同一个位置既能收裸值（`|| 10.0` → `Get<f32>`），也能收现成的
    /// `Rc<dyn Fn..>`（reflexive `From`）。
    OptInto(Type),
    /// 直接收存储类型。
    Plain,
}

impl V2Field {
    /// 从一个 `name: Type` 参数推导字段形态。
    ///
    /// `m` 是 `wy_mve` 的 crate 路径（`$crate` / `::wy_mve`），用于生成
    /// `Vec<#m::Fc>` / `#m::always(..)` 这类绝对路径。
    pub(super) fn classify(pt: &syn::PatType, m: &TokenStream2) -> syn::Result<Self> {
        let ident = match &*pt.pat {
            Pat::Ident(pi) if pi.subpat.is_none() => pi.ident.clone(),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "tfc/fc/rec 参数须是简单标识符",
                ))
            }
        };
        let ty = &*pt.ty;

        let is_children = ident == "children";
        let props = parse_props_attr(&pt.attrs)?;
        let props_default = props.default;
        let is_required = props.required;
        let is_getter = parse_getter(&pt.attrs)?;

        let (stored, optional, default, setter) = if is_children {
            // 子节点槽：body 里当 `Vec<Fc>` 用，与节点的 children 同型。
            let t: Type = syn::parse_quote!(::std::vec::Vec<#m::Fc>);
            (t, true, quote!(::std::vec::Vec::new()), V2Setter::Plain)
        } else if is_getter {
            let inner: Type = (*pt.ty).clone();
            let def = props_default.clone();
            // getter 字段的三种形态——**能用默认值就不用 Option**：
            //
            // ① 声明 `Option<U>`：getter 返回 `Option<U>`，缺省即 `None`
            //    （对齐 Kotlin `val focusOrder: Int? = null`）。只留**一层** Option，
            //    语义是"这个值现在是不是 null"，不是"这个 prop 有没有写"。
            // ② 声明 `T` 且给了默认值（`#[props(default = ..)]`）：
            //    存 `Get<T>`，缺省取默认值。worker 里直接 `f()`，不必判空。
            // ③ 声明 `T` 且没默认值：存 `Option<Get<T>>`，`None` = 未声明
            //    （`width` / `height` 这类"没写就交给布局"的字段）。
            if let Some(u) = option_inner(ty) {
                let stored_ty: Type =
                    syn::parse_quote!(::std::rc::Rc<dyn ::core::ops::Fn() -> #inner>);
                // 写了 `#[props(default = ..)]` 就以它为准（`always(Some(1.4))`），
                // 否则才落 `always(None)`。先前这里硬写死 `None`，使 getter 上的
                // default 被静默吞掉。
                let default = match &def {
                    Some(d) => quote!(#m::always(#d)),
                    None => quote!(#m::always(::core::option::Option::<#u>::None)),
                };
                (stored_ty, true, default, V2Setter::Getter(inner))
            } else if let Some(d) = &def {
                let stored_ty: Type =
                    syn::parse_quote!(::std::rc::Rc<dyn ::core::ops::Fn() -> #inner>);
                (
                    stored_ty,
                    true,
                    quote!(#m::always(#d)),
                    V2Setter::Getter(inner),
                )
            } else {
                let stored_ty: Type = syn::parse_quote!(
                    ::core::option::Option<::std::rc::Rc<dyn ::core::ops::Fn() -> #inner>>
                );
                (
                    stored_ty,
                    true,
                    quote!(::core::option::Option::None),
                    V2Setter::GetterOpt(inner),
                )
            }
        } else if let Some(it) = as_impl_fn(ty) {
            let trait_bounds: Vec<&TypeParamBound> = it
                .iter()
                .filter(|b| matches!(b, TypeParamBound::Trait(_)))
                .collect();
            let bound_tokens: TokenStream2 = quote! { #(#trait_bounds)+* };
            let stored_ty: Type =
                syn::parse_quote!(::core::option::Option<::std::rc::Rc<dyn #(#trait_bounds)+*>>);
            (
                stored_ty,
                true,
                quote!(::core::option::Option::None),
                V2Setter::Closure(bound_tokens),
            )
        } else if let Some(inner) = option_inner(ty) {
            // `Option<T>` ——「没写」的内部表示。stored 仍是 `Option<T>`（worker
            // 直接用），但 setter 收裸 `T`。
            //
            // 显式写了 `#[props(default = ..)]` 就以它为准——stored 是裸
            // `Option<T>`，所以 default 也是裸值（`Some(1.4)` / `None`）。
            // **这里曾漏掉对 `props_default` 的尊重**，导致 Option 字段上的
            // default 被静默忽略、一律落 `None`。
            let default = match &props_default {
                Some(d) => quote!(#d),
                None => quote!(::core::option::Option::None),
            };
            ((*pt.ty).clone(), true, default, V2Setter::OptInto(inner))
        } else if is_vec_type(ty) {
            (
                (*pt.ty).clone(),
                true,
                quote!(::std::vec::Vec::new()),
                V2Setter::Plain,
            )
        } else {
            let def = props_default.clone();
            if let Some(d) = &def {
                ((*pt.ty).clone(), true, d.clone(), V2Setter::Plain)
            } else {
                ((*pt.ty).clone(), false, quote!(), V2Setter::Plain)
            }
        };
        // 只有**值语义**的 `Vec<T>` 才有"追加"可言：`#[getter]` 的 `Vec<T>` 存的是
        // `Option<Get<Vec<T>>>`（每次求值重算），往一个闭包里 push 没有意义。
        let vec_elem = if is_children {
            Some(syn::parse_quote!(#m::Fc))
        } else if is_getter {
            None
        } else {
            vec_inner(ty)
        };
        // 字段级 `#[props(required)]`：把任意「按类型本可省略」的字段（`impl Fn` /
        // `Option<T>` / `Vec<T>` / 无默认 getter）拉回必填——build 时必须显式给出，
        // 缺失即 panic（附字段名）。属性写在字段自己身上，不做组件级清单。
        let (stored, optional, default) = if is_required {
            // 回调字段顺带剥掉那层 `Option`：必填的回调不该在 worker 里再判空。
            let stored = match &setter {
                V2Setter::Closure(bounds) => {
                    syn::parse_quote!(::std::rc::Rc<dyn #bounds>)
                }
                // `Option<T>` 必填时同样剥掉外层 Option，worker 直接拿 `T`。
                V2Setter::OptInto(inner) => inner.clone(),
                // getter 必填时剥掉外层 `Option`：worker 直接拿 `Rc<dyn Fn>`。
                V2Setter::GetterOpt(inner) => {
                    syn::parse_quote!(::std::rc::Rc<dyn ::core::ops::Fn() -> #inner>)
                }
                _ => stored,
            };
            (stored, false, quote!())
        } else {
            (stored, optional, default)
        };

        Ok(Self {
            ident,
            stored,
            optional,
            default,
            setter,
            vec_elem,
        })
    }

    /// `extends = base` 注入的 `rest` 字段：由宏把未知 key 转发给基类后填进来。
    ///
    /// 它会自动进入 worker 签名（body 直接可用），也进入 props / builder / build
    /// ——调用方完全不需要知道它的存在。
    ///
    /// 类型是**基类的 props**（`extends = node2` → `Node2Props`），不是某个内部
    /// IR：这样组件内部能写 `node2! { ..rest, focusable: true }`，把继承来的字段
    /// 原样展开、再覆盖自己要改的那几个——TS 里 `{ ...rest, focusable: true }`
    /// 的同一件事。
    pub(super) fn rest(base_props_ty: &Ident) -> Self {
        Self {
            ident: Ident::new("rest", proc_macro2::Span::call_site()),
            stored: syn::parse_quote!(#base_props_ty),
            optional: true,
            default: quote!(#base_props_ty::default()),
            setter: V2Setter::Plain,
            vec_elem: None,
        }
    }
}

/// 提取 `Option<T>` 的内层 `T`。
fn option_inner(ty: &Type) -> Option<Type> {
    let Type::Path(tp) = ty else {
        return None;
    };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(a) = &seg.arguments else {
        return None;
    };
    match a.args.first()? {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    }
}

/// 识别 `impl Fn(..) -> R (+ 'static)`，返回它的 trait bounds 列表。
fn as_impl_fn(ty: &Type) -> Option<&Punctuated<TypeParamBound, Token![+]>> {
    let Type::ImplTrait(it) = ty else {
        return None;
    };
    Some(&it.bounds)
}

/// `Vec<T>` 检测（不含 `children` 槽，children 单独处理）。
fn is_vec_type(ty: &Type) -> bool {
    vec_inner(ty).is_some()
}

/// `Vec<T>` 的元素类型 `T`。
fn vec_inner(ty: &Type) -> Option<Type> {
    let Type::Path(tp) = ty else {
        return None;
    };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Vec" {
        return None;
    }
    let PathArguments::AngleBracketed(a) = &seg.arguments else {
        return None;
    };
    match a.args.first()? {
        GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    }
}

/// 是否文本类型（`String` / `std::string::String`）。
pub(super) fn is_string_type(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().is_some_and(|s| s.ident == "String")
    } else {
        false
    }
}
