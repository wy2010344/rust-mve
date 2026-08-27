/// Memo：派生节点，惰性求值 + relay map 快照比对。
///
/// 读取时检查依赖是否变化，未变则返回缓存值。
pub struct Memo<T> {
    value: Option<T>,
    relays: std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>>,
    version: u64,
}
