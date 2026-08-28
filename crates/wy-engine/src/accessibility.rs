//! 无障碍桥接：将 Widget 树转换为 AccessKit tree。
//!
//! 对应 Kotlin `AccessibilityBridge`。AccessKit 是跨平台无障碍 API，
//! 本模块负责维护 widget 节点到 AccessKit 节点的映射，并生成 `TreeUpdate`。

/// 无障碍节点的角色。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AccessRole {
    /// 无特定角色。
    Unknown,
    /// 按钮。
    Button,
    /// 标签/文本。
    Label,
    /// 输入框。
    TextInput,
    /// 容器/分组。
    Group,
    /// 滚动区域。
    ScrollArea,
    /// 复选框。
    Checkbox,
    /// 单选按钮。
    RadioButton,
    /// 下拉菜单。
    ComboBox,
    /// 链接。
    Link,
    /// 图片。
    Image,
    /// 标题。
    Heading,
    /// 段落。
    Paragraph,
    /// 列表。
    List,
    /// 列表项。
    ListItem,
}

/// 无障碍节点：描述一个 widget 的无障碍属性。
#[derive(Clone, Debug)]
pub struct AccessNode {
    /// 节点 ID（与 widget ID 对应）。
    pub id: usize,
    /// 角色。
    pub role: AccessRole,
    /// 名称（如按钮标签、输入框提示）。
    pub name: Option<String>,
    /// 描述（如 tooltip）。
    pub description: Option<String>,
    /// 是否可聚焦。
    pub focusable: bool,
    /// 是否已选中（复选框/单选按钮）。
    pub selected: Option<bool>,
    /// 子节点 ID。
    pub children: Vec<usize>,
}

impl AccessNode {
    /// 创建无障碍节点。
    pub fn new(id: usize, role: AccessRole) -> Self {
        Self {
            id,
            role,
            name: None,
            description: None,
            focusable: false,
            selected: None,
            children: Vec::new(),
        }
    }

    /// 设置名称。
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// 设置描述。
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// 设置可聚焦。
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// 设置选中状态。
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// 设置子节点 ID。
    pub fn with_children(mut self, children: Vec<usize>) -> Self {
        self.children = children;
        self
    }
}

/// 无障碍桥接：管理 widget 到 AccessKit 节点的映射。
pub struct AccessibilityBridge {
    /// 根节点 ID。
    root_id: Option<usize>,
    /// 所有无障碍节点（按 ID 索引）。
    nodes: Vec<AccessNode>,
}

impl AccessibilityBridge {
    /// 创建空的无障碍桥接。
    pub fn new() -> Self {
        Self {
            root_id: None,
            nodes: Vec::new(),
        }
    }

    /// 设置根节点。
    pub fn set_root(&mut self, root_id: usize) {
        self.root_id = Some(root_id);
    }

    /// 添加或更新一个无障碍节点。
    pub fn update_node(&mut self, node: AccessNode) {
        if let Some(existing) = self.nodes.iter_mut().find(|n| n.id == node.id) {
            *existing = node;
        } else {
            self.nodes.push(node);
        }
    }

    /// 移除一个节点。
    pub fn remove_node(&mut self, id: usize) {
        self.nodes.retain(|n| n.id != id);
        // 同时从父节点的 children 中移除
        for node in &mut self.nodes {
            node.children.retain(|&child_id| child_id != id);
        }
    }

    /// 获取节点引用。
    pub fn get_node(&self, id: usize) -> Option<&AccessNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 获取节点可变引用。
    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut AccessNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// 获取根节点引用。
    pub fn root_node(&self) -> Option<&AccessNode> {
        self.root_id.and_then(|id| self.get_node(id))
    }

    /// 节点总数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 生成无障碍树的简单表示（用于调试/测试）。
    pub fn dump_tree(&self) -> String {
        let mut out = String::new();
        if let Some(root_id) = self.root_id {
            self.dump_node(root_id, &mut out, 0);
        }
        out
    }

    fn dump_node(&self, id: usize, out: &mut String, depth: usize) {
        if let Some(node) = self.get_node(id) {
            let indent = "  ".repeat(depth);
            let name = node.name.as_deref().unwrap_or("");
            let role = format!("{:?}", node.role);
            out.push_str(&format!("{indent}{role}({id}) \"{name}\"\n"));
            for &child_id in &node.children {
                self.dump_node(child_id, out, depth + 1);
            }
        }
    }
}

impl Default for AccessibilityBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_add_and_get_node() {
        let mut bridge = AccessibilityBridge::new();
        bridge.update_node(AccessNode::new(1, AccessRole::Button).with_name("OK"));
        let node = bridge.get_node(1).unwrap();
        assert_eq!(node.role, AccessRole::Button);
        assert_eq!(node.name.as_deref(), Some("OK"));
    }

    #[test]
    fn bridge_update_replaces_existing() {
        let mut bridge = AccessibilityBridge::new();
        bridge.update_node(AccessNode::new(1, AccessRole::Label));
        bridge.update_node(AccessNode::new(1, AccessRole::Button).with_name("OK"));
        assert_eq!(bridge.node_count(), 1);
        assert_eq!(bridge.get_node(1).unwrap().role, AccessRole::Button);
    }

    #[test]
    fn bridge_remove_node() {
        let mut bridge = AccessibilityBridge::new();
        bridge.update_node(AccessNode::new(1, AccessRole::Group).with_children(vec![2]));
        bridge.update_node(AccessNode::new(2, AccessRole::Label));
        bridge.remove_node(2);
        assert_eq!(bridge.node_count(), 1);
        assert!(bridge.get_node(2).is_none());
        // 父节点的 children 也应被清理
        assert!(bridge.get_node(1).unwrap().children.is_empty());
    }

    #[test]
    fn bridge_set_root() {
        let mut bridge = AccessibilityBridge::new();
        bridge.set_root(1);
        assert!(bridge.root_node().is_none()); // 节点还没加
        bridge.update_node(AccessNode::new(1, AccessRole::Group));
        assert!(bridge.root_node().is_some());
    }

    #[test]
    fn bridge_dump_tree() {
        let mut bridge = AccessibilityBridge::new();
        bridge.set_root(1);
        bridge.update_node(
            AccessNode::new(1, AccessRole::Group)
                .with_name("root")
                .with_children(vec![2, 3]),
        );
        bridge.update_node(AccessNode::new(2, AccessRole::Button).with_name("OK"));
        bridge.update_node(AccessNode::new(3, AccessRole::Label).with_name("Hello"));
        let tree = bridge.dump_tree();
        assert!(tree.contains("Group(1)"));
        assert!(tree.contains("Button(2)"));
        assert!(tree.contains("Label(3)"));
    }

    #[test]
    fn node_builder_chain() {
        let node = AccessNode::new(1, AccessRole::Checkbox)
            .with_name("Accept terms")
            .with_description("You must accept to continue")
            .with_focusable(true)
            .with_selected(true)
            .with_children(vec![2, 3]);
        assert_eq!(node.role, AccessRole::Checkbox);
        assert!(node.focusable);
        assert_eq!(node.selected, Some(true));
        assert_eq!(node.children, vec![2, 3]);
    }
}
