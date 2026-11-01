//! 显示层 tab 展开工具（对标 TS `canvasRender2/TabRender.ts`）。
//!
//! 段落（CanvasKit）对 `'\t'` 的宽度是未定义的，因此显示文本统一展开为空格。
//! 逻辑索引（模型文本，含 `'\t'`）与显示索引（展开后文本）的换算在编辑器
//! 光标/选区映射时使用（见 EditableTextNode 的 displayToLogicIndex 重写）。
//! demo 仅用到 [`expand_tabs`]，索引换算接口留待编辑器阶段。

/// 单个 tab 展开的等效空格数（固定 4，对标 Kotlin `SPACES_PER_TAB`）。
pub const SPACES_PER_TAB: usize = 4;

/// 显示层 tab 展开：逐字符替换。
#[must_use]
pub fn expand_tabs(text: &str) -> String {
    text.replace('\t', " ".repeat(SPACES_PER_TAB).as_str())
}

/// 逻辑索引 → 显示索引：每个 `'\t'` 贡献 [`SPACES_PER_TAB`] 个显示位，其余 1 个。
/// 越界时钳制到 `[0, text.len()]`。
#[must_use]
pub fn logic_to_display(text: &str, logic_pos: usize) -> usize {
    let limit = logic_pos.min(text.len());
    let mut display = 0usize;
    let chars: Vec<char> = text.chars().collect();
    for &c in &chars[..limit] {
        display += if c == '\t' { SPACES_PER_TAB } else { 1 };
    }
    display
}

/// 显示索引 → 逻辑索引：反向扫描；落在展开区间中间时吸附到 tab 之后。
/// 越界（>= 显示总长）时返回 `text.len()`。
#[must_use]
pub fn display_to_logic(text: &str, display_pos: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut logic = 0usize;
    let mut disp = 0usize;
    while logic < len && disp < display_pos {
        disp += if chars[logic] == '\t' {
            SPACES_PER_TAB
        } else {
            1
        };
        logic += 1;
    }
    logic
}
