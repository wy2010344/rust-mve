//! 文本排版：基于 Parley 的富文本 shaping + 布局 + 缓存。

mod text_layout;
mod font_cache;

pub use text_layout::TextLayout;
pub use font_cache::FontCache;
