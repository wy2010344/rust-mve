//! 批量更新调度：`batch()` 闭包包裹多个写入，结束后统一 flush。
//!
//! 双缓冲实现（对应 Kotlin 的 `currentBatch` / `nextBatch`）：
//! - `batch`：当前正在处理的观察者队列
//! - `next_batch`：flush 期间新触发的观察者推入此处
//! - 当 `batch` 处理完毕后，交换 `batch` 和 `next_batch`
//! - 重复直到两者都为空

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

/// 双缓冲 flush：处理当前批次，新触发的推入 next_batch，交替处理直到都为空。
///
/// 安全上限防止副作用循环导致死循环。
pub fn flush() {
    const SAFETY_LIMIT: usize = 1000;
    let mut safety = 0;

    with_global(|g| g.flushing = true);

    loop {
        // 取出当前批次的所有观察者
        let current_batch: Vec<_> = with_global(|g| std::mem::take(&mut g.batch));

        if current_batch.is_empty() {
            // 检查 next_batch 是否有待处理的
            let has_next = with_global(|g| !g.next_batch.is_empty());
            if !has_next {
                break;
            }
            // 交换 batch 和 next_batch
            with_global(|g| {
                std::mem::swap(&mut g.batch, &mut g.next_batch);
            });
            continue;
        }

        // 执行当前批次中的每个观察者
        for id in current_batch {
            safety += 1;
            if safety > SAFETY_LIMIT {
                with_global(|g| {
                    g.batch.clear();
                    g.next_batch.clear();
                });
                break;
            }

            let node = with_global(|g| g.registry.get(&id).map(std::rc::Rc::clone));

            if let Some(node) = node {
                node.add_fun();
            }
        }
    }

    with_global(|g| g.flushing = false);
}
