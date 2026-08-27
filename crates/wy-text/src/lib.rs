//! 文本排版：基于 Parley 的富文本 shaping + 布局 + 缓存。

mod font_cache;
mod text_layout;

pub use font_cache::FontCache;
pub use text_layout::TextLayout;
