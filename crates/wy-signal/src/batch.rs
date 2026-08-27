//! 批量更新调度：`batch()` 闭包包裹多个写入，结束后统一 flush。
//!
//! 写入信号只把依赖它的观察者推入批次队列；真正执行（`add_fun`）延迟到
//! 批次退出或合适的时机（flush）进行，从而把多次写入合并为一次重算。

use crate::context::with_global;

/// 进入批次作用域执行 `f`，退出后统一 flush。
///
/// - 批次内多个信号写入会被合并，只触发一次依赖重算。
/// - 支持嵌套：最外层批次退出时才 flush。
pub fn batch<F: FnOnce()>(f: F) {
    with_global(|g| g.batch_depth += 1);
    f();
    with_global(|g| g.batch_depth -= 1);

    // 仅最外层批次负责 flush。
    let is_outer = with_global(|g| g.batch_depth == 0);
    if is_outer {
        flush();
    }
}

/// 立即执行批次队列中所有待评估观察者（幂等，空队列直接返回）。
pub fn flush() {
    // 安全上限，防止副作用循环导致死循环。
    const SAFETY_LIMIT: usize = 1000;
    let mut safety = 0;

    loop {
        let next = with_global(|g| {
            if g.batch.is_empty() {
                None
            } else {
                let id = g.batch.remove(0);
                // 取出注册表中的强引用，避免后续 add_fun 期间持有注册表借用。
                g.registry.get(&id).map(std::rc::Rc::clone)
            }
        });

        let Some(node) = next else { break };
        safety += 1;
        if safety > SAFETY_LIMIT {
            with_global(|g| g.batch.clear());
            break;
        }

        // 通过注册表调用观察者的 add_fun。
        node.add_fun();
    }
}
