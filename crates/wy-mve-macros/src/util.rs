//! 宏的公共小工具：crate 路径解析与命名转换。

use proc_macro2::TokenStream as TokenStream2;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::{format_ident, quote};

/// 解析 `wy_mve` 的真实 crate 路径（宏在本 crate 内部使用时解析为 `crate`）。
pub(crate) fn mve() -> TokenStream2 {
    match crate_name("wy-mve") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name);
            quote! { ::#ident }
        }
        Err(_) => quote! { ::wy_mve },
    }
}

/// `snake_case` → `PascalCase`（用于组件名 → Props 结构体名）。
///
/// 例：`stat_card` → `StatCard`，`text_field` → `TextField`。
pub(crate) fn to_pascal_case(s: &str) -> String {
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
