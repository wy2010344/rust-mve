//! 富文本编辑数据：样式段边界表 + 编辑差异对齐（复刻 Kotlin `RichEditableTextNode`）。
//!
//! `TextBuffer` 是纯数据内核，不依赖信号/MVE/渲染：
//! - 逻辑文本是唯一真相源，样式段只做附属表示；
//! - 样式段采用**边界表**：每段记录自己的结束偏移与样式，
//!   第 `i` 个字符的样式 = 第一个 `end > i` 的段的样式（无则 `None` = 基础样式）；
//! - 所有文本写入经 `write_text`：按公共前缀/后缀对齐新旧文本，
//!   被替换区间的段裁剪平移，新插入区间继承插入点左侧字符的样式——
//!   因此打字、退格、选区替换、撤销重做、IME 提交都自动维持样式一致性。
//!
//! 索引单位：**字符索引**（Unicode 标量序号，等价 Kotlin 的字符级操作；
//! Kotlin 内部用 UTF-16 code unit，对 BMP 字符二者数值一致）。
//!
//! 与 Kotlin 对照：
//! - [`TextSegment`] = `Segment`；[`TextBuffer`] = `RichEditableTextNode` 的
//!   segmentList + writeText + styleAt + styleRange + contentSpans。

use crate::{TextSpan, TextStyle};

/// 样式段：覆盖 `(prev_end, end]` 区间；`style` 为 `None` 表示使用基础样式。
#[derive(Clone, Debug, PartialEq)]
pub struct TextSegment {
    /// 段的结束偏移（**字符索引**，不含）。
    pub end: usize,
    /// 显式样式；`None` = 继承基础样式。
    pub style: Option<TextStyle>,
}

/// 富文本缓冲：逻辑文本 + 样式段边界表。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBuffer {
    /// 逻辑文本（唯一真相源）。
    text: String,
    /// 样式段边界表（按 end 升序，铺满 `[0, char_count]`；空文档为空）。
    segments: Vec<TextSegment>,
}

impl TextBuffer {
    /// 构造空缓冲。
    pub fn new() -> Self {
        Self::default()
    }

    /// 从纯文本构造（全部使用基础样式）。
    pub fn from_plain(text: impl Into<String>) -> Self {
        let text = text.into();
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![TextSegment {
                end: text.chars().count(),
                style: None,
            }]
        };
        Self { text, segments }
    }

    /// 当前逻辑文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 当前样式段列表（引用）。
    pub fn segments(&self) -> &[TextSegment] {
        &self.segments
    }

    /// 覆盖第 `idx` 个字符的显式样式；无显式段时返回 `None`（基础样式）。
    pub fn style_at(&self, idx: usize) -> Option<&TextStyle> {
        let n = self.text.chars().count();
        if n == 0 {
            return None;
        }
        let i = idx.min(n - 1);
        self.segments
            .iter()
            .find(|seg| i < seg.end)
            .and_then(|seg| seg.style.as_ref())
    }

    /// 将区间 `[start, end)`（**字符索引**）设为给定样式；
    /// `style` 为 `None` 表示回退到基础样式。
    ///
    /// 与 Kotlin `styleRange` 一致：只改样式段，不动文本、不进撤销栈。
    pub fn style_range(&mut self, start: usize, end: usize, style: Option<TextStyle>) {
        let n = self.text.chars().count();
        let s = start.min(n);
        let e = end.min(n).max(s);
        if s >= e {
            return;
        }
        let mut out: Vec<TextSegment> = Vec::new();
        let mut prev = 0;
        for seg in &self.segments {
            let seg_start = prev;
            prev = seg.end;
            if seg.end <= s || seg_start >= e {
                // 完全在区间外
                out.push(seg.clone());
            } else {
                // 与区间相交：裁剪两侧保留
                if seg_start < s {
                    out.push(TextSegment {
                        end: s,
                        style: seg.style.clone(),
                    });
                }
                if seg.end > e {
                    out.push(TextSegment {
                        end: seg.end,
                        style: seg.style.clone(),
                    });
                }
            }
        }
        out.push(TextSegment {
            end: e,
            style: style.clone(),
        });
        self.segments = normalize(out, n);
    }

    /// 写入新文本：差异对齐样式段（复刻 Kotlin `writeText` 的 diff 逻辑）。
    pub fn write_text(&mut self, new_value: String) {
        if new_value == self.text {
            return;
        }
        let old: String = self.text.clone();
        let (p, ins_end) = diff_bounds(&old, &new_value);
        let old_n = old.chars().count();
        let new_n = new_value.chars().count();
        let aligned = align_segments(
            &self.segments,
            old_n,
            new_n,
            p,
            ins_end,
            self,
        );
        self.segments = aligned;
        self.text = new_value;
    }

    /// 当前文本还原为样式片段列表（对应 Kotlin `contentSpans`）。
    pub fn spans(&self) -> Vec<TextSpan> {
        let n = self.text.chars().count();
        if n == 0 {
            return Vec::new();
        }
        let mut out: Vec<TextSpan> = Vec::new();
        let mut prev = 0;
        for seg in &self.segments {
            let e = seg.end.min(n);
            if e > prev {
                let chunk = slice_chars(&self.text, prev, e).replace('\t', "    ");
                out.push(TextSpan::styled(
                    chunk,
                    seg.style.clone().unwrap_or_default(),
                ));
                prev = e;
                if prev >= n {
                    break;
                }
            }
        }
        if prev < n {
            out.push(TextSpan::text(slice_chars(&self.text, prev, n)));
        }
        out
    }
}

/// 公共前缀长度 `p` 与新串插入终点 `ins_end`（旧串删除终点 = `ins_end - delta`）。
///
/// 均为**字符索引**；逐字符比较天然对齐字符边界，不会把索引落在字符中间。
fn diff_bounds(old: &str, new: &str) -> (usize, usize) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let m = old_chars.len().min(new_chars.len());
    let mut p = 0;
    while p < m && old_chars[p] == new_chars[p] {
        p += 1;
    }
    let mut q = 0;
    while q < m - p && old_chars[old_chars.len() - 1 - q] == new_chars[new_chars.len() - 1 - q] {
        q += 1;
    }
    (p, new_chars.len() - q)
}

/// 差异对齐样式段（复刻 Kotlin `alignSegments`）；索引单位均为字符。
fn align_segments(
    segments: &[TextSegment],
    old_n: usize,
    new_n: usize,
    p: usize,
    ins_end: usize,
    buffer: &TextBuffer,
) -> Vec<TextSegment> {
    let delta = new_n as isize - old_n as isize;
    let old_remove_end = (ins_end as isize - delta) as usize;
    let mut out: Vec<TextSegment> = Vec::new();
    let mut prev = 0usize;
    for seg in segments {
        let start = prev;
        prev = seg.end;
        if seg.end <= p {
            // 整段在被删区之前
            out.push(seg.clone());
        } else if start >= old_remove_end {
            // 整段在后：整体平移
            out.push(TextSegment {
                end: (seg.end as isize + delta) as usize,
                style: seg.style.clone(),
            });
        } else {
            // 相交：裁剪拼接
            if start < p {
                out.push(TextSegment {
                    end: p,
                    style: seg.style.clone(),
                });
            }
            if seg.end > old_remove_end {
                out.push(TextSegment {
                    end: ins_end + (seg.end - old_remove_end),
                    style: seg.style.clone(),
                });
            }
        }
    }
    if ins_end > p {
        // 新插入区间继承插入点左侧字符的样式（文档首无左邻时取 None=基础样式）
        let inherit = if old_n == 0 {
            None
        } else {
            buffer.style_at(p.saturating_sub(1)).cloned()
        };
        out.push(TextSegment {
            end: ins_end,
            style: inherit,
        });
    }
    normalize(out, new_n)
}

/// 排序、钳制边界、合并相邻同款，保证边界表铺满 `[0, len]`（复刻 Kotlin `normalize`）。
fn normalize(segments: Vec<TextSegment>, len: usize) -> Vec<TextSegment> {
    if len == 0 {
        return Vec::new();
    }
    let mut out: Vec<TextSegment> = Vec::new();
    for s in sorted_by_end(segments) {
        let e = s.end.max(1).min(len);
        if out.is_empty() {
            out.push(TextSegment {
                end: e,
                style: s.style,
            });
            continue;
        }
        let last_idx = out.len() - 1;
        let last_end = out[last_idx].end;
        if e <= last_end {
            // 被覆盖
        } else if s.style == out[last_idx].style {
            // 合并相邻同款
            out[last_idx] = TextSegment { end: e, style: s.style };
        } else {
            out.push(TextSegment {
                end: e,
                style: s.style,
            });
        }
    }
    if out.last().map_or(0, |s| s.end) < len {
        out.push(TextSegment {
            end: len,
            style: None,
        });
    }
    out
}

fn sorted_by_end(mut segments: Vec<TextSegment>) -> Vec<TextSegment> {
    segments.sort_by_key(|s| s.end);
    segments
}

/// 按字符区间 `[start, end)` 切出字符串（index 均为字符索引）。
fn slice_chars(s: &str, start: usize, end: usize) -> String {
    s.chars().skip(start).take(end - start).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style_marker(n: u32) -> TextStyle {
        TextStyle::normal().with_color(n)
    }

    #[test]
    fn from_plain_uses_base_style() {
        let buf = TextBuffer::from_plain("hello");
        assert_eq!(buf.text(), "hello");
        assert_eq!(buf.segments().len(), 1);
        assert_eq!(buf.segments()[0].end, 5);
        assert_eq!(buf.segments()[0].style, None);
    }

    #[test]
    fn from_plain_char_count_for_multibyte() {
        let buf = TextBuffer::from_plain("你好");
        assert_eq!(buf.segments()[0].end, 2, "字符索引而非字节");
    }

    #[test]
    fn style_at_returns_explicit_style() {
        let mut buf = TextBuffer::from_plain("hello world");
        buf.style_range(0, 5, Some(style_marker(0xFF0000FF)));
        assert_eq!(buf.style_at(0), Some(&style_marker(0xFF0000FF)));
        assert_eq!(buf.style_at(4), Some(&style_marker(0xFF0000FF)));
        assert_eq!(buf.style_at(6), None); // 之后为 base
    }

    #[test]
    fn style_range_empty_is_noop() {
        let mut buf = TextBuffer::from_plain("abc");
        let before = buf.clone();
        buf.style_range(2, 2, Some(style_marker(1)));
        assert_eq!(buf, before);
    }

    #[test]
    fn style_range_splits_overlapping_segments() {
        let mut buf = TextBuffer::from_plain("abcdef");
        buf.style_range(1, 5, Some(style_marker(1)));
        assert_eq!(buf.style_at(0), None);
        assert_eq!(buf.style_at(1), Some(&style_marker(1)));
        assert_eq!(buf.style_at(4), Some(&style_marker(1)));
        assert_eq!(buf.style_at(5), None);
    }

    #[test]
    fn write_text_insert_inherits_left_style() {
        let mut buf = TextBuffer::from_plain("hello");
        buf.style_range(0, 5, Some(style_marker(1)));
        buf.write_text("hellox".into());
        assert_eq!(buf.style_at(5), Some(&style_marker(1)));
        assert_eq!(buf.text(), "hellox");
    }

    #[test]
    fn write_text_insert_splits_after_boundary() {
        let mut buf = TextBuffer::from_plain("ab");
        buf.style_range(0, 1, Some(style_marker(1))); // a 有样式
        buf.write_text("aXb".into());
        assert_eq!(buf.style_at(0), Some(&style_marker(1)));
        assert_eq!(buf.style_at(1), Some(&style_marker(1))); // X 继承左邻 a
        assert_eq!(buf.text(), "aXb");
    }

    #[test]
    fn write_text_delete_removes_affected_style() {
        let mut buf = TextBuffer::from_plain("abc");
        buf.style_range(1, 2, Some(style_marker(1))); // b 有样式
        buf.write_text("ac".into()); // 删 b
        assert_eq!(buf.text(), "ac");
        assert_eq!(buf.style_at(0), None); // a
        assert_eq!(buf.style_at(1), None); // c
    }

    #[test]
    fn write_text_append_keeps_style() {
        let mut buf = TextBuffer::from_plain("hi");
        buf.style_range(0, 2, Some(style_marker(1)));
        buf.write_text("hi!".into());
        assert_eq!(buf.text(), "hi!");
        assert_eq!(buf.style_at(0), Some(&style_marker(1)));
        assert_eq!(buf.style_at(2), Some(&style_marker(1)), "插入继承左邻");
    }

    #[test]
    fn write_text_replace_uses_suffix_alignment() {
        let mut buf = TextBuffer::from_plain("你好world");
        buf.style_range(0, 2, Some(style_marker(1))); // 你好 有样式
        buf.write_text("你world".into()); // 删 好
        assert_eq!(buf.text(), "你world");
        assert_eq!(buf.style_at(0), Some(&style_marker(1)));
        assert_eq!(buf.style_at(1), None); // 好 删除后，world 为 base
    }

    #[test]
    fn write_text_multibyte_insert_keeps_style() {
        let mut buf = TextBuffer::from_plain("你好");
        buf.style_range(0, 2, Some(style_marker(1)));
        buf.write_text("你好小".into()); // 尾部插入 小
        assert_eq!(buf.text(), "你好小");
        assert_eq!(buf.style_at(2), Some(&style_marker(1)), "新字符继承左邻");
        assert_eq!(buf.style_at(1), Some(&style_marker(1)));
    }

    #[test]
    fn spans_reconstruct_fragments() {
        let mut buf = TextBuffer::from_plain("hello world");
        buf.style_range(0, 5, Some(style_marker(1)));
        let spans = buf.spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "hello");
        assert_eq!(spans[0].style, style_marker(1));
        assert_eq!(spans[1].text, " world");
        assert_eq!(spans[1].style, TextStyle::default());
    }

    #[test]
    fn spans_on_empty_is_empty() {
        let buf = TextBuffer::from_plain("");
        assert!(buf.spans().is_empty());
    }

    #[test]
    fn spans_multibyte_preserved() {
        let mut buf = TextBuffer::from_plain("你好世界");
        buf.style_range(0, 2, Some(style_marker(1)));
        let spans = buf.spans();
        assert_eq!(spans[0].text, "你好");
        assert_eq!(spans[1].text, "世界");
    }

    #[test]
    fn normalize_merges_adjacent_same_style() {
        let segs = vec![
            TextSegment {
                end: 3,
                style: Some(style_marker(1)),
            },
            TextSegment {
                end: 6,
                style: Some(style_marker(1)),
            },
        ];
        let out = normalize(segs, 6);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].end, 6);
        assert_eq!(out[0].style, Some(style_marker(1)));
    }

    #[test]
    fn write_text_insert_middle_shifts_tail() {
        let mut buf = TextBuffer::from_plain("abcdefgh");
        buf.style_range(0, 4, Some(style_marker(1)));
        buf.write_text("abXYcdefgh".into()); // c 前插 XY
        assert_eq!(buf.text(), "abXYcdefgh");
        assert_eq!(buf.style_at(0), Some(&style_marker(1)), "头部样式保留");
        assert_eq!(buf.style_at(4), Some(&style_marker(1)), "XY 继承左邻 b");
        assert_eq!(buf.style_at(5), Some(&style_marker(1)), "c.. 仍保持原样式");
    }

    /// 等价 Kotlin 场景：对 "hello" 前 3 字符样式化后追加，再删中段。
    #[test]
    fn kotlin_scenario_type_delete_append() {
        let mut buf = TextBuffer::from_plain("hello");
        buf.style_range(0, 3, Some(style_marker(1))); // hel 有样式
        // 打字：末尾插 x
        buf.write_text("hellox".into());
        assert_eq!(buf.style_at(2), Some(&style_marker(1)));
        assert_eq!(buf.style_at(5), None, "x 继承左邻 l（无样式）");
        // 退格删 x
        buf.write_text("hello".into());
        assert_eq!(buf.style_at(2), Some(&style_marker(1)));
        assert_eq!(buf.style_at(4), None);
        // 中间替换 e→E
        buf.write_text("hEllo".into());
        assert_eq!(buf.style_at(1), Some(&style_marker(1)), "E 继承左邻 h");
        assert_eq!(buf.style_at(2), Some(&style_marker(1)));
        assert_eq!(buf.text(), "hEllo");
    }
}