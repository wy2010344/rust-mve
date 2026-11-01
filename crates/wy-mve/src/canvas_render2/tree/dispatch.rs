//! 指针派发（TS `tree/dispatch.ts`）。
//!
//! 顺序固定：命中链上先**捕获**（根 → 叶）再**冒泡**（叶 → 根），
//! 任一阶段 `stopPropagation()` 即整体中断（对标 Kotlin `dispatchPointer`）。
//! **所有类型都走这两个阶段**（含右键菜单 / 双击 / 滚轮）——捕获阶段是
//! "外层先知道、可拦截"的唯一位置，滚轮嵌套滚动就靠它。
//!
//! 不检查钩子是否存在：指针钩子在协议里是必答消息（[`Node2`] 的十四个方法，
//! 缺省走空实现），所以这里直接发消息——与 TS 一致（不能查"有没有这个方法"）。

use crate::canvas_render2::node2::Node2;
use crate::canvas_render2::protocol::{
    create_mouse_event2, EditorHandleExt, MouseEvent2, MouseEventInit2, MouseType2, PointerDevice,
    POINTER_BUTTON_LEFT,
};
use crate::canvas_render2::tree::geometry::{NodeHit2, NodeWithPosition2};
use crate::primitives::CursorType;

/// 一次派发的公共输入（局部坐标由命中链提供，这里只带全局量与按键状态）。
///
/// 之所以做成一个对象而不是长参数表：ooc 侧就是 `dispatchMousePath $ path init`，
/// 加字段时两侧都不用改签名。
#[derive(Clone, Copy, Debug)]
pub struct MouseDispatchInit {
    /// 事件类型。
    pub type_: MouseType2,
    /// 全局（画布）横坐标。
    pub global_x: f32,
    /// 全局（画布）纵坐标。
    pub global_y: f32,
    /// 输入设备（鼠标 / 触摸 / 触控笔）。
    pub device: PointerDevice,
    /// 指针编号（多点触控区分；鼠标恒 0）。
    pub pointer_id: i32,
    /// 触发本次事件的按键编号。
    pub button: i32,
    /// 当前按住的按键位掩码。
    pub buttons: i32,
    /// 滚轮水平位移。
    pub wheel_delta_x: f32,
    /// 滚轮垂直位移。
    pub wheel_delta_y: f32,
}

/// 最简派发入参：只要类型与全局坐标，其余取默认值（TS `mouseDispatchOf`）。
#[must_use]
pub fn mouse_dispatch_of(type_: MouseType2, global_x: f32, global_y: f32) -> MouseDispatchInit {
    MouseDispatchInit {
        type_,
        global_x,
        global_y,
        device: PointerDevice::Mouse,
        pointer_id: 0,
        button: 0,
        buttons: 0,
        wheel_delta_x: 0.0,
        wheel_delta_y: 0.0,
    }
}

/// 向单个节点发一条指针消息；返回事件是否被中断（TS `sendMouse`）。
fn send_mouse(target: &Node2, event: &mut MouseEvent2, capture: bool) -> bool {
    match event.type_ {
        MouseType2::Click => {
            if capture {
                target.mouse_click_capture(event)
            } else {
                target.mouse_click(event)
            }
        }
        MouseType2::Down => {
            if capture {
                target.mouse_down_capture(event)
            } else {
                target.mouse_down(event)
            }
        }
        // 取消与抬起走同一组钩子（对标 Kotlin：Cancel → onPointerUp）
        MouseType2::Up | MouseType2::Cancel => {
            if capture {
                target.mouse_up_capture(event)
            } else {
                target.mouse_up(event)
            }
        }
        MouseType2::Move => {
            if capture {
                target.mouse_move_capture(event)
            } else {
                target.mouse_move(event)
            }
        }
        MouseType2::Wheel => {
            if capture {
                target.mouse_wheel_capture(event)
            } else {
                target.mouse_wheel(event)
            }
        }
        MouseType2::ContextMenu => {
            if capture {
                target.mouse_context_menu_capture(event)
            } else {
                target.mouse_context_menu(event)
            }
        }
        MouseType2::DoubleClick => {
            if capture {
                target.mouse_double_click_capture(event)
            } else {
                target.mouse_double_click(event)
            }
        }
    }
    event.stopped_progression()
}

/// 沿命中链派发：捕获（根 → 叶）→ 冒泡（叶 → 根）。
///
/// 返回是否被 `stopPropagation()` 中断（滚轮用它决定要不要再交给外层 / 页面）。
///
/// TS 签名是 `dispatchMousePath(path: readonly NodeHit2[], init)`。这里直接吃
/// `hit_path` 的产出 [`NodeWithPosition2`]——派发要读每节的局部坐标，只问节点的
/// [`NodeHit2`] 抽象不够用。
#[must_use]
pub fn dispatch_mouse_path(path: &[NodeWithPosition2], init: &MouseDispatchInit) -> bool {
    // 捕获阶段：根 → 叶
    for it in path.iter() {
        if send_mouse(&it.node, &mut event_of(it, init), true) {
            return true;
        }
    }
    // 冒泡阶段：叶 → 根
    for it in path.iter().rev() {
        if send_mouse(&it.node, &mut event_of(it, init), false) {
            return true;
        }
    }
    false
}

/// 每个节点拿到的坐标是**相对该节点**的局部坐标（TS `eventOf`）。
#[must_use]
fn event_of(it: &NodeWithPosition2, init: &MouseDispatchInit) -> MouseEvent2 {
    create_mouse_event2(MouseEventInit2 {
        type_: init.type_,
        x: it.x,
        y: it.y,
        global_x: init.global_x,
        global_y: init.global_y,
        device: Some(init.device),
        pointer_id: Some(init.pointer_id),
        button: Some(init.button),
        buttons: Some(init.buttons),
        wheel_delta_x: Some(init.wheel_delta_x),
        wheel_delta_y: Some(init.wheel_delta_y),
        pressure: None,
    })
}

/// 光标派生（对标 Kotlin `cursorTrack`，TS `cursorOfPath`）：
/// 叶子优先——节点显式 `cursor()` 优先；其次编辑器 → text，可聚焦 → pointer，
/// 否则 default。全部按消息读取，不关心节点到底是谁。
#[must_use]
pub fn cursor_of_path(path: Option<&[impl NodeHit2]>) -> CursorType {
    let mut pointer = false;
    if let Some(path) = path {
        for it in path.iter().rev() {
            let n = it.hit_node();
            if let Some(c) = n.cursor() {
                return c;
            }
            if n.find_ext_rc::<EditorHandleExt>().is_some() {
                return CursorType::Text;
            }
            if n.focusable() {
                pointer = true;
            }
        }
    }
    if pointer {
        CursorType::Pointer
    } else {
        CursorType::Default
    }
}

/// 该节点是否在命中链上（hover / pressed 判定用，TS `pathIncludes`）。
#[must_use]
pub fn path_includes(path: Option<&[impl NodeHit2]>, node: &Node2) -> bool {
    match path {
        Some(path) => path.iter().any(|it| it.hit_node() == node),
        None => false,
    }
}

/// 命中点是否落在左键上（右键不进入拖选等左键专属行为，TS `isPrimaryButton`）。
#[must_use]
pub fn is_primary_button(button: i32) -> bool {
    button == POINTER_BUTTON_LEFT
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::node2::{Node2, Node2Props};
    use crate::canvas_render2::protocol::EditorHandleExt;
    use crate::canvas_render2::tree::geometry::hit_path;
    use crate::primitives::CursorType;
    use crate::{Fc, NodeContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn leaf(w: f32, h: f32) -> Node2Props {
        Node2Props {
            width: Some(Rc::new(move || w)),
            height: Some(Rc::new(move || h)),
            ..Node2Props::default()
        }
    }

    fn attach_child(parent: &Node2, arg: Node2Props) {
        let mut cx = NodeContext::new();
        cx.node2_parent = Some(parent.clone());
        Fc::new(move |cx| {
            Node2::empty().build(arg).mount(cx);
        })
        .mount(&mut cx);
    }

    #[test]
    fn dispatch_goes_capture_then_bubble() {
        // 两层：root → child。捕获 root→child，冒泡 child→root。
        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        let mut root_a = leaf(200.0, 200.0);
        root_a.x = Some(Rc::new(|| 0.0));
        root_a.y = Some(Rc::new(|| 0.0));
        let lc = Rc::clone(&log);
        root_a.mouse_down_capture = Some(Rc::new(move |_| {
            lc.borrow_mut().push("root:capture".into())
        }));
        let lb = Rc::clone(&log);
        root_a.mouse_down = Some(Rc::new(move |_| lb.borrow_mut().push("root:bubble".into())));

        let mut child_a = leaf(80.0, 80.0);
        child_a.x = Some(Rc::new(|| 10.0));
        child_a.y = Some(Rc::new(|| 10.0));
        let lc2 = Rc::clone(&log);
        child_a.mouse_down_capture = Some(Rc::new(move |_| {
            lc2.borrow_mut().push("child:capture".into())
        }));
        let lb2 = Rc::clone(&log);
        child_a.mouse_down = Some(Rc::new(move |_| {
            lb2.borrow_mut().push("child:bubble".into())
        }));

        let root = Node2::empty().build(root_a);
        attach_child(&root, child_a);

        let path = hit_path(&root, 30.0, 30.0);
        assert_eq!(path.len(), 2, "命中链应当是 root → child");

        let stopped = dispatch_mouse_path(&path, &mouse_dispatch_of(MouseType2::Down, 30.0, 30.0));
        assert!(!stopped);
        assert_eq!(
            log.borrow().clone(),
            vec![
                "root:capture",
                "child:capture",
                "child:bubble",
                "root:bubble"
            ]
        );
    }

    #[test]
    fn stop_propagation_aborts_both_phases() {
        // 捕获阶段就中断 → 后面的捕获与整个冒泡都不发生。
        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let mut root_a = leaf(200.0, 200.0);
        root_a.x = Some(Rc::new(|| 0.0));
        root_a.y = Some(Rc::new(|| 0.0));
        let lc = Rc::clone(&log);
        root_a.mouse_wheel_capture = Some(Rc::new(move |e| {
            lc.borrow_mut().push("root:capture".into());
            e.stop_propagation();
        }));
        let mut child_a = leaf(80.0, 80.0);
        child_a.x = Some(Rc::new(|| 10.0));
        child_a.y = Some(Rc::new(|| 10.0));
        let lc2 = Rc::clone(&log);
        child_a.mouse_wheel_capture = Some(Rc::new(move |_| {
            lc2.borrow_mut().push("child:capture".into())
        }));
        let lb2 = Rc::clone(&log);
        child_a.mouse_wheel = Some(Rc::new(move |_| {
            lb2.borrow_mut().push("child:bubble".into())
        }));

        let root = Node2::empty().build(root_a);
        attach_child(&root, child_a);
        let path = hit_path(&root, 30.0, 30.0);

        let stopped = dispatch_mouse_path(&path, &mouse_dispatch_of(MouseType2::Wheel, 30.0, 30.0));
        assert!(stopped);
        assert_eq!(log.borrow().clone(), vec!["root:capture"]);
    }

    #[test]
    fn each_node_receives_its_own_local_coordinates() {
        let seen: Rc<RefCell<Vec<(f32, f32)>>> = Rc::new(RefCell::new(Vec::new()));
        let mut root_a = leaf(200.0, 200.0);
        root_a.x = Some(Rc::new(|| 0.0));
        root_a.y = Some(Rc::new(|| 0.0));
        let mut child_a = leaf(80.0, 80.0);
        child_a.x = Some(Rc::new(|| 10.0));
        child_a.y = Some(Rc::new(|| 10.0));
        let s = Rc::clone(&seen);
        child_a.mouse_move = Some(Rc::new(move |e| s.borrow_mut().push((e.x, e.y))));

        let root = Node2::empty().build(root_a);
        attach_child(&root, child_a);
        let path = hit_path(&root, 30.0, 30.0);
        let _ = dispatch_mouse_path(&path, &mouse_dispatch_of(MouseType2::Move, 30.0, 30.0));

        // 点 (30,30)：root 局部 (30,30)，child 局部 (20,20)
        assert_eq!(seen.borrow().clone(), vec![(20.0, 20.0)]);
    }

    #[test]
    fn cursor_of_path_prefers_leaf_then_editor_then_focusable() {
        // 1) 空链 → default
        assert_eq!(cursor_of_path(EMPTY), CursorType::Default);

        // 2) 可聚焦但没显式 cursor → pointer
        let mut a = leaf(50.0, 50.0);
        a.focusable = Rc::new(|| true);
        a.x = Some(Rc::new(|| 0.0));
        a.y = Some(Rc::new(|| 0.0));
        let root = Node2::empty().build(a);
        let path = hit_path(&root, 10.0, 10.0);
        assert_eq!(cursor_of_path(Some(&path)), CursorType::Pointer);

        // 3) 编辑器标记 → text（优先于 focusable）
        let mut b = leaf(50.0, 50.0);
        b.focusable = Rc::new(|| true);
        b.x = Some(Rc::new(|| 0.0));
        b.y = Some(Rc::new(|| 0.0));
        b.exts = vec![
            Rc::new(EditorHandleExt(Rc::new(0u8) as Rc<dyn std::any::Any>))
                as crate::canvas_render2::node2::Ext,
        ];
        let root2 = Node2::empty().build(b);
        let path2 = hit_path(&root2, 10.0, 10.0);
        assert_eq!(cursor_of_path(Some(&path2)), CursorType::Text);

        // 4) 显式 cursor 最优先
        let mut c = leaf(50.0, 50.0);
        c.cursor = Some(Rc::new(|| CursorType::Pointer));
        c.x = Some(Rc::new(|| 0.0));
        c.y = Some(Rc::new(|| 0.0));
        let root3 = Node2::empty().build(c);
        let path3 = hit_path(&root3, 10.0, 10.0);
        assert_eq!(cursor_of_path(Some(&path3)), CursorType::Pointer);
    }

    /// `cursor_of_path(None)` / `path_includes(None, ..)` 的 `None` 也需要具体类型。
    const EMPTY: Option<&[NodeWithPosition2]> = None;

    #[test]
    fn path_includes_and_primary_button() {
        let mut r = leaf(50.0, 50.0);
        r.x = Some(Rc::new(|| 0.0));
        r.y = Some(Rc::new(|| 0.0));
        let root = Node2::empty().build(r);
        let path = hit_path(&root, 10.0, 10.0);
        assert!(path_includes(Some(&path), &root));
        assert!(!path_includes(EMPTY, &root));
        assert!(is_primary_button(POINTER_BUTTON_LEFT));
        assert!(!is_primary_button(3));
    }
}
