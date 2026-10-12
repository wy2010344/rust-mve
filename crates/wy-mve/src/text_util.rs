//! 文本编辑的 Unicode 边界工具（复刻 Kotlin `Graphemes` / `Words`）。
//!
//! 光标移动、退格、删除、词导航按字素簇/词边界操作，
//! 保证 emoji / 组合字符不拆半。

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// display char 索引 → 字节索引（越过 `char_idx` 个字符的起点；越界返回 `text.len()`）。
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// 字节索引 → char 索引（非 char 边界向下钳制）。
pub fn byte_to_char(text: &str, byte_idx: usize) -> usize {
    let mut b = byte_idx.min(text.len());
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    text[..b].chars().count()
}

/// 前一个字素簇边界（索引在字素簇边界仍在 ASCII 文本中为 `i-1`）。
///
/// 非字符边界输入向下钳制（`GraphemeCursor` 要求 char boundary）。
pub fn prev_boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    let mut cursor = GraphemeCursor::new(i, text.len(), true);
    match cursor.prev_boundary(text, 0) {
        Ok(Some(b)) => b,
        _ => i,
    }
}

/// 后一个字素簇边界。非字符边界输入向上钳制。
pub fn next_boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    let mut cursor = GraphemeCursor::new(i, text.len(), true);
    match cursor.next_boundary(text, 0) {
        Ok(Some(b)) => b,
        _ => i,
    }
}

/// 对索引范围内的文本拆分字素簇（Vec 的起终点）。
pub fn graphemes_bounds(text: &str) -> Vec<(usize, usize)> {
    text.grapheme_indices(true)
        .map(|(a, s)| (a, a + s.len()))
        .collect()
}

/// 前一个词边界（当前词或空白段的起点；`i == 0` 返回 0）。
pub fn prev_word_boundary(text: &str, i: usize) -> usize {
    let i = i.min(text.len());
    if i == 0 {
        return 0;
    }
    // 最后一个终点 ≤ i 的词的起点
    let mut found = 0;
    for (start, word) in text.split_word_bound_indices() {
        let end = start + word.len();
        if end <= i {
            found = start;
        } else {
            break;
        }
    }
    found
}

/// 后一个词边界（第一个终点 > i 的词的终点；到文末返回 `text.len()`）。
pub fn next_word_boundary(text: &str, i: usize) -> usize {
    let i = i.min(text.len());
    let mut found = text.len();
    for (start, word) in text.split_word_bound_indices() {
        let end = start + word.len();
        if end > i {
            found = end;
            break;
        }
    }
    found
}

/// 词字符判定：Kotlin `Words.isWordChar` = `isLetterOrDigit`（含汉字/数字）。
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// 包含 [offset] 的词区间（半开，字节）——抄 Kotlin `Words.wordRangeAt`（双击选词）。
///
/// - 空文本返回 `None`；`offset` 越界钳制到 `n-1`；
/// - offset 落在字素簇内部时归入该簇（`prevBoundary(pos+1)`）；
/// - 非词字符（标点 / 空白 / emoji）返回其所在字素簇。
///
/// 与 `prev_word_boundary`/`next_word_boundary`（UAX#29 分词）**不同源**：
/// 双击选词与 Ctrl+←/→ 词跳在 Kotlin 侧同走 `Words`，此处保持同构。
pub fn word_range_at(text: &str, offset: usize) -> Option<(usize, usize)> {
    let n = text.len();
    if n == 0 {
        return None;
    }
    let pos = offset.min(n - 1);
    // 含 offset 的字素簇起点（offset 在簇内部时归该簇，Kotlin prevBoundary(pos+1) 同义）。
    let anchor = text
        .grapheme_indices(true)
        .map(|(s, _)| s)
        .take_while(|&s| s <= pos)
        .last()
        .unwrap_or(0);
    let anchor_char = text[anchor..].chars().next()?;
    if !is_word_char(anchor_char) {
        // 标点 / 空白 / emoji：返回其所在字素簇
        return Some((anchor, next_boundary(text, anchor)));
    }
    // 向左扩展到词首
    let mut start = anchor;
    while start > 0 {
        let p = prev_boundary(text, start);
        if p == start {
            break;
        }
        let c = match text[p..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if !is_word_char(c) {
            break;
        }
        start = p;
    }
    // 向右扩展到词尾
    let mut end = next_boundary(text, anchor);
    while end < n {
        let q = next_boundary(text, end);
        if q == end {
            break;
        }
        let c = match text[end..].chars().next() {
            Some(c) => c,
            None => break,
        };
        if !is_word_char(c) {
            break;
        }
        end = q;
    }
    Some((start, end))
}

/// 双击后按住拖拽的词级扩展（抄 Kotlin `expandWordSelection` 纯函数）：
/// 锚词 `[anchor_start, anchor_end)` 固定，目标偏移在锚词之外时对应端
/// 吸附到目标处整词边界；无分词能力（`word_range_at` 返回 `None`）时退化为字符粒度。
pub fn expand_word_selection(
    anchor_start: usize,
    anchor_end: usize,
    target: usize,
    word_range_at: impl Fn(usize) -> Option<(usize, usize)>,
) -> (usize, usize) {
    if target < anchor_start {
        (
            word_range_at(target).map(|r| r.0).unwrap_or(target),
            anchor_end,
        )
    } else if target > anchor_end {
        (
            anchor_start,
            word_range_at(target).map(|r| r.1).unwrap_or(target),
        )
    } else {
        (anchor_start, anchor_end)
    }
}

/// 多击追踪：同位置连续点击计数（抄 Kotlin `ClickTracker.recordDown`，
/// 400ms 内、位移 < 5px 视为同一击）。触发新序列时计数归 1。
#[derive(Clone, Copy)]
pub struct ClickTracker {
    has_prev: bool,
    x: f32,
    y: f32,
    time: std::time::Instant,
    count: u32,
}

impl ClickTracker {
    pub fn new() -> Self {
        Self {
            has_prev: false,
            x: 0.0,
            y: 0.0,
            time: std::time::Instant::now(),
            count: 0,
        }
    }

    /// 记录一次按下，返回当前连击计数（1 = 单击，2 = 双击，>= 3 = 三击及以后）。
    pub fn record(&mut self, x: f32, y: f32) -> u32 {
        let now = std::time::Instant::now();
        let repeated = self.has_prev
            && now.duration_since(self.time).as_millis() < 400
            && (x - self.x).abs() < 5.0
            && (y - self.y).abs() < 5.0;
        self.count = if repeated { self.count + 1 } else { 1 };
        self.has_prev = true;
        self.x = x;
        self.y = y;
        self.time = now;
        self.count
    }
}

impl Default for ClickTracker {
    fn default() -> Self {
        Self::new()
    }
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
        let s = "a😀b"; // a[0,1) 😀[1,5) b[5,6)
        assert_eq!(next_boundary(s, 1), 5); // a → 整个 emoji
        assert_eq!(prev_boundary(s, 5), 1);
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

    // --- word_range_at（对齐 Kotlin DoubleClickWordSelectionTest） ---

    #[test]
    fn word_range_at_picks_word_around_offset() {
        let s = "Hello world";
        assert_eq!(word_range_at(s, 8), Some((6, 11))); // world 中间
        assert_eq!(word_range_at(s, 2), Some((0, 5))); // Hello 开头
        assert_eq!(word_range_at(s, 6), Some((6, 11))); // world 起点
        assert_eq!(word_range_at(s, 10), Some((6, 11))); // world 尾
        assert_eq!(word_range_at(s, 99), Some((6, 11))); // 越界钳制
        assert_eq!(word_range_at(s, 5), Some((5, 6))); // 空格上只选其字素簇
        assert_eq!(word_range_at("", 0), None); // 空文本
    }

    #[test]
    fn word_range_at_cjk() {
        let s = "中文 测试"; // 字节：中[0,3) 文[3,6) 空格[6,7) 测[7,10) 试[10,12)
                             // 连续汉字按 Kotlin Words 语义连成一个词 run（无 CJK 分词）
        assert_eq!(word_range_at(s, 3), Some((0, 6))); // "文" 起点 → "中文"
        assert_eq!(word_range_at(s, 4), Some((0, 6))); // 字节内部钳到字素
        assert_eq!(word_range_at(s, 6), Some((6, 7))); // 空格只选其字素簇
        assert_eq!(word_range_at(s, 7), Some((7, 13))); // "测试" 整词（字节 7..13）
    }

    #[test]
    fn expand_word_selection_snaps_to_word() {
        let w = |t: &'static str| move |o: usize| word_range_at(t, o);
        let s = "Hello world foo";
        let g = w(s);
        // 锚词 world[6,11)，向右拖到 foo 中间 → 吸附到词尾
        assert_eq!(expand_word_selection(6, 11, 13, g), (6, 15));
        // 向左拖到 Hello 中间 → 吸附到词首
        assert_eq!(expand_word_selection(6, 11, 2, g), (0, 11));
        // 锚词内部 → 不变
        assert_eq!(expand_word_selection(6, 11, 8, g), (6, 11));
        // 无分词能力 → 退化为字符粒度（抄 Kotlin 测试）
        let none = |_: usize| None;
        assert_eq!(expand_word_selection(6, 11, 14, none), (6, 14));
        assert_eq!(expand_word_selection(6, 11, 3, none), (3, 11));
    }

    #[test]
    fn click_tracker_counts_within_window() {
        let mut t = ClickTracker::new();
        assert_eq!(t.record(10.0, 10.0), 1);
        assert_eq!(t.record(11.0, 10.5), 2);
        assert_eq!(t.record(10.5, 10.0), 3);
        assert_eq!(t.record(10.0, 10.0), 4); // >=3 仍累加（三击分支按 >=3 处理）
                                             // 位移超阈值 → 重置
        assert_eq!(t.record(100.0, 10.0), 1);
        assert_eq!(t.record(101.0, 10.0), 2);
    }
}
