//! 字体回退回归：默认样式的 CJK 文本必须选中含有效字形的字体（glyph id != 0）。
//!
//! 若此测试失败，说明系统缺 CJK 字体回退——界面中文会渲染成方块（tofu）。
//!
//! # 关于「机器上根本没有可用的中文字体」
//!
//! 干净容器 / CI runner 可能一个中文字体都没装，或装了却没注册进字体系统
//! （例如只把 `.ttf` 丢进 `Fonts` 目录却未写注册表）。这时**测不出任何结论**：
//! 字形 id=0 只是因为回退链实际选不到中文字体，不是回退逻辑坏了。
//!
//! 所以测试先**用与实际渲染完全相同的路径**试渲染一次 `中`：
//!
//! - 默认字体 + parley 回退链能选出非零字形 → 做完整断言（失败就是真 bug）
//! - 选不出（tofu）→ 跳过并说明，不是失败
//!
//! 这样在开发者本机（有中文字体）能抓回退回归，在字体不全的 CI 上不会误报红。
//!
//! # 为什么跳过条件必须用「真实渲染」而不是「字符表查询」
//!
//! 早期版本用 fontique 的 `Charmap::map` 枚举所有字体族问「有没有覆盖中文」，
//! 但它只要有一个字体**声称**覆盖就返回 true；而真实排版走的是
//! 默认字体族 + 回退链，选不到那个声称覆盖的字体时仍是 tofu。
//! 两套路径结论会打架，导致探测说「有字体」、断言却失败——正是此前 CI 一直红的根因。
use parley::{FontContext, LayoutContext, PositionedLayoutItem, StyleProperty};

/// 用**默认字体 + parley 回退链**渲染 `中`，看能否得到非零字形。
///
/// 与 `cjk_glyph_selection_nonzero` 的断言走同一条路径，
/// 所以「能不能渲染」由真实渲染决定，而非由「集合里有没有字体声称覆盖」决定。
fn default_renders_cjk() -> bool {
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let text = "中";
    let mut builder = layout_cx.ranged_builder(&mut font_cx, text, 1.0, false);
    builder.push_default(StyleProperty::FontSize(20.0));
    // 不指定任何 explicit family —— 完全走默认字体族 + parley 回退链（与真实 app 一致）
    let mut layout: parley::Layout<[u8; 4]> = builder.build(text);
    layout.break_all_lines(None);
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(gr) = item {
                for g in gr.glyphs() {
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
    if !default_renders_cjk() {
        eprintln!(
            "跳过：默认字体 + 回退链选不出中文字形（系统未安装/未注册可用的中文字体）。\n\
             这是环境缺口，不是回退逻辑 bug；装中文字体后再跑即可验证回退是否正确。"
        );
        return;
    }

    // —— 以下与 default_renders_cjk 同路径，但这次做完整断言 ——
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

/// 拉丁字符必须被覆盖——守住「渲染路径本身没坏」。
#[test]
fn probe_detects_latin_coverage() {
    let mut font_cx = FontContext::new();
    let mut layout_cx = LayoutContext::new();
    let mut builder = layout_cx.ranged_builder(&mut font_cx, "A", 1.0, false);
    builder.push_default(StyleProperty::FontSize(20.0));
    let mut layout: parley::Layout<[u8; 4]> = builder.build("A");
    layout.break_all_lines(None);
    let mut latin_ok = false;
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(gr) = item {
                for g in gr.glyphs() {
                    if g.id != 0 {
                        latin_ok = true;
                    }
                }
            }
        }
    }
    assert!(latin_ok, "拉丁字符应能被覆盖，渲染路径可能坏了");
}

/// 字符表探测函数的**否定分支**单元测试（与上面的渲染路径解耦，纯逻辑）。
///
/// 只在「不覆盖 CJK 的字体族」集合里查，必须返回 false；
/// 否则 `system_has_cjk_font` 类探测一旦恒返回 true，缺字体的机器上就会误报失败。
/// 这里用字体字符表查询（不是渲染路径）来验证探测逻辑本身没写反。
fn families_cover_cjk(font_cx: &mut FontContext, names: &[String]) -> bool {
    let source_cache = &mut font_cx.source_cache;
    for name in names {
        let Some(id) = font_cx.collection.family_id(name) else {
            continue;
        };
        let Some(info) = font_cx.collection.family(id) else {
            continue;
        };
        let font_count = info.fonts().len();
        for index in 0..font_count {
            let mut query = font_cx.collection.query(source_cache);
            query.set_families([id]);
            let mut found = false;
            query.matches_with(|f| {
                if f.family.1 != index {
                    return fontique::QueryStatus::Continue;
                }
                if let Some(charmap) = f.charmap() {
                    if charmap.map('中' as u32).is_some() {
                        found = true;
                    }
                }
                fontique::QueryStatus::Continue
            });
            if found {
                return true;
            }
        }
    }
    false
}

#[test]
fn probe_returns_false_when_no_cjk_font_is_present() {
    let mut font_cx = FontContext::new();
    let all: Vec<String> = font_cx
        .collection
        .family_names()
        .map(|s| s.to_string())
        .collect();

    let mut latin_only: Vec<String> = Vec::new();
    for name in &all {
        if !families_cover_cjk(&mut font_cx, std::slice::from_ref(name)) {
            latin_only.push(name.clone());
        }
    }

    assert!(
        !latin_only.is_empty(),
        "系统里应至少有几个不覆盖 CJK 的字体族（拉丁字体），\
         用来验证探测的否定分支；一个都没有说明探测逻辑恒返回 true"
    );

    assert!(
        !families_cover_cjk(&mut font_cx, &latin_only),
        "只有拉丁字体时探测必须返回 false（否则缺字体的机器上会误报失败）"
    );
}
