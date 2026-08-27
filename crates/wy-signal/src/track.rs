/// TrackSignal：副作用观察者，依赖动态收集。
///
/// 在闭包中读取 Signal/Memo 时自动注册依赖。
pub struct TrackSignal {
    id: SignalId,
}
