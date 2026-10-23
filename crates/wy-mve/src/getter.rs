//! 「显示值」属性：调用处传裸表达式或闭包，框架自动按需求值。
//!
//! `#[component]` 里用 `#[getter]` 标注的 prop，其 builder setter 收
//! `impl IntoGetter<T>`：
//!
//! - 传**裸值**（如 `"BUY"`、`model.x.get()`）→ 自动包成 `move || 值`，每次重绘重算；
//! - 传**闭包**（`|| ...`）→ 原样使用。
//!
//! 这对应 ooc 里「属性即表达式、每次 recomposition 重算」的写法：组件作者只须
//! 在 prop 上标 `#[getter]`，调用处就不必再手写 `move || ...` 样板。
//!
//! 只有一组具体值类型 + 一条闭包 blanket 实现，靠具体类型避开 blanket 重叠：
//! 值实现针对 `String` / `f32` / `Color` 等叶子类型，闭包实现针对 `F: Fn() -> T`。
//! 二者不会重叠（`String` 不是 `Fn`），故编译期无冲突。

use std::rc::Rc;

use wy_render::Color;

/// 「裸值或闭包 → 取值函数」的分发 trait。
///
/// 由 `#[component]` 生成的 getter setter 使用；不要手动实现——只通过
/// `String` / 数值 / `Color` / 闭包这几种既有实现接入。
pub trait IntoGetter<T> {
    /// 打包成一个 `Rc<dyn Fn() -> T>`（每次重绘被调用）。
    fn into_getter(self) -> Rc<dyn Fn() -> T>;
}

impl IntoGetter<String> for String {
    fn into_getter(self) -> Rc<dyn Fn() -> String> {
        Rc::new(move || self.clone())
    }
}

impl IntoGetter<String> for &'static str {
    fn into_getter(self) -> Rc<dyn Fn() -> String> {
        Rc::new(move || self.to_string())
    }
}

impl IntoGetter<f32> for f32 {
    fn into_getter(self) -> Rc<dyn Fn() -> f32> {
        Rc::new(move || self)
    }
}

impl IntoGetter<f64> for f64 {
    fn into_getter(self) -> Rc<dyn Fn() -> f64> {
        Rc::new(move || self)
    }
}

impl IntoGetter<i32> for i32 {
    fn into_getter(self) -> Rc<dyn Fn() -> i32> {
        Rc::new(move || self)
    }
}

impl IntoGetter<i64> for i64 {
    fn into_getter(self) -> Rc<dyn Fn() -> i64> {
        Rc::new(move || self)
    }
}

impl IntoGetter<i8> for i8 {
    fn into_getter(self) -> Rc<dyn Fn() -> i8> {
        Rc::new(move || self)
    }
}

impl IntoGetter<i16> for i16 {
    fn into_getter(self) -> Rc<dyn Fn() -> i16> {
        Rc::new(move || self)
    }
}

impl IntoGetter<u8> for u8 {
    fn into_getter(self) -> Rc<dyn Fn() -> u8> {
        Rc::new(move || self)
    }
}

impl IntoGetter<u16> for u16 {
    fn into_getter(self) -> Rc<dyn Fn() -> u16> {
        Rc::new(move || self)
    }
}

impl IntoGetter<u32> for u32 {
    fn into_getter(self) -> Rc<dyn Fn() -> u32> {
        Rc::new(move || self)
    }
}

impl IntoGetter<u64> for u64 {
    fn into_getter(self) -> Rc<dyn Fn() -> u64> {
        Rc::new(move || self)
    }
}

impl IntoGetter<usize> for usize {
    fn into_getter(self) -> Rc<dyn Fn() -> usize> {
        Rc::new(move || self)
    }
}

impl IntoGetter<bool> for bool {
    fn into_getter(self) -> Rc<dyn Fn() -> bool> {
        Rc::new(move || self)
    }
}

impl IntoGetter<Color> for Color {
    fn into_getter(self) -> Rc<dyn Fn() -> Color> {
        Rc::new(move || self)
    }
}

/// 闭包分支（唯一 blanket）：已有的 `move || ...` 写法继续可用。
impl<T: 'static, F: Fn() -> T + 'static> IntoGetter<T> for F {
    fn into_getter(self) -> Rc<dyn Fn() -> T> {
        Rc::new(self)
    }
}

/// 已存在的取值闭包（如 `Rc<dyn Fn() -> String>`）直接复用，不再包一层。
///
/// 与上面的 blanket 不冲突：`Rc<dyn Fn() -> T>` 本身**不**实现 `Fn() -> T`
/// （trait object 不参与 `Rc<T: Fn>` 的自动转发），故 blanket 不会覆盖它。
impl<T: 'static> IntoGetter<T> for Rc<dyn Fn() -> T> {
    fn into_getter(self) -> Rc<dyn Fn() -> T> {
        self
    }
}
