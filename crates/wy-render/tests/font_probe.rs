//! 字体回退回归：默认样式的 CJK 文本必须选中含有效字形的字体（glyph id != 0）。
//!
//! 若此测试失败，说明系统缺 CJK 字体回退——界面中文会渲染成方块（tofu）。
use parley::{FontContext, LayoutContext, PositionedLayoutItem, StyleProperty};

#[test]
fn cjk_glyph_selection_nonzero() {
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
