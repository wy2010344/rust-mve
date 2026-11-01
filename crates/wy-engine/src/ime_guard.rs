//! 输入法提交守护窗口：丢掉「选词那一枚按键的另一半」。
//!
//! Windows 上「空格选词」是两条独立消息：先 `Ime::Commit("中")`，随后那次按键的
//! `WM_CHAR` 才作为 `Character(" ")` 到达——此时组合态已经结束，翻译层也分辨不出
//! （`KeyEvent::text` 确实存在，它就是个空格），只能靠「紧跟在 Commit 之后」这条
//! 时间关系把它认出来。
//!
//! 窗口必须足够短：用户提交后马上敲空格是常见动作，30ms 内人手打不出第二枚键。

use std::cell::Cell;
use std::time::{Duration, Instant};

/// 守护窗口时长（从 `Ime::Commit` 起算）。
pub const IME_COMMIT_GUARD: Duration = Duration::from_millis(30);

/// 提交守护窗口。
///
/// 用法：`ImeEvent::Commit` 时 [`arm`](Self::arm)，字符键到达时先问
/// [`swallow`](Self::swallow)——返回 `true` 表示这枚键是残留、应整条丢弃。
#[derive(Debug, Default)]
pub struct ImeCommitGuard {
    until: Cell<Option<Instant>>,
}

impl ImeCommitGuard {
    pub const fn new() -> Self {
        Self {
            until: Cell::new(None),
        }
    }

    /// 输入法刚提交：开一个窗口。
    pub fn arm(&self) {
        self.arm_at(Instant::now());
    }

    /// 该字符键是不是窗口内的「按键残留」。
    ///
    /// 无论命中与否都关闭窗口：残留只会有一枚，多吞会误伤用户真正敲下的字符。
    pub fn swallow(&self) -> bool {
        self.swallow_at(Instant::now())
    }

    /// 窗口是否还开着（提交后 30ms 内）。
    pub fn is_armed(&self) -> bool {
        self.until.get().is_some_and(|until| Instant::now() < until)
    }

    fn arm_at(&self, now: Instant) {
        self.until.set(Some(now + IME_COMMIT_GUARD));
    }

    fn swallow_at(&self, now: Instant) -> bool {
        match self.until.take() {
            Some(until) => now < until,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_then_swallows_exactly_one_key() {
        let g = ImeCommitGuard::new();
        assert!(!g.swallow(), "没提交过就按键，不该被吞");

        g.arm();
        assert!(g.swallow(), "提交后紧随的字符键应被吞掉");
        assert!(!g.swallow(), "只吞一枚：第二次按键是用户真敲的");
    }

    #[test]
    fn expired_window_lets_keys_through() {
        let g = ImeCommitGuard::new();
        let t0 = Instant::now();
        g.arm_at(t0);
        assert!(!g.swallow_at(t0 + IME_COMMIT_GUARD), "窗口边界之后应放行");
    }

    #[test]
    fn inside_window_is_swallowed() {
        let g = ImeCommitGuard::new();
        let t0 = Instant::now();
        g.arm_at(t0);
        assert!(
            g.swallow_at(t0 + IME_COMMIT_GUARD / 2),
            "窗口内应吞掉残留按键"
        );
    }
}
