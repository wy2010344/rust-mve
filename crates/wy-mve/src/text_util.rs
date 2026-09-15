//! 文本编辑的 Unicode 边界工具（复刻 Kotlin `Graphemes` / `Words`）。
//!
//! 光标移动、退格、删除、词导航按字素簇/词边界操作，
//! 保证 emoji / 组合字符不拆半。

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// 前一个字素簇边界（索引在字素簇边界仍在 ASCII 文本中为 `i-1`）。
pub fn prev_boundary(text: &str, i: usize) -> usize {
    let i = i.min(text.len());
    let mut cursor = GraphemeCursor::new(i, text.len(), true);
    match cursor.prev_boundary(text, 0) {
        Ok(Some(b)) => b,
        _ => i,
    }
}

/// 后一个字素簇边界。
pub fn next_boundary(text: &str, i: usize) -> usize {
    let i = i.min(text.len());
    let mut cursor = GraphemeCursor::new(i, text.len(), true);
    match cursor.next_boundary(text, 0) {
        Ok(Some(b)) => b,
        _ => i,
    }
}

/// 对索引范围内的文本拆分字素簇（Vec 的起终点）。
pub fn graphemes_bounds(text: &str) -> Vec<(usize, usize)> {
    text.grapheme_indices(true).map(|(a, s)| (a, a + s.len())).collect()
}

/// 前一个词边界。
pub fn prev_word_boundary(text: &str, i: usize) -> usize {
    let i = i.min(text.len());
    if i == 0 {
        return 0;
    }
    // 收集 [0, i) 的逐个词边界
    let mut bounds: Vec<usize> = vec![0];
    let mut last_end = 0;
    for (start, word) in text.split_word_bound_indices() {
        let end = start + word.len();
        if end >= i {
            break;
        }
        last_end = end;
        let _ = bounds;
    }
    last_end
}

/// 后一个词边界。
pub fn next_word_boundary(text: &str, i: usize) -> usize {
    let i = i.min(text.len());
    if i == text.len() {
        return text.len();
    }
    let mut found = text.len();
    for (start, _) in text.split_word_bound_indices() {
        if start >= i {
            found = start;
            break;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prev_boundary_ascii() {
        assert_eq!(prev_boundary("abc", 2), 1);
        assert_eq!(prev_boundary("abc", 0), 0);
    }

    #[test]
    fn next_boundary_ascii() {
        assert_eq!(next_boundary("abc", 0), 1);
        assert_eq!(next_boundary("abc", 3), 3);
    }

    #[test]
    fn emoji_not_split() {
        let s = "a😀b";
        assert_eq!(next_boundary(s, 1), 4); // a → 整个 emoji
        assert_eq!(prev_boundary(s, 4), 1);
    }

    #[test]
    fn combining_char_not_split() {
        let s = "e\u{301}x"; // e + combining acute
        let b = next_boundary(s, 0);
        assert_eq!(&s[..b], "e\u{301}");
    }

    #[test]
    fn word_boundaries_basic() {
        let s = "hello world foo";
        assert_eq!(prev_word_boundary(s, 6), 5); // 在 world 内
        assert_eq!(next_word_boundary(s, 0), 5); // hello 尾
        assert_eq!(prev_word_boundary(s, 0), 0);
        assert_eq!(next_word_boundary(s, s.len()), s.len());
    }

    #[test]
    fn word_boundary_skips_spaces() {
        let s = "a  b";
        assert_eq!(next_word_boundary(s, 0), 1);
        assert_eq!(next_word_boundary(s, 1), 3); // 跳过空格到 b
    }
}