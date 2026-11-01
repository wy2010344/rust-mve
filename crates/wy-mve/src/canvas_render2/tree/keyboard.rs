//! 键盘路由（TS `tree/keyboard.ts`）。
//!
//! 这里只放**规则**：谁先拿到按键、什么组合键算全局快捷键、Tab 什么时候才切焦点。
//! 具体动作（写编辑器、改选择器、动全局焦点、吞默认行为）全由 [`KeyRouterDeps`] 提供。
//!
//! 分层理由：规则在两侧是"向能力对象发消息的顺序"，动作才是宿主自己的事
//! （浏览器要 `preventDefault`，宿主侧不一定需要）。

use std::rc::Rc;

use crate::canvas_render2::focus::{KeyCode2, KeyEvent2};
use crate::canvas_render2::node2::Node2;

/// 键盘消费目标的最小契约（TS `KeyTarget2`）。
///
/// 只要"能处理键盘事件"就算——编辑器句柄、普通节点都满足（结构化），
/// 路由不关心它到底是什么，也不读它别的成员。
pub trait KeyTarget2 {
    /// 处理按键；返回 `true` = 已消费。
    fn handle_key(&self, e: &KeyEvent2) -> bool;
    /// 该目标是否与这个节点是同一个对象。
    ///
    /// 对应 TS 路由里的 `focused !== editor` 引用比较——避免"既是活跃编辑器、
    /// 又是焦点节点"时把同一个按键消费两遍。Rust 没有跨具体类型的引用相等，
    /// 故由实现自报；非节点的编辑器句柄默认 `false` 即可。
    fn is_same_node(&self, _node: &Node2) -> bool {
        false
    }
}

impl KeyTarget2 for Node2 {
    fn handle_key(&self, e: &KeyEvent2) -> bool {
        self.handle_key(e)
    }
    fn is_same_node(&self, node: &Node2) -> bool {
        self == node
    }
}

/// 键盘路由需要的外部能力（TS `KeyRouterDeps`）。
pub trait KeyRouterDeps {
    /// 输入法组合中：键盘归输入法独占，整条路由放弃。
    fn composing(&self) -> bool;
    /// 当前活跃编辑器（无则 `None`）。
    fn active_editor(&self) -> Option<Rc<dyn KeyTarget2>>;
    /// 当前焦点节点（无则 `None`）。
    fn focused(&self) -> Option<Node2>;
    /// 全选：编辑器存在时写进编辑器本地光标，否则走选择器。
    fn select_all(&self);
    /// 复制。
    fn copy(&self);
    /// 剪切。
    fn cut(&self);
    /// 粘贴：内容由宿主的 paste 事件异步提供，这里只负责吞掉默认行为。
    fn paste_pending(&self);
    /// Tab 焦点遍历；返回是否真的移动了焦点。
    fn move_focus(&self, forward: bool) -> bool;
    /// 谁都没消费的按键：广播给全局旁听者（快捷键面板 / 调试）。
    fn notify_unhandled(&self, event: &KeyEvent2);
}

/// 处理一次按键，返回宿主是否需要 `preventDefault`（即"已被消费"）。
///
/// 顺序与 Kotlin `Renderer.handleKey` 对齐：
/// 1. 组合输入期间直接放弃；
/// 2. 全局快捷键（Ctrl/Cmd + A/C/X/V，且不带 Alt）；
/// 3. 活跃编辑器优先（节点消息，编辑器会自己处理 Tab 插入等）；
/// 4. 其它焦点节点；
/// 5. 都没消费才轮到 Tab 焦点遍历，且排除 Alt/Ctrl/Meta 组合（Alt+Tab 交给系统）。
pub fn route_key_event(deps: &impl KeyRouterDeps, event: &KeyEvent2) -> bool {
    if deps.composing() {
        return false;
    }

    let editor = deps.active_editor();
    let accel = event.ctrl || event.meta;
    if accel && !event.alt {
        match event.key.to_lowercase().as_str() {
            "a" => {
                deps.select_all();
                return true;
            }
            "c" => {
                deps.copy();
                return true;
            }
            "x" => {
                deps.cut();
                return true;
            }
            "v" => {
                deps.paste_pending();
                return true;
            }
            _ => {}
        }
    }

    // 先给焦点节点 / 编辑器消费（对齐 Kotlin Renderer.handleKey：节点优先，
    // 未被消费才走 Tab 焦点遍历）。否则编辑器永远拿不到 Tab（无法插入 \t）。
    if let Some(ed) = editor.as_ref() {
        if ed.handle_key(event) {
            return true;
        }
    }

    if let Some(f) = deps.focused() {
        let is_editor = editor.as_ref().is_some_and(|e| e.is_same_node(&f));
        if !is_editor && f.handle_key(event) {
            return true;
        }
    }

    if event.code == KeyCode2::TAB && !event.alt && !event.ctrl && !event.meta {
        return deps.move_focus(!event.shift);
    }
    // 到这儿说明没人认领：交给全局旁听（不影响返回值，仍视为未消费）
    deps.notify_unhandled(event);
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// 记录型 deps：编辑器 / 焦点节点的消费与否都可在测试里指定。
    struct TestDeps {
        composing: bool,
        editor_take: bool,
        /// 编辑器与焦点节点是不是同一对象
        editor_is_focused: bool,
        focus_moved: bool,
        log: RefCell<Vec<String>>,
        node: Node2,
    }

    struct TestEditor {
        take: bool,
        node: Option<Node2>,
    }

    impl KeyTarget2 for TestEditor {
        fn handle_key(&self, _e: &KeyEvent2) -> bool {
            self.take
        }
        fn is_same_node(&self, node: &Node2) -> bool {
            self.node.as_ref() == Some(node)
        }
    }

    impl KeyRouterDeps for TestDeps {
        fn composing(&self) -> bool {
            self.composing
        }
        fn active_editor(&self) -> Option<Rc<dyn KeyTarget2>> {
            Some(Rc::new(TestEditor {
                take: self.editor_take,
                node: self.editor_is_focused.then(|| self.node.clone()),
            }))
        }
        fn focused(&self) -> Option<Node2> {
            Some(self.node.clone())
        }
        fn select_all(&self) {
            self.log.borrow_mut().push("selectAll".into());
        }
        fn copy(&self) {
            self.log.borrow_mut().push("copy".into());
        }
        fn cut(&self) {
            self.log.borrow_mut().push("cut".into());
        }
        fn paste_pending(&self) {
            self.log.borrow_mut().push("paste".into());
        }
        fn move_focus(&self, forward: bool) -> bool {
            self.log.borrow_mut().push(format!("moveFocus:{forward}"));
            self.focus_moved
        }
        fn notify_unhandled(&self, _event: &KeyEvent2) {
            self.log.borrow_mut().push("unhandled".into());
        }
    }

    fn deps(take: bool) -> TestDeps {
        TestDeps {
            composing: false,
            editor_take: take,
            editor_is_focused: false,
            focus_moved: true,
            log: RefCell::new(Vec::new()),
            node: Node2::empty(),
        }
    }

    fn key(k: &str, code: &str) -> KeyEvent2 {
        KeyEvent2 {
            key: k.into(),
            code: code.into(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    fn accel(k: &str) -> KeyEvent2 {
        KeyEvent2 {
            ctrl: true,
            ..key(k, "")
        }
    }

    /// 一个会消费按键的节点（用来验证"焦点节点"那一档）。
    fn consuming_node() -> Node2 {
        let a = crate::canvas_render2::node2::Node2Props {
            handle_key: Some(std::rc::Rc::new(|_| true)),
            ..crate::canvas_render2::node2::Node2Props::default()
        };
        Node2::empty().build(a)
    }

    #[test]
    fn composing_abandons_the_whole_route() {
        let mut d = deps(true);
        d.composing = true;
        assert!(!route_key_event(&d, &accel("a")), "组合输入期间一律不消费");
        assert!(d.log.borrow().is_empty(), "连全局快捷键都不该触发");
    }

    #[test]
    fn accel_shortcuts_win_over_everything() {
        for (k, want) in [
            ("a", "selectAll"),
            ("c", "copy"),
            ("x", "cut"),
            ("v", "paste"),
        ] {
            let d = deps(true);
            assert!(route_key_event(&d, &accel(k)), "Ctrl+{k} 应被消费");
            assert_eq!(d.log.borrow().clone(), vec![want.to_string()]);
        }
        // 大写的 'A'（Shift+Ctrl+A）同样算
        let d = deps(true);
        assert!(route_key_event(&d, &accel("A")));
        assert_eq!(d.log.borrow().clone(), vec!["selectAll".to_string()]);
    }

    #[test]
    fn alt_excludes_accel_shortcuts() {
        let d = deps(false);
        let mut e = accel("a");
        e.alt = true;
        assert!(!route_key_event(&d, &e), "Ctrl+Alt+A 不是全选");
        assert!(!d.log.borrow().contains(&"selectAll".to_string()));
    }

    #[test]
    fn editor_is_asked_before_focused_node() {
        // 编辑器消费 → 焦点节点那一档不再调用（`is_same_node` 为 false 时会调，
        // 这里编辑器先返回 true 直接短路）
        let d = deps(true);
        assert!(route_key_event(&d, &key("a", "KeyA")));
        assert!(
            !d.log.borrow().contains(&"unhandled".to_string()),
            "编辑器消费了就不该走到未处理广播"
        );
    }

    #[test]
    fn focused_node_is_not_asked_twice_when_it_is_the_editor() {
        let mut d = deps(false);
        d.editor_is_focused = true;
        d.node = consuming_node();
        // 编辑器（= 焦点节点，不消费）→ 焦点节点跳过 → Tab 判定 → 未处理
        assert!(!route_key_event(&d, &key("a", "KeyA")));
        assert_eq!(d.log.borrow().clone(), vec!["unhandled".to_string()]);
    }

    #[test]
    fn tab_moves_focus_only_without_modifiers() {
        let d = deps(false);
        assert!(route_key_event(&d, &key("\t", KeyCode2::TAB)));
        assert_eq!(d.log.borrow().clone(), vec!["moveFocus:true".to_string()]);

        // Shift+Tab → 反向
        let d = deps(false);
        let mut e = key("\t", KeyCode2::TAB);
        e.shift = true;
        assert!(route_key_event(&d, &e));
        assert_eq!(d.log.borrow().clone(), vec!["moveFocus:false".to_string()]);

        // Alt+Tab → 交给系统，不切焦点
        let d = deps(false);
        let mut e = key("\t", KeyCode2::TAB);
        e.alt = true;
        assert!(!route_key_event(&d, &e));
        assert_eq!(d.log.borrow().clone(), vec!["unhandled".to_string()]);
    }

    #[test]
    fn unhandled_is_broadcast_and_still_not_consumed() {
        let d = deps(false);
        assert!(!route_key_event(&d, &key("q", "KeyQ")));
        assert_eq!(d.log.borrow().clone(), vec!["unhandled".to_string()]);
    }
}
