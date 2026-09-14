//! wy-signal 集成测试：覆盖依赖收集、批量更新、memo 缓存失效三大关键路径。

use std::cell::Cell;
use std::rc::Rc;

use wy_signal::{
    batch, create_effect, create_late_signal, create_memo, create_signal,
    create_signal_with_comparator, reset_signal_global_state, GetValue, SetValue,
};

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

// ===== 对齐 Kotlin SignalGuardTest — shouldChange 自定义比较器 =====

/// 自定义 shouldChange 接受"相等"值：即使值相同也触发通知。
/// 对齐 Kotlin SignalGuardTest.customShouldChangeAcceptsEqualValues。
#[test]
fn custom_should_change_accepts_equal_values() {
    // `|_, _| true` — 始终视为变化
    let s = create_signal_with_comparator(0, |_, _| true);
    let calls = Rc::new(Cell::new(0));
    create_effect({
        let s = s.clone();
        let calls = calls.clone();
        move || {
            let _ = s.get();
            calls.set(calls.get() + 1);
        }
    });
    assert_eq!(calls.get(), 1);

    // 设置相同值，但 shouldChange 始终返回 true → 触发通知
    s.set(0);
    assert_eq!(calls.get(), 2, "shouldChange 始终 true → 相同值也通知");
}

/// 自定义 shouldChange 拒绝不同值：即使值不同也不触发通知。
/// 对齐 Kotlin SignalGuardTest.customShouldChangeRejectsDifferentValues。
#[test]
fn custom_should_change_rejects_different_values() {
    // `|_, _| false` — 永不变化
    let s = create_signal_with_comparator(0, |_, _| false);
    let calls = Rc::new(Cell::new(0));
    create_effect({
        let s = s.clone();
        let calls = calls.clone();
        move || {
            let _ = s.get();
            calls.set(calls.get() + 1);
        }
    });
    assert_eq!(calls.get(), 1);

    s.set(999);
    assert_eq!(calls.get(), 1, "shouldChange 始终 false → 不同值也不通知");
    assert_eq!(s.get(), 0, "值未实际变化（set 被 shouldChange 拦截）");
}

// ===== 对齐 Kotlin MemoGuardTest — 循环 Memo 检测 =====

/// 循环 Memo 检测：直接自引用的 memo 应 panic。
/// 对齐 Kotlin MemoGuardTest.circularMemoDetectsDuplicate。
#[test]
#[should_panic(expected = "循环 memo 依赖")]
fn circular_memo_self_reference_panics() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wy_signal::Memo;

    // 构造直接自引用：memo_a 读 memo_a（通过 Rc<RefCell> 打破类型循环）
    let memo_a: Memo<i32> = {
        let self_ref: Rc<RefCell<Option<Memo<i32>>>> = Rc::new(RefCell::new(None));
        let a = create_memo({
            let self_ref = Rc::clone(&self_ref);
            move || {
                let m = self_ref.borrow();
                let m = m.as_ref().unwrap();
                m.get() + 1
            }
        });
        *self_ref.borrow_mut() = Some(a.clone());
        a
    };

    // 首次读取应 panic（循环检测）
    let _ = memo_a.get();
}

/// 循环 Memo 检测：A → B → A 循环应 panic。
#[test]
#[should_panic(expected = "循环 memo 依赖")]
fn circular_memo_chains_panic() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wy_signal::Memo;

    // 构造 A → B → A 循环
    let memo_a: Memo<i32> = {
        let memo_b_ref: Rc<RefCell<Option<Memo<i32>>>> = Rc::new(RefCell::new(None));
        let a = create_memo({
            let memo_b_ref = Rc::clone(&memo_b_ref);
            move || {
                let b = memo_b_ref.borrow();
                let b = b.as_ref().unwrap();
                b.get() + 1
            }
        });
        let b = create_memo({
            let a = a.clone();
            move || a.get() + 1
        });
        *memo_b_ref.borrow_mut() = Some(b);
        a
    };

    // 首次读取应 panic（循环检测）
    let _ = memo_a.get();
}

// ===== 对齐 Kotlin TrackSignalGuardTest — collect 嵌套守卫 =====

/// collect 在无当前观察者时正常工作。
/// 对齐 Kotlin TrackSignalNormalTest.collectWorksWhenCurrentFunIsNull。
#[test]
fn collect_works_when_no_current_observer() {
    let s = create_signal(10);
    let eff = create_effect({
        let s = s.clone();
        move || {
            let _ = s.get();
        }
    });
    // collect 在 effect 外部调用（currentFun 为 None）
    let val = eff.collect(|| s.get());
    assert_eq!(val, 10);
}

// ===== 对齐 Kotlin TrackSignalNormalTest — dispose 行为 =====

/// dispose 后 effect 不再响应信号变化。
/// 对齐 Kotlin TrackSignalNormalTest.disposedTrackSignalDoesNotExecuteAddFun。
#[test]
fn disposed_track_effect_stops_responding() {
    let s = create_signal(0);
    let calls = Rc::new(Cell::new(0));
    let eff = create_effect({
        let s = s.clone();
        let calls = calls.clone();
        move || {
            let _ = s.get();
            calls.set(calls.get() + 1);
        }
    });
    assert_eq!(calls.get(), 1);

    eff.dispose();
    s.set(1);
    assert_eq!(calls.get(), 1, "dispose 后不再响应");
}

// ===== 对齐 Kotlin TrackSignalNormalTest — 值变化回调 =====

/// Track 值变化时调用 on_change 回调。
/// 对齐 Kotlin TrackSignalNormalTest.trackSignalSetCalledWhenValueChanges。
#[test]
fn track_on_change_called_when_value_changes() {
    let s = create_signal(1);
    let last_seen = Rc::new(Cell::new(0));
    let t = wy_signal::track({
        let s = s.clone();
        move || s.get() * 10
    });
    // on_change 在 new() 之后设置，首次 compute 已执行
    assert_eq!(t.get_value(), 10, "首次 compute 已完成");

    t.on_change({
        let last_seen = last_seen.clone();
        move |v| last_seen.set(*v)
    });

    s.set(2);
    assert_eq!(t.get_value(), 20);
    assert_eq!(last_seen.get(), 20, "值变化触发 on_change");

    s.set(2); // 相同值
    assert_eq!(t.get_value(), 20);
    assert_eq!(last_seen.get(), 20, "相同值不触发 on_change");
}

// ===== 对齐 Kotlin AddEffectTest — effects 执行顺序 =====

/// 多个 effect 按注册顺序执行。
/// 对齐 Kotlin AddEffectTest.effectsSameLevelRunInRegistrationOrder。
#[test]
fn effects_run_in_registration_order() {
    let order = Rc::new(std::cell::RefCell::new(Vec::new()));
    let s = create_signal(0);

    create_effect({
        let s = s.clone();
        let order = order.clone();
        move || {
            let _ = s.get();
            order.borrow_mut().push(1);
        }
    });
    create_effect({
        let s = s.clone();
        let order = order.clone();
        move || {
            let _ = s.get();
            order.borrow_mut().push(2);
        }
    });
    create_effect({
        let s = s.clone();
        let order = order.clone();
        move || {
            let _ = s.get();
            order.borrow_mut().push(3);
        }
    });

    order.borrow_mut().clear();
    s.set(1);
    // batch flush 后按注册顺序执行
    let o = order.borrow().clone();
    assert_eq!(o, vec![1, 2, 3], "同层级 effect 按注册顺序执行");
}

// ===== 对齐 Kotlin BatchGuardTest — batch 安全限制 =====

/// batch safety limit：嵌套 batch 不应 panic。
/// 对齐 Kotlin BatchGuardTest.batchSignalEndStopsAtSafetyLimit（隐式覆盖）。
#[test]
fn nested_batch_does_not_panic() {
    let s = create_signal(0);
    let calls = Rc::new(Cell::new(0));
    create_effect({
        let s = s.clone();
        let calls = calls.clone();
        move || {
            let _ = s.get();
            calls.set(calls.get() + 1);
        }
    });

    // 嵌套 batch
    batch(|| {
        s.set(1);
        batch(|| {
            s.set(2);
        });
    });
    assert_eq!(calls.get(), 2, "嵌套 batch 正常 flush");
}

// ===== Memo afters 回调测试 =====

/// Memo afters 回调在值变化时触发。
#[test]
fn memo_afters_fire_on_value_change() {
    let base = create_signal(1);
    let after_values = Rc::new(std::cell::RefCell::new(Vec::new()));
    let memo = create_memo({
        let base = base.clone();
        move || base.get() * 10
    });
    memo.after({
        let after_values = after_values.clone();
        move |v| after_values.borrow_mut().push(*v)
    });

    // 首次读取触发计算 → afters 不应在 get 之前触发
    // （afters 在 validate 内部触发，首次计算 changed=true）
    let _ = memo.get();
    // 首次计算时 afters 触发（changed=true 因为 inited=false）
    // 但因为是首次，afters 已经在 validate 里触发了

    base.set(2);
    let _ = memo.get();
    let vals = after_values.borrow().clone();
    // 应该有两次调用：首次计算 + base=2 变化
    assert!(
        vals.len() >= 2,
        "afters 应在首次计算和值变化时各触发一次，实际: {:?}",
        vals
    );
    assert_eq!(vals.last(), Some(&20), "最后一次应为 2*10=20");
}

// ===== 对齐 Kotlin SignalGuardTest — lateSignal =====

/// lateSignal 只能获取一次写句柄。
/// 对齐 Kotlin SignalGuardTest.lateSignalOnlySetsOnce。
#[test]
fn late_signal_only_sets_once() {
    let late = create_late_signal(0);
    let writer = late.get_only_set();
    assert!(writer.is_some(), "首次获取应成功");

    // 第二次获取应返回 None
    assert!(late.get_only_set().is_none(), "第二次获取应返回 None");
}

/// lateSignal 写入后值更新。
/// 对齐 Kotlin SignalGuardTest.lateSignalGetOnlySetCustomMessage。
#[test]
fn late_signal_write_updates_value() {
    let late = create_late_signal(0);
    let calls = Rc::new(Cell::new(0));

    create_effect({
        let late = late.clone();
        let calls = calls.clone();
        move || {
            let _ = late.get();
            calls.set(calls.get() + 1);
        }
    });
    assert_eq!(calls.get(), 1);

    // 通过写句柄写入
    let writer = late.get_only_set().unwrap();
    writer.set(42);
    assert_eq!(late.get(), 42);
    assert_eq!(calls.get(), 2, "写入应触发 effect 重跑");
}

// ===== reset_signal_global_state 测试 =====

/// reset_signal_global_state 清除所有全局状态。
#[test]
fn reset_clears_global_state() {
    let s = create_signal(1);
    let calls = Rc::new(Cell::new(0));
    create_effect({
        let s = s.clone();
        let calls = calls.clone();
        move || {
            let _ = s.get();
            calls.set(calls.get() + 1);
        }
    });
    assert_eq!(calls.get(), 1);

    reset_signal_global_state();

    // 重置后，旧 effect 不再响应（已从注册表移除）
    s.set(999);
    assert_eq!(calls.get(), 1, "reset 后旧 effect 不再响应");

    // 新 effect 正常工作
    let s2 = create_signal(0);
    let calls2 = Rc::new(Cell::new(0));
    create_effect({
        let s2 = s2.clone();
        let calls2 = calls2.clone();
        move || {
            let _ = s2.get();
            calls2.set(calls2.get() + 1);
        }
    });
    assert_eq!(calls2.get(), 1);
    s2.set(1);
    assert_eq!(calls2.get(), 2, "reset 后新 effect 正常工作");
}
