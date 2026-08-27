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
