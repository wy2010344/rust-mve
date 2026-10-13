//! 系统剪贴板桥：wy-mve 定义 thread_local 注入点，wy-engine 用 arboard 注册实现。
//!
//! 组件层（`text_field`）通过 [`clipboard_get`] / [`clipboard_set`] 读写剪贴板，
//! 不直接依赖平台库——保持 wy-mve 的平台无关性，实现由宿主注入。

use std::cell::RefCell;
use std::rc::Rc;

/// 剪贴板实现（平台绑定）。
pub trait Clipboard {
    /// 读取当前文本（无文本时返回 `None`）。
    fn get_text(&self) -> Option<String>;
    /// 写入文本。
    fn set_text(&self, text: &str);
}

thread_local! {
    static CLIPBOARD: RefCell<Option<Rc<dyn Clipboard>>> = RefCell::new(None);
}

/// 注册剪贴板实现（引擎启动时调用一次）。
pub fn set_clipboard(impl_: impl Clipboard + 'static) {
    CLIPBOARD.with(|c| *c.borrow_mut() = Some(Rc::new(impl_)));
}

/// 读取剪贴板文本；未注册实现时返回 `None`。
pub fn clipboard_get() -> Option<String> {
    CLIPBOARD.with(|c| c.borrow().as_ref().and_then(|c| c.get_text()))
}

/// 写入剪贴板文本；未注册实现时为空操作。
pub fn clipboard_set(text: &str) {
    CLIPBOARD.with(|c| {
        if let Some(c) = c.borrow().as_ref() {
            c.set_text(text);
        }
    });
}

/// 测试用：清空注册（thread_local，仅当前线程可见）。
#[cfg(test)]
pub(crate) fn reset_for_tests() {
    CLIPBOARD.with(|c| *c.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct MemoryClipboard(RefCell<Option<String>>);

    impl Clipboard for MemoryClipboard {
        fn get_text(&self) -> Option<String> {
            self.0.borrow().clone()
        }
        fn set_text(&self, text: &str) {
            *self.0.borrow_mut() = Some(text.to_string());
        }
    }

    fn reset() {
        CLIPBOARD.with(|c| *c.borrow_mut() = None);
    }

    // thread_local 全局可变，多测试并行会互踩——合并为单个测试串行覆盖。
    #[test]
    fn clipboard_noop_then_roundtrip() {
        reset();
        assert_eq!(clipboard_get(), None);
        clipboard_set("x"); // 未注册：no-op 不 panic

        set_clipboard(MemoryClipboard(RefCell::new(None)));
        clipboard_set("hello");
        assert_eq!(clipboard_get().as_deref(), Some("hello"));
        clipboard_set("");
        assert_eq!(clipboard_get().as_deref(), Some(""));

        reset();
        assert_eq!(clipboard_get(), None);
    }
}
