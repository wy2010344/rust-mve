//! 同步内存剪贴板（对标 TS `canvasRender2/Clipboard.ts`，实现抽自 `canvasCore/Clipboard`）。
//!
//! 编辑器在同步按键流里取用，走**同步内存 API**；优先尝试系统剪贴板，失败回退内存。

use std::sync::Mutex;

static MEM: Mutex<String> = Mutex::new(String::new());

/// 读取剪贴板文本：优先系统剪贴板，失败回退内存。
///
/// 对标 TS `clipboardGetText`。
#[must_use]
pub fn clipboard_get_text() -> String {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        if let Ok(t) = cb.get_text() {
            return t;
        }
    }
    MEM.lock().map(|g| g.clone()).unwrap_or_default()
}

/// 写入剪贴板文本：系统剪贴板 + 内存各一份。
///
/// 对标 TS `clipboardSetText`。
pub fn clipboard_set_text(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
    if let Ok(mut g) = MEM.lock() {
        *g = text.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_clipboard_roundtrip() {
        clipboard_set_text("secret");
        assert_eq!(clipboard_get_text(), "secret");
    }
}
