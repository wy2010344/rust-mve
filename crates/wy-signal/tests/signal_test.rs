//! wy-signal 集成测试：覆盖依赖收集、批量更新、memo 缓存失效三大关键路径。

use std::cell::Cell;
use std::rc::Rc;

use wy_signal::{batch, create_effect, create_memo, create_signal, GetValue, SetValue};

#[test]
fn signal_get_set_value() {
    let s = create_signal(0);
    assert_eq!(s.get(), 0);
    s.set(42);
    assert_eq!(s.get(), 42);
}

#[test]
fn set_same_value_does_not_notify() {
    let s = create_signal(5);
    let calls = Rc::new(Cell::new(0));
    create_effect({
        let s = s.clone();
        let calls = calls.clone();
        move || {
            let _ = s.get();
            calls.set(calls.get() + 1);
        }
    });
    assert_eq!(calls.get(), 1, "首次创建立即执行一次收集依赖");

    s.set(5); // 相同值不触发
    assert_eq!(calls.get(), 1);

    s.set(6);
    assert_eq!(calls.get(), 2);
}

#[test]
fn effect_dependency_collection() {
    let a = create_signal(1);
    let b = create_signal(2);
    let sum = Rc::new(Cell::new(0));

    create_effect({
        let a = a.clone();
        let b = b.clone();
        let sum = sum.clone();
        move || {
            sum.set(a.get() + b.get());
        }
    });

    assert_eq!(sum.get(), 3);

    a.set(10);
    assert_eq!(sum.get(), 12);

    b.set(20);
    assert_eq!(sum.get(), 30);
}

#[test]
fn batch_merges_multiple_writes() {
    let a = create_signal(0);
    let b = create_signal(0);
    let runs = Rc::new(Cell::new(0));

    create_effect({
        let a = a.clone();
        let b = b.clone();
        let runs = runs.clone();
        move || {
            let _ = a.get();
            let _ = b.get();
            runs.set(runs.get() + 1);
        }
    });

    assert_eq!(runs.get(), 1, "首次运行");

    // 批量内两次写入，应合并为一次重算。
    batch(|| {
        a.set(1);
        b.set(2);
    });
    assert_eq!(runs.get(), 2);

    assert_eq!(a.get(), 1);
    assert_eq!(b.get(), 2);
}

#[test]
fn memo_cache_and_invalidation() {
    let base = create_signal(10);
    let factor = create_signal(2);
    let compute_calls = Rc::new(Cell::new(0));

    let doubled = create_memo({
        let base = base.clone();
        let factor = factor.clone();
        let compute_calls = compute_calls.clone();
        move || {
            compute_calls.set(compute_calls.get() + 1);
            base.get() * factor.get()
        }
    });

    // 首次读取触发计算。
    assert_eq!(doubled.get(), 20);
    assert_eq!(compute_calls.get(), 1);

    // 再次读取，无依赖变化 → 命中缓存，不重算。
    assert_eq!(doubled.get(), 20);
    assert_eq!(compute_calls.get(), 1);

    // 写入 base，memo 失效并重算。
    base.set(11);
    assert_eq!(doubled.get(), 22);
    assert_eq!(compute_calls.get(), 2);

    // 写入 factor，再次重算。
    factor.set(3);
    assert_eq!(doubled.get(), 33);
    assert_eq!(compute_calls.get(), 3);
}

#[test]
fn memo_no_recompute_when_unrelated_signal_changes() {
    let base = create_signal(1);
    let other = create_signal(100);
    let compute_calls = Rc::new(Cell::new(0));

    let tracked = create_memo({
        let base = base.clone();
        let compute_calls = compute_calls.clone();
        move || {
            compute_calls.set(compute_calls.get() + 1);
            base.get()
        }
    });

    let _ = tracked.get();
    assert_eq!(compute_calls.get(), 1);

    // 写入无关信号 other：memo 不应重算（relay 快照比对判定无变化）。
    other.set(200);
    let v = tracked.get();
    assert_eq!(v, 1);
    assert_eq!(compute_calls.get(), 1);
}

#[test]
fn memo_invalidates_dependent_effect() {
    let base = create_signal(5);
    let doubled = create_memo({
        let base = base.clone();
        move || base.get() * 2
    });

    let seen = Rc::new(Cell::new(0));
    create_effect({
        let doubled = doubled.clone();
        let seen = seen.clone();
        move || {
            let _ = doubled.get();
            seen.set(seen.get() + 1);
        }
    });

    assert_eq!(seen.get(), 1, "effect 首次执行");

    base.set(10);
    // base → memo（监听者）→ effect（监听者），一环触发。
    assert_eq!(seen.get(), 2);
    // memo 应已被重算为最新值。
    assert_eq!(doubled.get(), 20);
}

#[test]
fn nested_memo() {
    let a = create_signal(2);
    let b = create_signal(3);
    let inner = create_memo({
        let a = a.clone();
        let b = b.clone();
        move || a.get() + b.get()
    });
    let outer = create_memo({
        let inner = inner.clone();
        move || inner.get() * 10
    });

    assert_eq!(outer.get(), 50);

    a.set(5);
    assert_eq!(outer.get(), 80);
}

// ===== 对齐 Kotlin SignalFlowTest 的端到端链路测试 =====

/// memo 幂等性：同 stateVersion 内多次读取，过程体只执行一次。
/// 对齐 Kotlin SignalFlowTest.memoProcessRunsOnceWithinSameStateVersion。
#[test]
fn memo_idempotent_within_same_state_version() {
    let base = create_signal(2);
    let recompute = Rc::new(Cell::new(0));
    let doubled = create_memo({
        let base = base.clone();
        let recompute = recompute.clone();
        move || {
            recompute.set(recompute.get() + 1);
            base.get() * 2
        }
    });

    assert_eq!(doubled.get(), 4);
    assert_eq!(doubled.get(), 4);
    assert_eq!(doubled.get(), 4);
    assert_eq!(
        recompute.get(),
        1,
        "同 stateVersion 内多次读取过程体只执行一次"
    );

    base.set(3);
    assert_eq!(doubled.get(), 6);
    assert_eq!(recompute.get(), 2, "上游变化应导致过程体重算");
}

/// memo 链式派生：signal → memo → memo → effect。
/// 对齐 Kotlin SignalFlowTest.memoChainsThroughAnotherMemo。
#[test]
fn memo_chains_through_another_memo() {
    let a = create_signal(1);
    let b = create_memo({
        let a = a.clone();
        move || a.get() + 10
    });
    let c = create_memo({
        let b = b.clone();
        move || b.get() * 2
    });

    let received = Rc::new(Cell::new(0));
    create_effect({
        let c = c.clone();
        let received = received.clone();
        move || {
            received.set(c.get());
        }
    });

    assert_eq!(received.get(), 22, "a=1 -> b=11 -> c=22");

    a.set(5);
    assert_eq!(received.get(), 30, "a=5 -> b=15 -> c=30");
}

/// 新观察者加入时重放 relays 不得重算 memo 过程体。
/// 对齐 Kotlin SignalFlowTest.memoProcessNotReRunWhenNewObserverReplaysRelay。
#[test]
fn memo_new_observer_does_not_rerun_process() {
    let base = create_signal(5);
    let recompute = Rc::new(Cell::new(0));
    let squared = create_memo({
        let base = base.clone();
        let recompute = recompute.clone();
        move || {
            recompute.set(recompute.get() + 1);
            base.get() * base.get()
        }
    });

    let a_val = Rc::new(Cell::new(0));
    create_effect({
        let squared = squared.clone();
        let a_val = a_val.clone();
        move || {
            a_val.set(squared.get());
        }
    });
    assert_eq!(a_val.get(), 25);
    assert_eq!(recompute.get(), 1, "首次计算一次");

    let b_val = Rc::new(Cell::new(0));
    create_effect({
        let squared = squared.clone();
        let b_val = b_val.clone();
        move || {
            b_val.set(squared.get());
        }
    });
    assert_eq!(b_val.get(), 25, "新观察者应拿到缓存值");
    assert_eq!(
        recompute.get(),
        1,
        "新增观察者重放 relays 不得重算 memo 过程体"
    );

    base.set(6);
    assert_eq!(a_val.get(), 36, "观察者 A 应感知变化");
    assert_eq!(b_val.get(), 36, "观察者 B 应感知变化");
    assert_eq!(recompute.get(), 2, "一次上游变化只重算一次");
}
