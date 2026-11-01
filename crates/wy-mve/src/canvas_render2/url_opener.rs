//! 全局链接打开器（对标 TS `canvasRender2/UrlOpener.ts`）。
//!
//! 由各平台宿主注册实现（Web 的 `window.open`、Desktop 的 `browse`、
//! Android 的 `Intent`）。未注册时点击链接静默忽略——引擎层不持有平台环境。

use std::cell::RefCell;

thread_local! {
    static OPENER: RefCell<Option<fn(&str)>> = const { RefCell::new(None) };
}

/// 注册 / 清除平台打开实现（传 `None` 恢复静默忽略）。
pub fn set_url_opener(open: Option<fn(&str)>) {
    OPENER.with(|o| *o.borrow_mut() = open);
}

/// 打开链接；未注册 opener 时静默忽略。
pub fn open_url(url: &str) {
    OPENER.with(|o| {
        if let Some(f) = o.borrow().as_ref().copied() {
            f(url);
        }
    });
}
