//! 信号缓存：在 tree 构造阶段缓存信号值，draw 阶段读取。
//!
//! 解决的问题：信号读取必须在 tracker.collect() 阶段执行才能被追踪为依赖。
//! 组件的 draw_fn 在 draw_tree 阶段执行（collect 之后），此时读信号不会被追踪。
//!
//! 解决方案：
//! 1. 组件构造时（tree 构造阶段）读信号值，存入 signal_cache
//! 2. draw_fn 执行时从 signal_cache 读取缓存值，不再读信号

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static SIGNAL_CACHE: RefCell<HashMap<u64, Box<dyn std::any::Any>>> = RefCell::new(HashMap::new());
}

/// 生成唯一的缓存 key。
pub fn next_key() -> u64 {
    NEXT_KEY.fetch_add(1, Ordering::Relaxed)
}

/// 缓存一个信号值（key 通常是 Signal 的 id 或 pointer address）。
pub fn cache_signal_value<T: 'static>(key: u64, value: T) {
    SIGNAL_CACHE.with(|c| {
        c.borrow_mut().insert(key, Box::new(value));
    });
}

/// 读取缓存的信号值。
pub fn get_cached_signal<T: 'static>(key: u64) -> Option<T> {
    SIGNAL_CACHE.with(|c| {
        c.borrow_mut()
            .remove(&key)
            .and_then(|b| b.downcast::<T>().ok())
            .map(|b| *b)
    })
}

/// 清空缓存（每帧开始时调用）。
#[allow(dead_code)]
pub fn clear_signal_cache() {
    SIGNAL_CACHE.with(|c| {
        c.borrow_mut().clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_and_get() {
        cache_signal_value(1, 42i32);
        assert_eq!(get_cached_signal::<i32>(1), Some(42));
        assert_eq!(get_cached_signal::<i32>(1), None); // 已取出
    }

    #[test]
    fn overwrite() {
        cache_signal_value(1, 10i32);
        cache_signal_value(1, 20i32);
        assert_eq!(get_cached_signal::<i32>(1), Some(20));
    }

    #[test]
    fn clear() {
        cache_signal_value(1, 10i32);
        clear_signal_cache();
        assert_eq!(get_cached_signal::<i32>(1), None);
    }
}
