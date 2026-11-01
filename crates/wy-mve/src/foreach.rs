//! 动态区域：`render_for_each`（keyed 列表）与 `render_when`（条件分支）。
//!
//! 对标 TS `canvasRender2` 的 `ctx.renderForEach` / `ifNode`：**在父节点的
//! `children` 里管一段**——重算时把旧节点摘掉、按当前列表在**原位置**重建，
//! 与 TS「销毁旧节点、`ctx.addNode` 建新的」同序。
//!
//! # 为什么不再有 `ChildSlot`
//!
//! v1 把动态区域表达成 `ChildSlot::GetNodes(Rc<dyn Fn() -> Vec<ChildSlot>>)`，
//! 由引擎在绘制期把 `Root.target` 摊平成一棵新树。纯 `Node2` 树没有"引擎读 IR"
//! 这一层：`Node2` 的 `children` 就是它自己持有的 `Vec<Node2>`，所以区域拿到
//! 父节点后**直接增删那一段**即可。v1 的 holder / memo 分支随之删除。
//!
//! # 重算时机
//!
//! 区域本身是一个 [`Memo`]：读到的信号变化时重算。挂载期先同步跑一遍（TS 里
//! `renderForEach` 在构建期就跑），之后由 [`engine_global::register_region_probe`]
//! 在绘制前的 `probe_regions()` 阶段触发。

use std::cell::RefCell;
use std::rc::Rc;

use wy_signal::{GetValue, Memo};

use crate::canvas_render2::node2::{expand_node2, Node2};
use crate::canvas_render2::tree::engine_global;
use crate::context::{NodeContext, Provided};

/// 在 `parent` 的 `children` 里管一段 keyed 列表（TS `ctx.renderForEach`）。
///
/// - `for_each` 在区域 memo 求值时执行，读取源信号，并经 `emit(key, value)` 发出每一项；
/// - `creater` 为每一项构建子树，产物经 `ctx` 直接挂到 `parent` 下。
///
/// 重算一次 = 摘掉本区域上一轮建的节点 + 按当前列表在原位置按序重建。
/// 行节点建好后要 [`expand_node2`] 一次：动态创建的节点和静态树一样，
/// `children` 得靠展开才会被挂上。
pub fn render_for_each<K, T>(
    parent: &Node2,
    provided: &Provided,
    for_each: impl Fn(&mut dyn FnMut(K, T)) + 'static,
    creater: impl Fn(K, Rc<RefCell<T>>, &mut NodeContext) + 'static,
) where
    K: Eq + std::hash::Hash + Clone + 'static,
    T: Clone + PartialEq + 'static,
{
    // 区域在父节点里的起始下标：此刻父节点里只有声明在它**之前**的兄弟。
    let insert_at = parent.children().len();
    // 本区域当前拥有的节点（下一轮先摘掉它们）。
    let owned: Rc<RefCell<Vec<Node2>>> = Rc::new(RefCell::new(Vec::new()));

    let parent_in_memo = parent.clone();
    let provided_in_memo = provided.clone();
    let owned_in_memo = Rc::clone(&owned);

    let region = Memo::new(move || {
        for n in owned_in_memo.borrow_mut().drain(..) {
            n.detach();
        }
        let before = parent_in_memo.children().len();
        let mut cx =
            NodeContext::inherit_with_parent(&provided_in_memo, Some(parent_in_memo.clone()));

        for_each(&mut |key, value| {
            creater(key, Rc::new(RefCell::new(value)), &mut cx);
        });

        // 这一轮新建出来的节点 = 追加在 `before` 之后的那一段。
        let kids = parent_in_memo.children();
        let built: Vec<Node2> = kids[before.min(kids.len())..].to_vec();
        for n in &built {
            expand_node2(n);
        }
        // 搬回区域原本的位置（否则区域后面的兄弟会被挤到前面）。
        for (i, n) in built.iter().enumerate() {
            n.detach();
            parent_in_memo.insert_child_at(insert_at + i, n);
        }
        *owned_in_memo.borrow_mut() = built;
        // 载荷只用来"有变化"：真正的产物已经挂进树里了，闭包返回 unit。
    });

    // 首轮立刻建好（TS 里 `renderForEach` 在构建期就跑一遍）。
    region.get();
    let probe_target = region.clone();
    engine_global::register_region_probe(Rc::new(move || {
        probe_target.get();
    }));
}

/// 在 `parent` 的 `children` 里管一个条件分支（TS `ifNode`）。
///
/// `cond()` 为真渲染 `branch(true, ..)` 产出的节点，否则渲染 `branch(false, ..)`。
/// 条件翻转 = 旧分支摘掉、新分支重建；条件不变则不动。
///
/// 实现上复用 [`render_for_each`] 的单元素 keyed 区域：`bool` 当 key，翻转即
/// 换 key，与列表项增删走**完全相同的生命周期**。
pub fn render_when(
    parent: &Node2,
    provided: &Provided,
    cond: impl Fn() -> bool + 'static,
    branch: impl Fn(bool, &mut NodeContext) + 'static,
) {
    render_for_each(
        parent,
        provided,
        move |emit| emit(cond(), ()),
        move |key, _value, cx| branch(key, cx),
    );
}
