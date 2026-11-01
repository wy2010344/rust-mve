//! 引擎级全局状态（TS `tree/engineGlobal.ts` 的 1:1 复刻）。
//!
//! 与 `tree/protocol` 同属「算法层可依赖、但不依赖具体节点的纯模块」：
//! 树算法（`messages` 的默认实现、`dispatch` 的派发、`focusWalk` 的焦点遍历）
//! 只向这里读信号 / 写状态，不碰宿主。宿主（`Renderer`）负责把 DOM 事件灌进来、
//! 把全局焦点喂给键盘 / IME 路由。
//!
//! 从 `Focus.ts` 下沉到这里是为了切断 `tree/*` 对 `../Focus` 的依赖环：
//! `Focus` 现在只 re-export 本文件 + 持有编辑器句柄的强类型
//! （`EditableTextHandle`）。Rust 侧同理：[`crate::canvas_render2::focus`] 只
//! re-export 本模块并负责 `EditorHandleExt` → 强类型句柄的下转。
//!
//! ## 与 TS 的唯一结构性差别
//!
//! TS **导出信号对象本身**（`focusedSignal` / `ctrlSignal` …），调用方自己
//! `.get()` / `.set()`。Rust 没有模块级可变状态，信号放在 `thread_local` 里，
//! 故以访问器函数导出（`focused_node()` / `ctrl()` …）。读写语义与 TS 一致，
//! 且在 memo / track 闭包内读取同样会自动建立依赖（焦点变化触发重绘）。

use std::cell::RefCell;
use std::rc::Rc;

use wy_signal::{create_signal, GetValue, SetValue, Signal};

use crate::canvas_render2::focus::KeyEvent2;
use crate::canvas_render2::node2::Node2;
use crate::canvas_render2::protocol::EditorHandleExt;

/// 全局按键旁听的回调类型（TS 模块级 `Set<(e: KeyEvent2) => void>` 的元素类型）。
type KeyPressCb = Rc<dyn Fn(&KeyEvent2)>;

thread_local! {
    /// 焦点真相源（TS `focusedSignal`，对标 Kotlin `EngineGlobal.focused`）。
    static FOCUSED: Signal<Option<Node2>> = create_signal(None);
    /// 当前活跃编辑器标记（TS `activeEditorSignal`）。
    ///
    /// 存的是协议层的标记实例（含 `impl`），强类型句柄由
    /// [`crate::canvas_render2::focus::active_editor`] 下转取用。
    static ACTIVE_EDITOR: Signal<Option<EditorHandleExt>> = create_signal(None);
    /// 按压态真相源（TS `pressedSignal`，对标 Kotlin `EngineGlobal.pressed`）：
    /// 最近一次左键按下落到的叶子节点。
    ///
    /// 与焦点解耦：焦点可能已转移，但"正按住谁"是另一回事。
    static PRESSED: Signal<Option<Node2>> = create_signal(None);
    /// 修饰键实时状态（TS `ctrlSignal`，对标 Kotlin `EngineGlobal.ctrl`）。
    static CTRL: Signal<bool> = create_signal(false);
    /// TS `shiftSignal`。
    static SHIFT: Signal<bool> = create_signal(false);
    /// TS `altSignal`。
    static ALT: Signal<bool> = create_signal(false);
    /// TS `metaSignal`。
    static META: Signal<bool> = create_signal(false);
    /// 全局按键旁听（TS 模块级 `keyPressListeners: Set`）。
    static KEY_PRESS_LISTENERS: RefCell<Vec<KeyPressCb>> =
        const { RefCell::new(Vec::new()) };
}

/// 当前持有焦点的节点（TS `focusedSignal.get()`）。
#[must_use]
pub fn focused_node() -> Option<Node2> {
    FOCUSED.with(|s| s.get())
}

/// 当前是否持有焦点（TS `isFocused(node)`）。
///
/// 响应式读取：在 memo / track 闭包内调用会在焦点变化时自动触发重绘。
#[must_use]
pub fn is_focused(node: &Node2) -> bool {
    focused_node().as_ref() == Some(node)
}

/// 请求获得焦点（TS `requestFocus(node)`）。
///
/// 编辑器节点自动登记为活跃编辑器（取它的 [`EditorHandleExt`]），否则清空活跃
/// 编辑器——与 TS `activeEditorSignal.set(node.findExt(EditorHandleExt))` 一致。
pub fn request_focus(node: &Node2) {
    FOCUSED.with(|s| s.set(Some(node.clone())));
    let ext = node.find_ext_rc::<EditorHandleExt>().map(|r| (*r).clone());
    ACTIVE_EDITOR.with(|e| e.set(ext));
}

/// 清空焦点（TS `clearFocus()`，如窗口失焦）。
pub fn clear_focus() {
    FOCUSED.with(|s| s.set(None));
    ACTIVE_EDITOR.with(|e| e.set(None));
}

/// 当前活跃编辑器的**协议层标记**（TS `activeEditorSignal.get()`）。
///
/// 要强类型句柄用 [`crate::canvas_render2::focus::active_editor`]。
#[must_use]
pub fn active_editor_ext() -> Option<EditorHandleExt> {
    ACTIVE_EDITOR.with(|e| e.get())
}

/// 当前正被按住的叶子节点（TS `pressedSignal.get()`）。
#[must_use]
pub fn pressed_node() -> Option<Node2> {
    PRESSED.with(|s| s.get())
}

/// 当前是否正被按住（TS `isPressed(node)`）。
#[must_use]
pub fn is_pressed(node: &Node2) -> bool {
    pressed_node().as_ref() == Some(node)
}

/// 写入按压态（TS `pressedSignal.set(...)`，由指针路由在 down / up 时调用）。
pub fn set_pressed(node: Option<&Node2>) {
    PRESSED.with(|s| s.set(node.cloned()));
}

/// 修饰键快照（TS `ctrlSignal` / `shiftSignal` / `altSignal` / `metaSignal` 的合读）。
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Modifiers {
    /// Ctrl。
    pub ctrl: bool,
    /// Shift。
    pub shift: bool,
    /// Alt。
    pub alt: bool,
    /// Meta（Cmd / Win）。
    pub meta: bool,
}

/// 读取修饰键快照（TS 逐个 `signal.get()`）。
#[must_use]
pub fn modifiers() -> Modifiers {
    Modifiers {
        ctrl: CTRL.with(|s| s.get()),
        shift: SHIFT.with(|s| s.get()),
        alt: ALT.with(|s| s.get()),
        meta: META.with(|s| s.get()),
    }
}

/// 宿主上报修饰键快照（TS `applyModifiers`）。
pub fn apply_modifiers(ctrl: bool, shift: bool, alt: bool, meta: bool) {
    CTRL.with(|s| s.set(ctrl));
    SHIFT.with(|s| s.set(shift));
    ALT.with(|s| s.set(alt));
    META.with(|s| s.set(meta));
}

/// 清空修饰键（TS `clearModifiers()`，窗口失焦等，防止修饰键残留）。
pub fn clear_modifiers() {
    apply_modifiers(false, false, false, false);
}

/// 注册全局按键旁听，返回注销句柄（TS `registerKeyPress`）。
///
/// TS 用 `Set` 去重（同一函数引用注册两次只算一次）；Rust 侧按 `Rc` 引用同一性
/// 去重，语义等价。
pub fn register_key_press(callback: KeyPressCb) -> impl Fn() {
    let registered = Rc::clone(&callback);
    KEY_PRESS_LISTENERS.with(|l| {
        let mut l = l.borrow_mut();
        if !l.iter().any(|it| Rc::ptr_eq(it, &registered)) {
            l.push(registered);
        }
    });
    move || {
        KEY_PRESS_LISTENERS.with(|l| {
            l.borrow_mut().retain(|it| !Rc::ptr_eq(it, &callback));
        })
    }
}

/// 广播一次未被消费的按键（TS `notifyKeyPress`；Renderer 在路由结束后调用）。
pub fn notify_key_press(e: &KeyEvent2) {
    let listeners: Vec<KeyPressCb> = KEY_PRESS_LISTENERS.with(|l| l.borrow().clone());
    for cb in listeners {
        cb(e);
    }
}

/// 重置全部全局状态（测试用；TS 无对应函数，因为页面刷新即重置）。
#[doc(hidden)]
pub fn reset() {
    clear_focus();
    set_pressed(None);
    clear_modifiers();
    KEY_PRESS_LISTENERS.with(|l| l.borrow_mut().clear());
    clear_region_probes();
}

// ============================================================================
// 动态区域探针（纯 Node2 模式的 `for_each` / `if`）
// ============================================================================

/// 动态区域探针：读一次该区域的 memo（脏了就重算 → 重建这一段子节点）。
///
/// # 为什么需要它
///
/// 旧引擎每帧读 `Root.target`，`GetNodes` 槽会把动态区域重新求值；纯 Node2
/// 树没有那个"每帧读一遍"的时机，区域 memo 会一直躺在那里不重算。
/// 把每个区域登记成探针，绘制前统一读一遍——既触发首次求值（首帧就有内容），
/// 也让 `Signal` 变化经 memo 传到重绘追踪器（绘制闭包内读取 → 自动重绘）。
type RegionProbe = Rc<dyn Fn()>;

thread_local! {
    /// 当前树上所有动态区域的探针（`build_root_node2` 重建时清空）。
    static REGION_PROBES: RefCell<Vec<RegionProbe>> = const { RefCell::new(Vec::new()) };
}

/// 登记一个动态区域探针。
pub fn register_region_probe(probe: RegionProbe) {
    REGION_PROBES.with(|p| p.borrow_mut().push(probe));
}

/// 依次读一遍所有区域（脏的会重算并就地更新子节点）。绘制前调用。
pub fn probe_regions() {
    REGION_PROBES.with(|p| {
        for probe in p.borrow().iter() {
            probe();
        }
    });
}

/// 清空探针（树要重建时调用；旧的区域已经不属于新树）。
pub fn clear_region_probes() {
    REGION_PROBES.with(|p| p.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::node2::Node2Props;
    use crate::canvas_render2::protocol::EditorHandleExt;

    fn key() -> KeyEvent2 {
        KeyEvent2 {
            key: "a".to_string(),
            code: String::new(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    fn node() -> Node2 {
        Node2::empty().build(Node2Props::default())
    }

    fn editor_node() -> Node2 {
        Node2::empty().build(Node2Props {
            exts: vec![Rc::new(EditorHandleExt(
                Rc::new(0u8) as Rc<dyn std::any::Any>
            ))],
            ..Node2Props::default()
        })
    }

    #[test]
    fn request_focus_sets_active_editor_from_ext() {
        reset();
        let n = editor_node();
        assert!(!is_focused(&n));
        request_focus(&n);
        assert!(is_focused(&n));
        assert!(active_editor_ext().is_some());

        // 非编辑器节点：焦点转移后活跃编辑器被清空（TS `findExt` 返回 undefined）
        let plain = node();
        request_focus(&plain);
        assert!(is_focused(&plain));
        assert!(!is_focused(&n));
        assert!(active_editor_ext().is_none());
        reset();
    }

    #[test]
    fn clear_focus_drops_both() {
        reset();
        let n = editor_node();
        request_focus(&n);
        clear_focus();
        assert!(focused_node().is_none());
        assert!(active_editor_ext().is_none());
    }

    #[test]
    fn pressed_is_decoupled_from_focus() {
        reset();
        let a = node();
        let b = node();
        request_focus(&a);
        set_pressed(Some(&b));
        assert!(is_pressed(&b));
        assert!(!is_pressed(&a));
        assert!(is_focused(&a));
        set_pressed(None);
        assert!(pressed_node().is_none());
        reset();
    }

    #[test]
    fn modifiers_round_trip_and_clear() {
        reset();
        assert_eq!(modifiers(), Modifiers::default());
        apply_modifiers(true, false, true, false);
        assert_eq!(
            modifiers(),
            Modifiers {
                ctrl: true,
                shift: false,
                alt: true,
                meta: false
            }
        );
        clear_modifiers();
        assert_eq!(modifiers(), Modifiers::default());
    }

    #[test]
    fn key_press_listeners_are_notified_and_removable() {
        reset();
        let hits = Rc::new(RefCell::new(0usize));
        let h = Rc::clone(&hits);
        let cb: KeyPressCb = Rc::new(move |_| *h.borrow_mut() += 1);
        let off = register_key_press(Rc::clone(&cb));
        // 同一引用重复注册不叠加（TS `Set` 语义）
        let _off2 = register_key_press(Rc::clone(&cb));

        notify_key_press(&key());
        assert_eq!(*hits.borrow(), 1);

        off();
        notify_key_press(&key());
        assert_eq!(*hits.borrow(), 1);
        reset();
    }
}
