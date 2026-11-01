//! `#[rec]` 属性宏：命名参数 + 默认值 + 同名 `!` 宏（返回值即函数返回类型）。
//!
//! # 模块划分（按职责，不按行数）
//!
//! | 文件 | 职责 |
//! |---|---|
//! | `attr.rs` | 组件级属性 `arg = ..` / `extends = ..` 的解析 |
//! | `field.rs` | **唯一的语义判定处**：参数类型 → 存储类型 / 可选性 / 缺省值 |
//! | `extends.rs` | 基类字段表（与 `element.rs` 的 `node2` 签名手工同步） |
//! | `codegen.rs` | 产物代码：worker 签名、`XxxArg`、builder、setter、`.build()` |
//! | `rules.rs` | 同名 `!` 宏的 `macro_rules!` 文本 |
//! | `mod.rs` | 编排：解析 → 逐字段判定 → 三块产物拼装 |
//!
//! 除 `field.rs` 外，其余模块**不做类型判断**，只把已定好的字段照原样铺成代码；
//! 要改语义只需改 `field.rs`，要改生成形状只需改 `codegen.rs` / `rules.rs`。
//!
//! # 产物三件套
//!
//! `XxxArg`（存"存储类型"）+ `XxxArgBuilder`（字段 `Option<存储>`，每个字段一个
//! setter）+ 同名 `!` 宏（`node2! { width: 80.0, .. }`）。调用处**不再**出现
//! `XxxProps { .. }.of()`；`!` 宏点完 setter 直接 `.build()` 出函数签名上写的
//! 那个返回类型，宏不做任何隐式注入（ctx / ref / mount 全在明面上）。
//!
//! # 参数分类（按类型驱动，不靠参数上的辅助属性）
//!
//! - `children` → `Vec<Fc>` 子节点槽，默认空；
//! - `#[getter]` → 读取式字段，见下面「getter 三态」；
//! - `impl Fn(..) -> R` → 可空回调：`Option<Rc<dyn Fn(..) -> R>>`，默认 `None`；
//! - `Option<T>` → 可空，默认 `None`，setter 收裸 `T`（自动包 `Some`）；
//! - `Vec<T>` → 可空，默认空 `Vec`；
//! - 其余具体类型 → **必填**；`#[props(default)]` / `#[props(default = expr)]` 可给默认。
//!
//! 上面四条"可空"形态都能用字段级 `#[props(required)]` 拉回必填（见下方注解表）。
//!
//! # getter：**属性即表达式**（懒求值）
//!
//! `#[getter]` 字段在调用处写的是**表达式本身**，不是它此刻的值：
//!
//! ```ignore
//! node2! {
//!     width: 220.0,                 // → .width(move || 220.0)
//!     height: cx.get().1,           // → .height(move || cx.get().1)  每帧重算
//!     hide: flag.get(),             // → .hide(move || flag.get())
//! }
//! ```
//!
//! 宏为每个 getter 字段生成一条**字面量 ident 分支**（`.{字段}(move || $v)`），
//! 排在通用 `$k:ident : $v:expr` 分支之前——于是 getter 字段的右值永不在此刻求值，
//! 到绘制/布局/命中期才调用。这正对应 TS 的 `GetValue<T> = T | (() => T)`：
//! 裸值那半（`T`）由 `move || T` 承担，**不需要**任何 `Always`/`IntoGetter` 包装；
//! setter 直接收 `F: Fn() -> T`。
//!
//! `move` 是必需的：getter 要 `'static`。因为 getter 只用不可变捕获（`Fn`，不是
//! `FnMut`），`move` 只是把句柄（`Signal` / `Rc`）搬进闭包，读语义不变；同一份
//! 数据要在多个 prop 里用，就先 `clone()` 一份（`Rc`）或直接用 `Copy` 的 `Signal`。
//!
//! 要"把已有 getter 往下传"，写 `x: readout()`（属性是表达式，括号即求值），
//! 不要写 `x: readout` ——那会把 `Get<T>` 本身当成 `T`。
//!
//! 简写（`gap,`）只对**非 getter** 字段有效：getter 字段的简写要转写成
//! `.gap(move || gap)`，而宏定义侧生成的 ident 落到调用处会被 hygienic 挡住，
//! 所以 getter 字段一律写全 `gap: gap`。
//!
//! # getter 三态（能用默认值就不用 Option）
//!
//! | 写法 | 存储类型 | 缺省 |
//! |---|---|---|
//! | `#[getter] focus_order: Option<i32>` | `Get<Option<i32>>` | `None`（Kotlin `Int? = null`） |
//! | `#[getter] #[props(default = false)] hide: bool` | `Get<bool>` | `false` |
//! | `#[getter] width: f32`（无默认） | `Option<Get<f32>>` | `None` = 未声明 |
//!
//! 也就是：声明类型带 `Option` 时它就是「值本身可空」（只有一层 Option，语义是
//! null 而不是"没写"）；给了默认值就是 `Get<T>`（worker 直接 `f()`，不必判空）；
//! 两者都没有才落到 `Option<Get<T>>`——留给 `width` / `height` 这种"没写就交给
//! 布局"的字段。
//!
//! # 字段注解（全部写在字段上，组件级不维护任何清单）
//!
//! | 注解 | 管什么 |
//! |---|---|
//! | `#[getter]` | 怎么读：每次求值重算（`Get<T>`），属性当表达式写 |
//! | `#[props(default = ..)]` | 缺省值 |
//! | `#[props(required)]` | 强制必填（与 `default` 互斥，同时写直接报错） |
//!
//! `required` 只用于把**按类型本可省略**的字段（`impl Fn` / `Option<T>` / `Vec<T>` /
//! 无默认的 getter）拉回必填；普通无默认字段本来就是必填，不必再写。

mod attr;
mod codegen;
mod extends;
mod field;
mod rules;

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;

use crate::util::{mve, to_pascal_case};

use attr::V2Attr;
use field::V2Field;

pub(crate) fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let m = mve();
    let func: syn::ItemFn = syn::parse2(item)?;
    let sig = &func.sig;
    let name = sig.ident.clone();
    let vis = func.vis.clone();

    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &sig.generics,
            "tfc/fc/rec 不支持泛型/生命周期参数",
        ));
    }
    if sig.asyncness.is_some() || sig.constness.is_some() || sig.unsafety.is_some() {
        return Err(syn::Error::new_spanned(
            sig,
            "tfc/fc/rec 不支持 async / const / unsafe",
        ));
    }

    let v2attr: V2Attr = if attr.is_empty() {
        V2Attr::default()
    } else {
        syn::parse2(attr)?
    };

    let props_pascal = to_pascal_case(&name.to_string());
    let props_name = v2attr
        .arg_name
        .clone()
        .unwrap_or_else(|| format_ident!("{}Arg", props_pascal));

    // 继承：基类约定暴露一份「props」类型作为 `rest` 字段的类型（`extends = node2`
    // 时即 `Node2Props`，`extends = rich_text_node` 时是 `RichTextNodeArg`——名字由
    // 基类的 `#[rec(arg = ..)]` 决定，查表见 `extends.rs`）。宏把未知 key 内联转发
    // 进基类 props 的 builder，`rest` 本身就是一份完整的基类 props。组件内部因此能写
    // `node2! { ..rest, focusable: true }`——TS `{ ...rest, focusable: true }` 的
    // 同一件事，不必知道任何内部 IR。
    let extends = v2attr.extends.clone();
    let base_props_ty: Ident = match &extends {
        Some(b) => extends::props_type(b),
        None => format_ident!("__NoExtends"),
    };

    // `omit(..)`：TS `extends Omit<Base, 'k'>` 的落点。只有继承了才有东西可省；
    // 点名的字段必须是基类真有的（否则调用处会莫名其妙地「宏不认这个 key」）。
    if let Some(b) = &extends {
        extends::validate_omit(b, &v2attr.omit)?;
    } else if let Some(first) = v2attr.omit.first() {
        return Err(syn::Error::new_spanned(
            first,
            "`omit(..)` 只能和 `extends = ..` 一起用：没有基类，就没有可省的字段",
        ));
    }

    let mut fields: Vec<V2Field> = Vec::new();
    for arg in &sig.inputs {
        let syn::FnArg::Typed(pt) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "tfc/fc/rec 参数须是简单 `name: Type`（不支持 self）",
            ));
        };
        fields.push(V2Field::classify(pt, &m)?);
    }
    if extends.is_some() {
        fields.push(V2Field::rest(&base_props_ty));
    }

    let ret = codegen::ret_ty(sig);
    let worker = codegen::rewrite_worker(func, &fields);
    let props = codegen::gen_props(&name, &props_name, &vis, &props_pascal, &ret, &fields);
    let decl_macro = rules::gen(&name, &props_name, &fields, extends.as_ref(), &v2attr.omit)?;

    Ok(quote! {
        #worker
        #props
        #decl_macro
    })
}
