//! The node tree. Nodes are shared through `Arc`s and copied only along the
//! path to a change, so a snapshot of a document for undo shares every node
//! that an edit didn't touch.

use std::sync::Arc;

use omavec_geom::kurbo::{Affine, Size};

/// A node's identity, kept across saves so diffs stay small and instance
/// overrides can name nodes inside components.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u64);

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("there is no node {0:?}")]
    NoSuchNode(NodeId),
    #[error("node {0:?} can't hold other nodes")]
    NotAContainer(NodeId),
    #[error("node {0:?} can't be moved into itself")]
    IntoItself(NodeId),
    #[error("a page can only sit at the top of the document")]
    PageInsideNode,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// A page: the top of each tree, and the only kind found there.
    Page,
    /// A container with its own size. Top-level frames are the artboards.
    Frame { clip: bool },
    /// A container that is only the sum of its children.
    Group,
    Rectangle,
    Ellipse,
}

impl NodeKind {
    pub fn is_container(&self) -> bool {
        matches!(self, NodeKind::Page | NodeKind::Frame { .. } | NodeKind::Group)
    }

    /// What Figma calls a new node of this kind.
    fn label(&self) -> &'static str {
        match self {
            NodeKind::Page => "Page",
            NodeKind::Frame { .. } => "Frame",
            NodeKind::Group => "Group",
            NodeKind::Rectangle => "Rectangle",
            NodeKind::Ellipse => "Ellipse",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f64,
    /// Where the node sits in its parent.
    pub transform: Affine,
    pub size: Size,
    pub kind: NodeKind,
    /// Back to front: the last child is drawn on top.
    pub children: Vec<Arc<Node>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub pages: Vec<Arc<Node>>,
    /// The next id to hand out. Ids are never reused, even after a delete.
    next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        let mut document = Self { pages: Vec::new(), next_id: 1 };
        document.add_page("Page 1");
        document
    }
}

impl Document {
    /// A new node of `kind` with an id of its own, not yet in the tree.
    pub fn create(&mut self, kind: NodeKind, size: Size) -> Node {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        Node { id, name: kind.label().into(), visible: true, locked: false, opacity: 1.0, transform: Affine::IDENTITY, size, kind, children: Vec::new() }
    }

    pub fn add_page(&mut self, name: &str) -> NodeId {
        let mut page = self.create(NodeKind::Page, Size::ZERO);
        page.name = name.into();
        let id = page.id;
        self.pages.push(Arc::new(page));
        id
    }

    /// The child indices that lead from the pages down to `id`.
    fn path(&self, id: NodeId) -> Option<Vec<usize>> {
        fn find(nodes: &[Arc<Node>], id: NodeId, path: &mut Vec<usize>) -> bool {
            for (index, node) in nodes.iter().enumerate() {
                path.push(index);
                if node.id == id || find(&node.children, id, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        find(&self.pages, id, &mut path).then_some(path)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        let path = self.path(id)?;
        let mut nodes = &self.pages;
        let mut found = None;
        for index in path {
            let node = nodes.get(index)?;
            (found, nodes) = (Some(&**node), &node.children);
        }
        found
    }

    /// The node to change. Every node from its page down to it is copied if a
    /// snapshot still shares it; the rest of the tree stays shared.
    pub fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, Error> {
        let path = self.path(id).ok_or(Error::NoSuchNode(id))?;
        let (last, above) = path.split_last().ok_or(Error::NoSuchNode(id))?;
        let mut nodes = &mut self.pages;
        for &index in above {
            nodes = &mut Arc::make_mut(nodes.get_mut(index).ok_or(Error::NoSuchNode(id))?).children;
        }
        Ok(Arc::make_mut(nodes.get_mut(*last).ok_or(Error::NoSuchNode(id))?))
    }

    /// The node `id` is a child of. `None` for a page, and for no node at all.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let mut path = self.path(id)?;
        path.pop();
        let mut nodes = &self.pages;
        let mut parent = None;
        for index in path {
            let node = nodes.get(index)?;
            (parent, nodes) = (Some(node.id), &node.children);
        }
        parent
    }

    fn container(&self, id: NodeId) -> Result<&Node, Error> {
        let node = self.node(id).ok_or(Error::NoSuchNode(id))?;
        if node.kind.is_container() { Ok(node) } else { Err(Error::NotAContainer(id)) }
    }

    /// Puts `node` among `parent`'s children at `index` (0 is the back), or
    /// on top if there are fewer than that.
    pub fn insert(&mut self, parent: NodeId, index: usize, node: impl Into<Arc<Node>>) -> Result<(), Error> {
        let node = node.into();
        if node.kind == NodeKind::Page {
            return Err(Error::PageInsideNode);
        }
        self.container(parent)?;
        let children = &mut self.node_mut(parent)?.children;
        children.insert(index.min(children.len()), node);
        Ok(())
    }

    /// Takes `id` out of the tree, with everything inside it.
    pub fn remove(&mut self, id: NodeId) -> Result<Arc<Node>, Error> {
        let siblings = match self.parent(id) {
            Some(parent) => &mut self.node_mut(parent)?.children,
            None => &mut self.pages,
        };
        let index = siblings.iter().position(|node| node.id == id).ok_or(Error::NoSuchNode(id))?;
        Ok(siblings.remove(index))
    }

    /// Moves `id` to `index` among `parent`'s children: a new place in the
    /// same parent, or a new parent. `index` counts the children without it.
    pub fn move_to(&mut self, id: NodeId, parent: NodeId, index: usize) -> Result<(), Error> {
        // Check everything first, so a refused move changes nothing.
        let node = self.node(id).ok_or(Error::NoSuchNode(id))?;
        if node.kind == NodeKind::Page {
            return Err(Error::PageInsideNode);
        }
        self.container(parent)?;
        let mut above = Some(parent);
        while let Some(ancestor) = above {
            if ancestor == id {
                return Err(Error::IntoItself(id));
            }
            above = self.parent(ancestor);
        }
        let node = self.remove(id)?;
        self.insert(parent, index, node)
    }
}
