//! 帧动画信号：将"值随时间平滑变化"表达为可观察值。
//!
//! 对齐 Kotlin `skia-engine/animation/AnimateSignal`。

use std::cell::RefCell;
use std::rc::Rc;

// ===== 帧源抽象 =====

/// 帧订阅句柄。
pub trait FrameSubscription {
    fn cancel(&self);
}

/// 帧源抽象：由平台提供节拍。
pub trait FrameSource {
    fn subscribe(
        &self,
        callback: Box<dyn FnMut(f32) -> bool>,
        on_finish: Box<dyn FnOnce(bool)>,
    ) -> Box<dyn FrameSubscription>;
}

// ===== 位移协调器 =====

/// 位移协调器：动画期间以 init_value 为基准输出位移。
pub struct SilentDiff {
    value_set: Rc<RefCell<f32>>,
    init_value: f32,
    on_process: Option<Box<dyn FnMut(f32)>>,
    /// 动画目标值。
    pub target: Option<f32>,
}

impl SilentDiff {
    pub fn new(
        value: Rc<RefCell<f32>>,
        on_process: Option<Box<dyn FnMut(f32)>>,
        target: Option<f32>,
    ) -> Self {
        let init_value = *value.borrow();
        Self {
            value_set: value,
            init_value,
            on_process,
            target,
        }
    }

    /// 输出动画位移（相对基准）。
    pub fn set_displacement(&mut self, n: f32) {
        let nv = self.init_value + n;
        *self.value_set.borrow_mut() = nv;
        if let Some(ref mut cb) = self.on_process {
            cb(nv);
        }
    }

    /// 外部增量：基准与目标同步平移。
    pub fn silent_diff(&mut self, n: f32) {
        self.init_value += n;
        *self.value_set.borrow_mut() += n;
        if let Some(ref mut t) = self.target {
            *t += n;
        }
    }

    /// 改写目标但不重启动画。
    pub fn silent_change_to(&mut self, n: f32) {
        let t = self.target.expect("not a target function");
        self.silent_diff(n - t);
    }
}

// ===== 动画配置 =====

/// 动画配置：返回帧回调（displacement_time → displacement, should_stop）。
pub trait AnimateSignalConfig {
    fn create_frame(&self, delta_x: f32) -> Box<dyn FnMut(f32) -> (f32, bool)>;
}

// ===== 缓动函数 =====

pub type EaseFn = fn(f32) -> f32;

pub fn linear(t: f32) -> f32 {
    t
}

pub fn quad(t: f32) -> f32 {
    t * t
}

pub fn cubic(t: f32) -> f32 {
    t * t * t
}

// ===== 时长缓动动画 =====

/// tween 配置。
pub struct TweenConfig {
    duration_ms: f32,
    ease_fn: EaseFn,
}

impl AnimateSignalConfig for TweenConfig {
    fn create_frame(&self, delta_x: f32) -> Box<dyn FnMut(f32) -> (f32, bool)> {
        let dur = self.duration_ms;
        let ease = self.ease_fn;
        Box::new(move |diff_time_ms: f32| {
            let pc = diff_time_ms / dur;
            if pc < 1.0 {
                (delta_x * ease(pc), false)
            } else {
                (delta_x, true)
            }
        })
    }
}

/// 时长缓动动画工厂。
pub fn tween(duration_ms: f32, ease_fn: EaseFn) -> impl Fn(f32) -> TweenConfig {
    assert!(duration_ms > 0.0, "tween duration_ms 必须为正数");
    move |_delta_x: f32| TweenConfig {
        duration_ms,
        ease_fn,
    }
}

// ===== 弹簧物理 =====

#[derive(Clone, Copy, Debug)]
pub struct SpringBaseArg {
    pub omega0: f32,
    pub zeta: f32,
}

impl Default for SpringBaseArg {
    fn default() -> Self {
        Self {
            omega0: 20.0,
            zeta: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpringOutValue {
    pub displacement: f32,
    pub velocity: f32,
}

pub fn spring_base(
    elapsed_time_ms: f32,
    delta_x: f32,
    initial_velocity: f32,
    zeta: f32,
    omega0: f32,
    velocity_when_zeta1_plus: bool,
) -> SpringOutValue {
    let t = elapsed_time_ms / 1000.0;
    let v0 = -initial_velocity * 1000.0;

    if (zeta - 1.0).abs() < 0.001 {
        let coeff_a = delta_x;
        let coeff_b = v0 + omega0 * delta_x;
        let envelope = (-omega0 * t).exp();
        let displacement = (coeff_a + coeff_b * t) * envelope;
        let velocity = if velocity_when_zeta1_plus {
            envelope * ((coeff_a + coeff_b * t) * -omega0 + coeff_b)
        } else {
            0.0
        };
        SpringOutValue {
            displacement,
            velocity,
        }
    } else if zeta < 1.0 {
        let omega_d = omega0 * (1.0 - zeta * zeta).sqrt();
        let cos_coeff = delta_x;
        let sin_coeff = (v0 + zeta * omega0 * delta_x) / omega_d;
        let cos1 = (omega_d * t).cos();
        let sin1 = (omega_d * t).sin();
        let envelope = (-zeta * omega0 * t).exp();
        let displacement = envelope * (cos_coeff * cos1 + sin_coeff * sin1);
        let velocity = displacement * -omega0 * zeta
            + envelope * omega_d * (sin_coeff * cos1 - cos_coeff * sin1);
        SpringOutValue {
            displacement,
            velocity,
        }
    } else {
        let cext = omega0 * (zeta * zeta - 1.0).sqrt();
        let gamma_plus = -zeta * omega0 + cext;
        let gamma_minus = -zeta * omega0 - cext;
        let coeff_b = (gamma_minus * delta_x - v0) / (gamma_minus - gamma_plus);
        let coeff_a = delta_x - coeff_b;
        let em = (gamma_minus * t).exp();
        let ep = (gamma_plus * t).exp();
        let displacement = coeff_a * em + coeff_b * ep;
        let velocity = if velocity_when_zeta1_plus {
            coeff_a * gamma_minus * em + coeff_b * gamma_plus * ep
        } else {
            0.0
        };
        SpringOutValue {
            displacement,
            velocity,
        }
    }
}

pub fn spring_is_stop(n: &SpringOutValue, disp_threshold: f32, vel_threshold: f32) -> bool {
    n.displacement.abs() < disp_threshold && n.velocity.abs() < vel_threshold
}

#[derive(Clone, Copy, Debug)]
pub struct SpringAnimationArg {
    pub config: SpringBaseArg,
    pub initial_velocity: f32,
    pub displacement_threshold: f32,
    pub velocity_threshold: f32,
}

impl Default for SpringAnimationArg {
    fn default() -> Self {
        Self {
            config: SpringBaseArg::default(),
            initial_velocity: 0.0,
            displacement_threshold: 0.5,
            velocity_threshold: 10.0,
        }
    }
}

/// 弹簧动画配置。
pub struct SpringConfig {
    arg: SpringAnimationArg,
}

impl AnimateSignalConfig for SpringConfig {
    fn create_frame(&self, delta_x: f32) -> Box<dyn FnMut(f32) -> (f32, bool)> {
        let arg = self.arg;
        Box::new(move |diff_time_ms: f32| {
            let sv = spring_base(
                diff_time_ms,
                delta_x,
                arg.initial_velocity,
                arg.config.zeta,
                arg.config.omega0,
                true,
            );
            let stop = spring_is_stop(&sv, arg.displacement_threshold, arg.velocity_threshold);
            if stop {
                (delta_x, true)
            } else {
                (delta_x - sv.displacement, false)
            }
        })
    }
}

/// 弹簧动画工厂。
pub fn spring(arg: SpringAnimationArg) -> impl Fn(f32) -> SpringConfig {
    move |_delta_x: f32| SpringConfig { arg }
}

// ===== AnimateSignal =====

struct Running {
    out: Rc<RefCell<SilentDiff>>,
    subscription: Option<Box<dyn FrameSubscription>>,
}

/// 动画信号：将"值随时间平滑变化"表达为可观察值。
pub struct AnimateSignal {
    value: Rc<RefCell<f32>>,
    frames: Box<dyn FrameSource>,
    running: RefCell<Option<Running>>,
    lock: RefCell<bool>,
}

impl AnimateSignal {
    pub fn new(init_value: f32, frames: Box<dyn FrameSource>) -> Self {
        Self {
            value: Rc::new(RefCell::new(init_value)),
            frames,
            running: RefCell::new(None),
            lock: RefCell::new(false),
        }
    }

    /// 当前值。
    pub fn value(&self) -> f32 {
        *self.value.borrow()
    }

    /// 是否有动画进行中。
    pub fn is_animating(&self) -> bool {
        self.running.borrow().is_some()
    }

    fn finish(&self) {
        let o = self.running.borrow_mut().take();
        if let Some(mut r) = o {
            if let Some(sub) = r.subscription.take() {
                sub.cancel();
            }
        }
    }

    /// 目标值。
    pub fn get_target(&self) -> f32 {
        if let Some(ref r) = *self.running.borrow() {
            if let Some(t) = r.out.borrow().target {
                return t;
            }
        }
        self.value()
    }

    /// 直接写值并打断动画。
    pub fn set(&self, n: f32) -> f32 {
        assert!(!*self.lock.borrow(), "禁止在帧回调/配置构造期间修改");
        self.finish();
        *self.value.borrow_mut() = n;
        n
    }

    /// 冻结在当前值。
    pub fn stop(&self) {
        self.set(self.value());
    }

    /// 增量写值。
    pub fn change_diff(&self, n: f32) -> f32 {
        self.set(self.value() + n)
    }

    /// 外部增量。
    pub fn silent_diff(&self, n: f32) {
        if let Some(ref r) = *self.running.borrow() {
            r.out.borrow_mut().silent_diff(n);
        } else {
            *self.value.borrow_mut() += n;
        }
    }

    /// 改写目标不重启动画。
    pub fn silent_change_to(&self, n: f32) {
        if let Some(ref r) = *self.running.borrow() {
            r.out.borrow_mut().silent_change_to(n);
        } else {
            *self.value.borrow_mut() = n;
        }
    }

    /// 启动自定义动画。
    pub fn change(
        &self,
        config: &dyn AnimateSignalConfig,
        delta_x: f32,
        on_process: Option<Box<dyn FnMut(f32)>>,
        target: Option<f32>,
    ) -> Rc<RefCell<bool>> {
        assert!(!*self.lock.borrow(), "禁止在帧回调/配置构造期间修改");
        self.finish();

        let out = SilentDiff::new(self.value.clone(), on_process, target);
        let mut frame = config.create_frame(delta_x);

        let completed = Rc::new(RefCell::new(false));
        let completed_clone = completed.clone();
        let lock_ref = &self.lock as *const RefCell<bool>;
        let running_ref = &self.running as *const RefCell<Option<Running>>;

        // 使用 Rc 共享 out，闭包和 Running 各持一份
        let out_shared = Rc::new(RefCell::new(out));
        let out_for_frame = out_shared.clone();

        let subscription = self.frames.subscribe(
            Box::new(move |diff_time_ms: f32| {
                let _guard = LockGuard(unsafe { &*lock_ref });
                let (displacement, stop) = frame(diff_time_ms);
                out_for_frame.borrow_mut().set_displacement(displacement);
                if stop {
                    unsafe { &*running_ref }.borrow_mut().take();
                    *completed_clone.borrow_mut() = true;
                }
                stop
            }),
            Box::new(|_| {}),
        );

        let out_inner = out_shared; // 保持 Rc<RefCell<SilentDiff>>
        self.running.borrow_mut().replace(Running {
            out: out_inner,
            subscription: Some(subscription),
        });
        completed
    }

    /// 动画到目标值。
    pub fn animate_to(
        &self,
        n: f32,
        config_fn: impl Fn(f32) -> Box<dyn AnimateSignalConfig>,
        on_process: Option<Box<dyn FnMut(f32)>>,
    ) -> Rc<RefCell<bool>> {
        assert!(!*self.lock.borrow(), "禁止在帧回调/配置构造期间修改");
        self.finish();
        let diff = n - self.value();
        if diff != 0.0 {
            let cfg = config_fn(diff);
            return self.change(&*cfg, diff, on_process, Some(n));
        }
        Rc::new(RefCell::new(true))
    }

    /// 有配置则动画过去，否则直接写。
    pub fn change_to(
        &self,
        n: f32,
        config_fn: Option<impl Fn(f32) -> Box<dyn AnimateSignalConfig>>,
        on_process: Option<Box<dyn FnMut(f32)>>,
    ) -> Rc<RefCell<bool>> {
        if let Some(f) = config_fn {
            self.animate_to(n, f, on_process)
        } else {
            self.set(n);
            Rc::new(RefCell::new(true))
        }
    }
}

struct LockGuard<'a>(&'a RefCell<bool>);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        *self.0.borrow_mut() = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SimpleFrameSource {
        callback: RefCell<Option<Box<dyn FnMut(f32) -> bool>>>,
    }

    impl SimpleFrameSource {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                callback: RefCell::new(None),
            })
        }

        fn fire(&self, diff: f32) {
            if let Some(ref mut cb) = *self.callback.borrow_mut() {
                cb(diff);
            }
        }
    }

    impl FrameSource for SimpleFrameSource {
        fn subscribe(
            &self,
            callback: Box<dyn FnMut(f32) -> bool>,
            _on_finish: Box<dyn FnOnce(bool)>,
        ) -> Box<dyn FrameSubscription> {
            *self.callback.borrow_mut() = Some(callback);
            Box::new(SimpleSubHandle)
        }
    }

    struct SimpleSubHandle;
    impl FrameSubscription for SimpleSubHandle {
        fn cancel(&self) {}
    }

    fn make_animate(frames: &Rc<SimpleFrameSource>, init: f32) -> AnimateSignal {
        AnimateSignal::new(init, Box::new(SimpleFrameSourceWrapper(frames.clone())))
    }

    struct SimpleFrameSourceWrapper(Rc<SimpleFrameSource>);

    impl FrameSource for SimpleFrameSourceWrapper {
        fn subscribe(
            &self,
            callback: Box<dyn FnMut(f32) -> bool>,
            on_finish: Box<dyn FnOnce(bool)>,
        ) -> Box<dyn FrameSubscription> {
            self.0.subscribe(callback, on_finish)
        }
    }

    #[test]
    fn tween_converges_to_target() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 0.0);
        a.animate_to(100.0, |dx| Box::new(tween(200.0, linear)(dx)), None);

        frames.fire(100.0);
        assert!(a.value() > 0.0 && a.value() < 100.0);
        assert!(a.is_animating());

        frames.fire(200.0);
        assert_eq!(a.value(), 100.0);
        assert!(!a.is_animating());
    }

    #[test]
    fn tween_linear_halfway() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 0.0);
        a.animate_to(100.0, |dx| Box::new(tween(100.0, linear)(dx)), None);

        frames.fire(50.0);
        assert_eq!(a.value(), 50.0);
    }

    #[test]
    #[should_panic(expected = "tween duration_ms 必须为正数")]
    fn tween_rejects_zero_duration() {
        let _ = tween(0.0, linear);
    }

    #[test]
    fn spring_converges() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 0.0);
        let arg = SpringAnimationArg {
            config: SpringBaseArg {
                omega0: 20.0,
                zeta: 1.0,
            },
            ..Default::default()
        };
        a.animate_to(200.0, |dx| Box::new(spring(arg)(dx)), None);

        // 逐帧推进，传入累计时间
        let mut t = 0.0;
        while t < 2000.0 {
            t += 10.0;
            frames.fire(t);
        }
        assert!((a.value() - 200.0).abs() < 1.0, "实际 {}", a.value());
    }

    #[test]
    fn set_interrupts() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 0.0);
        a.animate_to(100.0, |dx| Box::new(tween(500.0, linear)(dx)), None);

        frames.fire(100.0);
        a.set(42.0);
        assert_eq!(a.value(), 42.0);
        assert!(!a.is_animating());
    }

    #[test]
    fn same_value_no_start() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 7.0);
        a.animate_to(7.0, |dx| Box::new(tween(100.0, linear)(dx)), None);
        assert!(!a.is_animating());
    }

    #[test]
    fn stop_freezes() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 0.0);
        a.animate_to(100.0, |dx| Box::new(tween(400.0, linear)(dx)), None);
        frames.fire(100.0);
        let frozen = a.value();
        a.stop();
        assert_eq!(a.value(), frozen);
    }

    #[test]
    fn get_target_idle() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 9.0);
        assert_eq!(a.get_target(), 9.0);
    }

    #[test]
    fn change_diff_idle() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 3.0);
        a.change_diff(4.0);
        assert_eq!(a.value(), 7.0);
    }

    #[test]
    fn silent_diff_idle() {
        let frames = SimpleFrameSource::new();
        let a = make_animate(&frames, 10.0);
        a.silent_diff(5.0);
        assert_eq!(a.value(), 15.0);
    }

    #[test]
    fn spring_base_critical() {
        let sv = spring_base(1000.0, 100.0, 0.0, 1.0, 20.0, false);
        assert!(sv.displacement.is_finite());
        assert!(sv.displacement.abs() < 100.0);
    }

    #[test]
    fn spring_is_stop_works() {
        let sv = SpringOutValue {
            displacement: 0.1,
            velocity: 5.0,
        };
        assert!(spring_is_stop(&sv, 0.5, 10.0));
        let sv2 = SpringOutValue {
            displacement: 2.0,
            velocity: 5.0,
        };
        assert!(!spring_is_stop(&sv2, 0.5, 10.0));
    }
}
