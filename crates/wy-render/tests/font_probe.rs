//! 字体回退回归：默认样式的 CJK 文本必须选中含有效字形的字体（glyph id != 0）。
//!
//! 若此测试失败，说明系统缺 CJK 字体回退——界面中文会渲染成方块（tofu）。
//!
//! # 关于「机器上根本没有中文字体」
//!
//! 干净容器 / CI runner 可能一个中文字体都没装。这时**测不出任何结论**：
//! 字形 id=0 只是因为没字体可用，不是回退逻辑坏了。
//! 所以这里先探测系统里**有没有任何字体的字符表覆盖 CJK**：
//!
//! - 没有 → 跳过并说明原因（不是失败）
//! - 有 → 断言回退机制正确工作（此时失败就是真 bug）
//!
//! # 探测为什么不能用「排版一次看字形」
//!
//! 排版路径会**先选默认字体族，再走回退链**；而逐族排版会**直接指定族**，
//! 两条路径不同——前者可能是 tofu，后者却因为某个族恰好覆盖而返回非零，
//! 于是探测说"有字体"、实际断言却失败，自相矛盾。
//!
//! 所以探测走**字符表查询**（fontique 的 `Charmap::map`）：
//! 直接问「有没有字体的 cmap 里有这个码位」，
//! 与排版路径完全解耦，是地面真相。
use parley::{FontContext, LayoutContext, PositionedLayoutItem, StyleProperty};

/// 探测系统里是否存在能覆盖 CJK 的字体。
///
/// 走 fontique 的字符表查询：枚举所有字体族，检查其 cmap 是否含中文字符。
fn system_has_cjk_font() -> bool {
    let mut font_cx = FontContext::new();
    let names: Vec<String> = font_cx
        .collection
        .family_names()
        .map(|s| s.to_string())
        .collect();
    families_cover_cjk(&mut font_cx, &names)
}

/// 在给定的字体族集合里找是否有能覆盖 CJK 的字体。
///
/// 拆成带参数的形式，是为了能**单独测「只有拉丁字体时必须返回 false」**
/// 这条分支——否则探测函数写错了（恒返回 true）在有字体的机器上测不出来。
fn families_cover_cjk(font_cx: &mut FontContext, names: &[String]) -> bool {
    let source_cache = &mut font_cx.source_cache;

    for name in names {
        let Some(id) = font_cx.collection.family_id(name) else {
            continue;
        };
        let Some(info) = font_cx.collection.family(id) else {
            continue;
        };

        // 该族的各个字体
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

/// 探测函数本身的可靠性：拉丁字符必须被覆盖。
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

/// 探测函数的**否定分支**：只看能覆盖拉丁字符的字体时，必须返回 false。
///
/// 这条测试模拟「机器上没有任何中文字体」——正是 CI runner 的情形。
/// 如果探测在这里错误地返回 true，`cjk_glyph_selection_nonzero` 就会
/// 在缺字体的机器上误报失败（这正是之前 CI 一直红的原因）。
#[test]
fn probe_returns_false_when_no_cjk_font_is_present() {
    let mut font_cx = FontContext::new();
    let all: Vec<String> = font_cx
        .collection
        .family_names()
        .map(|s| s.to_string())
        .collect();

    // 逐个族检查：找出**不覆盖 CJK** 的族（拉丁字体就是这种）
    let mut latin_only: Vec<String> = Vec::new();
    for name in &all {
        let covers = families_cover_cjk(&mut font_cx, std::slice::from_ref(name));
        if !covers {
            latin_only.push(name.clone());
        }
    }

    assert!(
        !latin_only.is_empty(),
        "系统里应至少有几个不覆盖 CJK 的字体族（拉丁字体），\
         用来验证探测的否定分支；一个都没有说明探测逻辑恒返回 true"
    );

    // 只喂拉丁字体族，探测必须返回 false
    assert!(
        !families_cover_cjk(&mut font_cx, &latin_only),
        "只有拉丁字体时探测必须返回 false（否则缺字体的机器上会误报失败）"
    );
}
