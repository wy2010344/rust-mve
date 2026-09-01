//! StateHolder：管理节点树。

use crate::context::NodeContext;
use crate::node::Node;

/// StateHolder：持有构建后的节点树。
pub struct StateHolder {
    cx: NodeContext,
}

impl StateHolder {
    pub fn new(callback: impl FnOnce(&mut NodeContext)) -> Self {
        let mut cx = NodeContext::new(0);
        callback(&mut cx);
        Self { cx }
    }

    pub fn children(&self) -> &[Node] {
        self.cx.nodes()
    }

    pub fn children_mut(&mut self) -> &mut Vec<Node> {
        self.cx.nodes_mut()
    }

    pub fn rebuild(&mut self, callback: impl FnOnce(&mut NodeContext)) {
        self.cx = NodeContext::new(0);
        callback(&mut self.cx);
    }
}
