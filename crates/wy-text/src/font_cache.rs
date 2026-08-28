//! 字体上下文：封装 Parley 的 `FontContext` + `LayoutContext`。
//!
//! `FontContext` 管理系统字体数据库（加载代价高），`LayoutContext` 是排版计算的
//! scratch space（可复用分配）。两者都应全局共享（至少在粗粒度边界共享）。

use parley::{FontContext as ParleyFontContext, LayoutContext};

/// 全局字体上下文：包含字体数据库 + 排版 scratch space。
///
/// 创建代价高（需要枚举系统字体），应在应用生命周期中共享复用。
/// 两个字段公开以便 `build_paragraph` 分别借用。
pub struct FontContext {
    /// Parley 字体数据库。
    pub font_cx: ParleyFontContext,
    /// Parley 排版 scratch space。
    pub layout_cx: LayoutContext,
}

impl FontContext {
    /// 创建新的字体上下文（枚举系统字体，代价较高）。
    pub fn new() -> Self {
        Self {
            font_cx: ParleyFontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }
}

impl Default for FontContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_context_creates_successfully() {
        let fc = FontContext::new();
        let _ = fc;
    }

    #[test]
    fn font_context_default_is_same_as_new() {
        let a = FontContext::new();
        let b = FontContext::default();
        let _ = a;
        let _ = b;
    }
}
