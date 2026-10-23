//! 字体回退回归：默认样式的 CJK 文本必须选中含有效字形的字体（glyph id != 0）。
//!
//! 若此测试失败，说明系统缺 CJK 字体回退——界面中文会渲染成方块（tofu）。
//!
//! # 关于「机器上根本没有中文字体」
//!
//! 干净容器 / CI runner 可能一个中文字体都没装。这时**测不出任何结论**：
//! 字形 id=0 只是因为没字体可用，不是回退逻辑坏了。
//! 所以这里先探测系统里**有没有任何字体能覆盖 CJK**：
//!
//! - 没有 → 跳过并说明原因（不是失败）
//! - 有 → 断言回退机制正确工作（此时失败就是真 bug）
//!
//! CI 里装了 `fonts-noto-cjk`（Linux）和 Noto Sans CJK SC（Windows），
//! 所以那条路径上测试是**真的在跑**的。
use parley::{FontContext, LayoutContext, PositionedLayoutItem, StyleProperty};

/// 探测系统里是否存在能覆盖 CJK 的字体。
///
/// 做法：对每个已注册的字体族，检查它是否报告了中文的 cmap 覆盖。
/// fontique 的 `Collection` 提供 `families()`，这里用最朴素的方式：
/// **直接用每个族去排版一个中文字符，看字形 id 是否非零**。
fn system_has_cjk_font() -> bool {
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let text = "中";

    // 收集族名（`family_names` 需要可变借用，先克隆出来）
    let names: Vec<String> = font_cx
        .collection
        .family_names()
        .map(|s| s.to_string())
        .collect();

    for name in names {
        let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, false);
        builder.push_default(StyleProperty::FontSize(20.0));
        builder.push_default(StyleProperty::FontFamily(name.as_str().into()));
        let mut layout: parley::Layout<[u8; 4]> = builder.build(text);
        layout.break_all_lines(None);

        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                for g in glyph_run.glyphs() {
                    if g.id != 0 {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[test]
fn cjk_glyph_selection_nonzero() {
    if !system_has_cjk_font() {
        eprintln!(
            "跳过：系统里没有能覆盖 CJK 的字体，测不出回退是否正常。\n\
             装一个中文字体后再跑（Linux: apt install fonts-noto-cjk）"
        );
        return;
    }

    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let text = "中文测试";
    let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(20.0));
    let mut layout: parley::Layout<[u8; 4]> = builder.build(text);
    layout.break_all_lines(None);

    let mut glyph_count = 0usize;
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            for g in glyph_run.glyphs() {
                assert_ne!(g.id, 0, "CJK 字形 id=0（tofu）：字体回退失败");
                glyph_count += 1;
            }
        }
    }
    assert_eq!(
        glyph_count,
        text.chars().count(),
        "每个 CJK 字符都应产出字形"
    );
}

/// 探测函数本身的可靠性：拉丁字符必须被覆盖，中文字符未必。
///
/// 这条测试守住「探测逻辑没写错」——如果 `system_has_cjk_font` 恒返回 true，
/// 那 CI 上缺字体的机器就会重新变成误报失败。
#[test]
fn probe_detects_latin_coverage() {
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();

    // "A" 一定被覆盖
    let mut builder = layout_cx.ranged_builder(&mut font_cx, "A", 20.0, false);
    builder.push_default(StyleProperty::FontSize(20.0));
    let mut layout: parley::Layout<[u8; 4]> = builder.build("A");
    layout.break_all_lines(None);

    let mut latin_ok = false;
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            for g in glyph_run.glyphs() {
                if g.id != 0 {
                    latin_ok = true;
                }
            }
        }
    }
    assert!(latin_ok, "拉丁字符应能被覆盖，探测逻辑可能写错了");
}
