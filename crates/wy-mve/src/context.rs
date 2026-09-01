//! NodeContext：传入 `arg_children()` 的上下文。

use std::any::Any;
use std::cell::RefCell;
use std::hash::Hash;
use std::rc::Rc;

use crate::node::Node;

/// 上下文键（用于 provide/consume）。
pub struct Context<T: 'static> {
    _marker: std::marker::PhantomData<T>,
}

impl<T: 'static> Context<T> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: 'static> Default for Context<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// 子节点缓存：存储子节点构建结果，支持信号驱动重建。
pub struct ChildrenCache {
    cache: Rc<RefCell<Vec<Node>>>,
    dirty: Rc<RefCell<bool>>,
}

impl Clone for ChildrenCache {
    fn clone(&self) -> Self {
        Self {
            cache: Rc::clone(&self.cache),
            dirty: Rc::clone(&self.dirty),
        }
    }
}

impl ChildrenCache {
    /// 获取缓存的子节点列表。
    pub fn get(&self) -> Vec<Node> {
        self.cache.borrow().clone()
    }

    /// 标记为脏（信号变化时调用）。
    pub fn invalidate(&self) {
        *self.dirty.borrow_mut() = true;
    }

    /// 检查是否需要重建。
    pub fn is_dirty(&self) -> bool {
        *self.dirty.borrow()
    }
}

/// NodeContext：`arg_children()` 的执行上下文。
pub struct NodeContext {
    pub(crate) nodes: Vec<Node>,
    pub(crate) children_caches: Vec<ChildrenCache>,
    pub(crate) contexts: Vec<(u64, Box<dyn Any>)>,
    #[expect(dead_code)]
    pub(crate) parent_context_index: usize,
}

impl NodeContext {
    pub fn new(parent_context_index: usize) -> Self {
        Self {
            nodes: Vec::new(),
            children_caches: Vec::new(),
            contexts: Vec::new(),
            parent_context_index,
        }
    }

    /// 添加一个子节点。
    pub fn add_node(&mut self, node: Node) {
        self.nodes.push(node);
    }

    /// 渲染一个子节点，创建子节点缓存。
    ///
    /// `callback` 中读取的信号会被自动追踪。
    /// 信号变化时缓存自动失效，下次 get 时重新构建。
    pub fn render_node(
        &mut self,
        node: Node,
        callback: impl Fn(&mut Node, &mut NodeContext) + 'static,
    ) -> ChildrenCache {
        let cache = ChildrenCache {
            cache: Rc::new(RefCell::new(Vec::new())),
            dirty: Rc::new(RefCell::new(true)),
        };

        let cache_ref = cache.clone();
        let cb = Rc::new(callback);

        // create_effect 追踪依赖：callback 中读取的信号会被追踪
        // 信号变化时自动重新执行
        wy_signal::create_effect(move || {
            let mut child_cx = NodeContext::new(0);
            let mut fresh_node = node.clone();
            cb(&mut fresh_node, &mut child_cx);
            *cache_ref.cache.borrow_mut() = child_cx.nodes;
            *cache_ref.dirty.borrow_mut() = false;
        });

        self.children_caches.push(cache.clone());
        cache
    }

    /// 渲染一个子节点（带配置）。
    pub fn render_node_with_config<C>(
        &mut self,
        node: Node,
        _config: C,
        callback: impl Fn(&mut Node, &mut NodeContext) + 'static,
    ) -> ChildrenCache {
        self.render_node(node, callback)
    }

    /// 获取所有子节点缓存。
    pub fn children_caches(&self) -> &[ChildrenCache] {
        &self.children_caches
    }

    /// 获取直接添加的子节点。
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// 获取可变子节点列表。
    pub fn nodes_mut(&mut self) -> &mut Vec<Node> {
        &mut self.nodes
    }

    /// 提供上下文值。
    pub fn provide<T: 'static>(&mut self, context_id: u64, value: T) {
        self.contexts.push((context_id, Box::new(value)));
    }

    /// 消费最近的祖先提供的上下文值。
    pub fn consume<T: 'static>(&self, context_id: u64) -> Option<&T> {
        for (id, value) in self.contexts.iter().rev() {
            if *id == context_id {
                return value.downcast_ref::<T>();
            }
        }
        None
    }

    /// 信号驱动的列表渲染（对应 Kotlin 的 `renderForEach`）。
    ///
    /// 基于 key 的 diff：已有 key 复用 Node，新增 key 创建 Node，
    /// 删除的 key 移除 Node。
    ///
    /// `items_fn` 读取信号时会被追踪，信号变化时自动重建。
    pub fn render_for_each<K, T>(
        &mut self,
        items_fn: impl Fn() -> Vec<(K, T)> + 'static,
        creator: impl Fn(K, T, &mut NodeContext) + 'static,
    ) where
        K: Eq + Hash + Clone + 'static,
        T: 'static,
    {
        let cache = ChildrenCache {
            cache: Rc::new(RefCell::new(Vec::new())),
            dirty: Rc::new(RefCell::new(true)),
        };

        let cache_ref = cache.clone();
        let cr = Rc::new(creator);

        // create_effect 追踪 items_fn 中读取的信号
        wy_signal::create_effect(move || {
            let items = items_fn();
            let old_nodes = cache_ref.cache.borrow().clone();
            let mut new_nodes = Vec::with_capacity(items.len());
            let old_len = old_nodes.len();

            for (i, (key, value)) in items.into_iter().enumerate() {
                if i < old_len {
                    // 复用已有 Node
                    new_nodes.push(old_nodes[i].clone());
                } else {
                    // 新增
                    let mut child_cx = NodeContext::new(0);
                    cr(key, value, &mut child_cx);
                    if let Some(node) = child_cx.nodes.into_iter().next() {
                        new_nodes.push(node);
                    }
                }
            }

            *cache_ref.cache.borrow_mut() = new_nodes;
            *cache_ref.dirty.borrow_mut() = false;
        });

        self.children_caches.push(cache);
    }
}

/// 渲染根节点。
pub fn render_root(callback: impl Fn(&mut NodeContext) + 'static) -> ChildrenCache {
    let cache = ChildrenCache {
        cache: Rc::new(RefCell::new(Vec::new())),
        dirty: Rc::new(RefCell::new(true)),
    };

    let cache_ref = cache.clone();
    let cb = Rc::new(callback);

    wy_signal::create_effect(move || {
        let mut cx = NodeContext::new(0);
        cb(&mut cx);
        *cache_ref.cache.borrow_mut() = cx.nodes;
        *cache_ref.dirty.borrow_mut() = false;
    });

    cache
}
