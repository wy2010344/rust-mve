//! 布局依赖审计（TS `tree/layoutAudit.ts` 的 1:1 复刻）。
//!
//! ## 为什么不是"静态 linter"
//!
//! 节点是 `children()` 里的**闭包**，运行时由信号驱动创建：
//! ```ignore
//! rect_node({ children: [todo_item(...)] })   // 编译期不存在这棵树
//! ```
//! 编译期没有树，AST 分析无从下手——只能在**已构建的树**上做检查。
//!
//! 与"撞一个修一个"的本质区别：
//!
//! | | 崩溃驱动 | 运行时审计 |
//! |---|---|---|
//! | 发现时机 | 渲染到那一条路径才炸 | 一次跑出**全部**违规点 |
//! | 覆盖 | 取决于这次渲染走到哪 | 整棵树（可遍历） |
//! | 成本 | 每次一个编译 + 渲染周期 | 一次遍历 |
//!
//! ## 怎么在不崩溃的前提下拿到依赖图
//!
//! 成环形态是 `父级布局 → 子的尺寸 → 子.size_from_parent → 父级布局`。
//! 若真让它跑，环闭合处立刻抛
//! [`LayoutCycleError`](crate::canvas_render2::tree::geometry::LayoutCycleError)，
//! 只能拿到**第一个**环。
//!
//! [`with_layout_dep`] 的探测模式把这一步改成「记依赖边、不再递归」：
//! [`size_from_parent`] 返回占位尺寸而不回头问父级，于是整棵树可以安全测量一遍，
//! 拿到完整依赖图；判环退化为**图上的边配对**——纯算法，不依赖运行时异常。
//!
//! ## 用法
//!
//! ```ignore
//! for text in audit_layout_report_all(&root) {
//!     eprintln!("{text}");
//! }
//! ```
//!
//! 审计只读：探测模式结束后自动恢复，不改变任何布局结果。

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;

use crate::canvas_render2::node2::{Axis, Node2};
use crate::canvas_render2::tree::geometry::{with_layout_dep, LayoutProbeEdge2};

/// 一条违规：某个节点的尺寸依赖父级，而父级反过来又读它的尺寸（TS `LayoutFinding2`）。
pub struct LayoutFinding2 {
    /// 成环涉及的节点（子节点侧）。
    pub node: Node2,
    /// 成环涉及的父级。
    pub parent: Node2,
    /// 节点索要尺寸的方向。
    pub direction: Axis,
    /// 环上的依赖边（按求值顺序：先"子索要父"，再"父读子"闭合）。
    pub cycle: Vec<LayoutProbeEdge2>,
    /// 报告要按序号定位节点，故持有一份遍历时登记的上下文。
    scope: Rc<AuditScope>,
}

impl LayoutFinding2 {
    /// 人类可读的报告（定位到业务名、方向、根因与修法）。
    #[must_use]
    pub fn report(&self) -> String {
        report_finding(self, &self.scope)
    }
}

/// 审计过程的上下文：节点编号（报告与边键都靠它，不散成模块级可变状态）。
struct AuditScope {
    /// 遍历顺序收集到的全部节点。
    nodes: Vec<Node2>,
    /// 节点 id → (遍历序号, 显示名)。父级也在遍历里，按父类型取即可。
    info: HashMap<u64, (usize, String)>,
}

impl AuditScope {
    fn index_of(&self, node: &Node2) -> i64 {
        self.info.get(&node.id()).map_or(-1, |(i, _)| *i as i64)
    }

    /// 节点显示名 + 序号。
    fn describe(&self, node: &Node2) -> String {
        match self.info.get(&node.id()) {
            Some((i, name)) => format!("{name}#{i}"),
            None => "节点#?".to_string(),
        }
    }

    /// 三元组标识（同一 (父, 子, 方向) 才可能是同一个环）。
    fn key(&self, edge: &LayoutProbeEdge2) -> String {
        let d = if edge.d == Axis::X { "x" } else { "y" };
        format!(
            "{}@{}>{}",
            d,
            self.index_of(&edge.parent),
            self.index_of(&edge.child)
        )
    }
}

/// 审计一棵已构建的树，返回全部成环点（不抛异常）。
///
/// `max_findings` 最多返回多少条（避免超大树的报告刷屏）。
#[must_use]
pub fn audit_layout(root: &Node2, max_findings: usize) -> Vec<LayoutFinding2> {
    // 1) 遍历树，收集节点与它们的序号（报告里要靠序号定位）
    let scope = Rc::new(collect_scope(root));

    // 2) 探测模式测量整棵树，收集 ASK / READ 两条边
    let prev = with_layout_dep(|d| d.probe_mode);
    with_layout_dep(|d| d.probe_mode = true);
    let edges = {
        let scope = Rc::clone(&scope);
        // `finally` 语义：无论中间发生什么都要恢复（这里只可能被 panic 打断）
        let run = || {
            for node in &scope.nodes {
                // 四个都碰，缺一不可：
                // - width / height：触发 **ASK** 边（子向父索要尺寸）；
                // - x / y：触发父级的 child_position → 父级布局求值 → 遍历子项取尺寸
                //   → **READ** 边。只测尺寸不够：`justify=between` 这类容器的布局
                //   只在算位置时才被求值。
                try_run(|| node.width());
                try_run(|| node.height());
                try_run(|| node.x());
                try_run(|| node.y());
            }
        };
        let guard = RestoreProbeMode { prev };
        let _ = panic::catch_unwind(AssertUnwindSafe(run));
        drop(guard);
        with_layout_dep(|d| d.readers.clear());
        crate::canvas_render2::tree::geometry::take_probe_edges()
    };

    // 3) 配对找环（见 find_cycles 的说明）
    find_cycles(&edges.0, &edges.1, max_findings, &scope)
}

/// [`audit_layout`] 的"不限条数"形式（TS `maxFindings` 的默认参数）。
#[must_use]
pub fn audit_layout_all(root: &Node2) -> Vec<LayoutFinding2> {
    audit_layout(root, usize::MAX)
}

/// 一次性拿到全部报告文本（TS `auditLayoutReport`）。
#[must_use]
pub fn audit_layout_report(root: &Node2, max_findings: usize) -> Vec<String> {
    audit_layout(root, max_findings)
        .iter()
        .map(LayoutFinding2::report)
        .collect()
}

/// [`audit_layout_report`] 的"不限条数"形式。
#[must_use]
pub fn audit_layout_report_all(root: &Node2) -> Vec<String> {
    audit_layout_report(root, usize::MAX)
}

/// 作用域守卫：离开作用域即把探测模式恢复成 `prev`（TS 的 `finally`）。
struct RestoreProbeMode {
    prev: bool,
}

impl Drop for RestoreProbeMode {
    fn drop(&mut self) {
        with_layout_dep(|d| d.probe_mode = self.prev);
    }
}

/// 遍历树登记序号（只取结构，不测尺寸）。
fn collect_scope(root: &Node2) -> AuditScope {
    let mut nodes: Vec<Node2> = Vec::new();
    // 遍历时顺手把名字存下来：报告里要用，而父级只有最小父类型（拿不到 debug_name）
    let mut info: HashMap<u64, (usize, String)> = HashMap::new();
    fn walk(node: &Node2, nodes: &mut Vec<Node2>, info: &mut HashMap<u64, (usize, String)>) {
        info.insert(
            node.id(),
            (
                nodes.len(),
                node.debug_name().unwrap_or_else(|| "节点".to_string()),
            ),
        );
        nodes.push(node.clone());
        for child in node.children() {
            walk(&child, nodes, info);
        }
    }
    walk(root, &mut nodes, &mut info);
    AuditScope { nodes, info }
}

/// 单次测量失败不应中断整棵树的审计（对齐 Kotlin 的 `runCatching`）。
///
/// 布局期的"未提供尺寸 / 无父级 / 未参与布局"在 Rust 侧是 panic
/// （`LayoutCycleError` 对应的那类），故用 `catch_unwind` 接住；同时临时把
/// panic hook 换成空实现，避免审计把整棵树的正常违规全刷到 stderr。
fn try_run(run: impl FnOnce() -> f32) {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let _ = panic::catch_unwind(AssertUnwindSafe(run));
    panic::set_hook(prev);
}

/// 在依赖图上找环 —— **纯观测判据，不复述引擎规则**。
///
/// 两个集合：
/// - **ASK**：子向父索要尺寸（`size_from_parent` 处记录）
/// - **READ**：父级测量时读取子项尺寸（`size` 处记录）
///
/// 同一个 `(父级, 子节点, 方向)` 三元组同时出现在两个集合中 ⟹ 互依 ⟹ 成环。
///
/// ## 为什么这样判
///
/// 早期版本在 auditor 里**复述**引擎规则（"父级是 Flex 就必然回读子项"），
/// 结果 363 处误报：漏了 grow 子项那一支、也漏了交叉轴的 align_fix / align_item
/// 语义。规则一复述就会与引擎漂移。
///
/// 改成观测后，判据与引擎行为**天然一致**——分支怎么走是引擎自己决定的，
/// auditor 只负责把两个事实配对。引擎将来改了分支语义，审计自动跟上。
fn find_cycles(
    asks: &[LayoutProbeEdge2],
    reads: &[LayoutProbeEdge2],
    max_findings: usize,
    scope: &Rc<AuditScope>,
) -> Vec<LayoutFinding2> {
    if asks.is_empty() || reads.is_empty() {
        return Vec::new();
    }

    // READ 边按 (父级, 子节点, 方向) 建索引
    let read_keys: std::collections::HashSet<String> = reads.iter().map(|r| scope.key(r)).collect();

    let mut findings: Vec<LayoutFinding2> = Vec::new();
    let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
    for ask in asks {
        // 同一三元组既被"索要"又被"读取" → 互依
        let key = scope.key(ask);
        if !read_keys.contains(&key) {
            continue;
        }
        // 同一三元组只报一次（探测会把每条边记到多次）
        if !reported.insert(key) {
            continue;
        }

        findings.push(LayoutFinding2 {
            node: ask.child.clone(),
            parent: ask.parent.clone(),
            direction: ask.d,
            // 环是两条边闭合的：子 → 父（索要），父 → 子（被测量）
            cycle: vec![
                ask.clone(),
                LayoutProbeEdge2 {
                    parent: ask.parent.clone(),
                    child: ask.child.clone(),
                    d: ask.d,
                },
            ],
            scope: Rc::clone(scope),
        });
        if findings.len() >= max_findings {
            break;
        }
    }
    findings
}

fn report_finding(finding: &LayoutFinding2, scope: &AuditScope) -> String {
    let name = scope.describe(&finding.node);
    let parent_name = scope.describe(&finding.parent);
    let dir = if finding.direction == Axis::X {
        "宽度"
    } else {
        "高度"
    };
    [
        format!(
            "布局依赖成环：{name} 的{dir}依赖父级 {parent_name}，而父级测量时又回读了它。"
        ),
        format!(
            "  依赖链：{name} → {parent_name}（索要尺寸）→ {name}（被父级测量）  ← 环在此闭合"
        ),
        "  含义：该节点未提供确定尺寸，沿默认尺寸回落到\"向父级索要\"，而其父级正在测量时又读回了它。"
            .to_string(),
        "  修法（二选一）：".to_string(),
        "    1) 让该节点有确定尺寸（不依赖父级）：返回 LayoutSize::outer(测得值)（from_inside: false）；"
            .to_string(),
        "    2) 让它成为 grow 子节点（grow(1.0)），由父级分配尺寸，父级便不会回读它。".to_string(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_render2::node2::{always, Flex, LayoutDir2, Node2Props};

    /// 只给方向的 flex（测试里最常用的形态）。
    fn flex_dir(direction: Layout) -> LayoutDir2 {
        LayoutDir2::new(Flex {
            direction: always(direction),
            ..Flex::default()
        })
    }
    use crate::canvas_render2::tree::geometry::{default_position, default_size};
    use crate::{Fc, Layout, NodeContext};
    use std::cell::RefCell;

    /// 一个"尺寸走默认链"的节点参数：把 `size` 消息接到 [`default_size`] 上。
    ///
    /// 翻转 `width()` / `height()` 的默认值之前，只有显式接线的节点才会沿
    /// `default_size → size_from_parent` 走；审计测的就是这条链。
    fn default_sized() -> (Rc<RefCell<Option<Node2>>>, Node2Props) {
        let slot: Rc<RefCell<Option<Node2>>> = Rc::new(RefCell::new(None));
        let s = Rc::clone(&slot);
        let p = Rc::clone(&slot);
        let arg = Node2Props {
            size: Some(Rc::new(move |d| default_size(&n_of(&s), d))),
            position: Some(Rc::new(move |d| default_position(&n_of(&p), d))),
            ..Node2Props::default()
        };
        (slot, arg)
    }

    fn n_of(slot: &RefCell<Option<Node2>>) -> Node2 {
        slot.borrow().clone().expect("节点尚未回填")
    }

    fn attach_child(parent: &Node2, arg: Node2Props) -> Node2 {
        let mut cx = NodeContext::new();
        cx.node2_parent = Some(parent.clone());
        let out = Rc::new(RefCell::new(None));
        let o = Rc::clone(&out);
        Fc::new(move |cx| {
            let n = Node2::empty().build(arg);
            *o.borrow_mut() = Some(n.clone());
            n.mount(cx);
        })
        .mount(&mut cx);
        let built = out.borrow().clone();
        built.expect("子节点未构建")
    }

    /// 父（flex 行容器）× 子（无确定尺寸）→ 子的宽回落到"向父索要"，
    /// 而父的布局又要读子的宽 → 成环。
    #[test]
    fn finds_cycle_between_flex_parent_and_unsized_child() {
        let (root_slot, root_arg) = default_sized();
        let mut root_arg = root_arg;
        root_arg.layout = flex_dir(Layout::Row);
        root_arg.debug_name = Some(Rc::new(|| "容器".to_string()));
        let root = Node2::empty().build(root_arg);
        *root_slot.borrow_mut() = Some(root.clone());

        let (c_slot, mut c_arg) = default_sized();
        // 高度给确定值：只让**宽度**走"向父索要"，隔离出单条违规
        c_arg.height = Some(Rc::new(|| 20.0));
        c_arg.debug_name = Some(Rc::new(|| "子项".to_string()));
        let child = attach_child(&root, c_arg);
        *c_slot.borrow_mut() = Some(child.clone());

        let findings = audit_layout_all(&root);
        assert_eq!(findings.len(), 1, "应恰好报一处：{:?}", findings.len());
        let f = &findings[0];
        assert_eq!(f.direction, Axis::X);
        assert_eq!(f.node.id(), child.id());
        assert_eq!(f.parent.id(), root.id());
        assert_eq!(f.cycle.len(), 2);

        let text = f.report();
        assert!(text.contains("子项#1"), "{text}");
        assert!(text.contains("容器#0"), "{text}");
        assert!(text.contains("宽度"), "{text}");
        assert!(text.contains("布局依赖成环"), "{text}");
    }

    /// 叶子给了确定尺寸 → 不向父索要 → 无 ASK 边 → 无违规。
    #[test]
    fn no_finding_when_child_has_own_size() {
        let (root_slot, mut root_arg) = default_sized();
        root_arg.layout = flex_dir(Layout::Row);
        let root = Node2::empty().build(root_arg);
        *root_slot.borrow_mut() = Some(root.clone());

        let child = attach_child(
            &root,
            Node2Props {
                width: Some(Rc::new(|| 10.0)),
                height: Some(Rc::new(|| 20.0)),
                ..Node2Props::default()
            },
        );
        assert_eq!(child.width(), 10.0);

        assert!(audit_layout_all(&root).is_empty());
    }

    /// `max_findings` 生效：只报一条。
    #[test]
    fn max_findings_caps_report() {
        let (root_slot, mut root_arg) = default_sized();
        root_arg.layout = flex_dir(Layout::Row);
        let root = Node2::empty().build(root_arg);
        *root_slot.borrow_mut() = Some(root.clone());

        for _ in 0..2 {
            let (c_slot, mut c_arg) = default_sized();
            c_arg.height = Some(Rc::new(|| 20.0));
            let child = attach_child(&root, c_arg);
            *c_slot.borrow_mut() = Some(child);
        }

        assert_eq!(audit_layout_all(&root).len(), 2);
        assert_eq!(audit_layout(&root, 1).len(), 1);
        assert_eq!(audit_layout_report(&root, 1).len(), 1);
    }

    /// 树里没有任何"向父索要"的节点（根也没给尺寸、直接测量会失败）→ 空报告，
    /// 且不会把 panic 抛出去。
    #[test]
    fn audit_is_readonly_and_never_panics() {
        let root = Node2::empty().build(Node2Props {
            width: Some(Rc::new(|| 100.0)),
            height: Some(Rc::new(|| 100.0)),
            ..Node2Props::default()
        });
        // 根节点没有父级：`x()` 走 default_position 会失败，try_run 必须接住
        let report = audit_layout_report_all(&root);
        assert!(report.is_empty());
        // 探测模式已恢复
        assert!(!with_layout_dep(|d| d.probe_mode));
        // 尺寸不受影响
        assert_eq!(root.width(), 100.0);
    }
}
