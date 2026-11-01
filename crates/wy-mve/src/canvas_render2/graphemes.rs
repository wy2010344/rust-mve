//! 词边界（对标 TS `canvasRender2/Graphemes.ts` 的 `Words`，实现抽自
//! `canvasCore/Graphemes`）。
//!
//! 字素簇边界（原 `Graphemes`）已随编辑态一起收进 `wy_text::EditCore`，组件层
//! 不再保留第二份——两套边界算法在同一个编辑器里只会互相打架。
//!
//! 索引空间：本文件所有 `i` 与返回偏移均为**字符（Unicode 标量）索引**，与
//! Rust `String` 的 `chars().count()` 一致。TS 原版用 UTF-16 索引；对 BMP 文本
//! （含中文）两者数值相同，仅 astral 字符（部分 emoji）上有差异——属平台差异，非取巧。

use unicode_segmentation::UnicodeSegmentation;

/// 字节偏移 → 字符索引。
fn byte_to_char_idx(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// 词边界工具（对标 TS `Words`）。
pub struct Words;

impl Words {
    /// 词字符区间 `[start, end)` 列表（按文档序）。
    fn ranges(text: &str) -> Vec<(usize, usize)> {
        UnicodeSegmentation::unicode_word_indices(text)
            .map(|(byte, word)| {
                let start = byte_to_char_idx(text, byte);
                let end = start + word.chars().count();
                (start, end)
            })
            .collect()
    }

    /// 包含 `offset` 的词区间 `[start, end)`；无词返回 `None`。
    ///
    /// 对标 TS `Words.wordRangeAt(text, offset)`（双击选词）。
    #[must_use]
    pub fn word_range_at(text: &str, offset: usize) -> Option<(usize, usize)> {
        Words::ranges(text)
            .into_iter()
            .find(|(s, e)| *s <= offset && offset < *e)
    }

    /// 上一词起点：最大 `start < offset`；无则 0（Ctrl+Left / 退词）。
    ///
    /// 对标 TS `Words.prevBoundary(text, i)`。
    #[must_use]
    pub fn prev_boundary(text: &str, i: usize) -> usize {
        Words::ranges(text)
            .into_iter()
            .map(|(s, _)| s)
            .filter(|&s| s < i)
            .max()
            .unwrap_or(0)
    }

    /// 下一词终点：最小 `end > offset`；无则文本字符长度（Ctrl+Right / 删词）。
    ///
    /// 对标 TS `Words.nextBoundary(text, i)`。
    #[must_use]
    pub fn next_boundary(text: &str, i: usize) -> usize {
        let total = text.chars().count();
        Words::ranges(text)
            .into_iter()
            .map(|(_, e)| e)
            .filter(|&e| e > i)
            .min()
            .unwrap_or(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundaries_basic() {
        assert_eq!(Words::word_range_at("hello world", 0), Some((0, 5)));
        assert_eq!(Words::word_range_at("hello world", 6), Some((6, 11)));
        assert_eq!(Words::word_range_at("hello world", 100), None);
        assert_eq!(Words::prev_boundary("hello world", 7), 6);
        assert_eq!(Words::next_boundary("hello world", 2), 5);
        assert_eq!(Words::next_boundary("hello", 2), 5);
    }
}
