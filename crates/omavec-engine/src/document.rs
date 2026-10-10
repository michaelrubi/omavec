//! The node tree. Nodes are shared through `Arc`s and copied only along the
//! path to a change, so a snapshot of a document for undo shares every node
//! that an edit didn't touch.

use std::sync::Arc;

use omavec_geom::kurbo::{Affine, Point, Rect, Size};
use serde::{Deserialize, Serialize};

use crate::paint::{Color, Paint, Stroke, is_no, is_one, is_yes, one, yes};

/// A node's identity, kept across saves so diffs stay small and instance
/// overrides can name nodes inside components.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
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
    #[error("nothing is selected")]
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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

    /// What Figma fills a new node of this kind with.
    fn fills(&self) -> Vec<Paint> {
        match self {
            NodeKind::Page | NodeKind::Group => Vec::new(),
            NodeKind::Frame { .. } => vec![Paint::solid(Color::rgb(0xff, 0xff, 0xff))],
            NodeKind::Rectangle | NodeKind::Ellipse => vec![Paint::solid(Color::rgb(0xd9, 0xd9, 0xd9))],
        }
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

/// In a file, whatever has its usual value is left out, so a plain
/// rectangle is five lines and a diff shows only what was changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(flatten)]
    pub kind: NodeKind,
    pub name: String,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "is_no")]
    pub locked: bool,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f64,
    /// Where the node sits in its parent.
    #[serde(default, skip_serializing_if = "is_identity")]
    pub transform: Affine,
    pub size: Size,
    /// Bottom to top: the last fill is painted over the others.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<Paint>,
    #[serde(default, skip_serializing_if = "Stroke::is_none")]
    pub stroke: Stroke,
    /// Back to front: the last child is drawn on top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Arc<Node>>,
}

fn is_identity(transform: &Affine) -> bool {
    *transform == Affine::IDENTITY
}

impl Node {
    /// Whether two trees are equal, without walking what they share: after
    /// an edit that is everything off the path to the change.
    pub(crate) fn same(a: &Arc<Node>, b: &Arc<Node>) -> bool {
        if Arc::ptr_eq(a, b) {
            return true;
        }
        // Spelled out so that a new field can't be forgotten here.
        let Node { id, kind, name, visible, locked, opacity, transform, size, fills, stroke, children } = &**a;
        (id, kind, name, visible, locked, opacity, transform, size, fills, stroke) == (&b.id, &b.kind, &b.name, &b.visible, &b.locked, &b.opacity, &b.transform, &b.size, &b.fills, &b.stroke)
            && children.len() == b.children.len()
            && children.iter().zip(&b.children).all(|(a, b)| Node::same(a, b))
    }

    /// The node's box in its own coordinates. A group has none of its own:
    /// its box is whatever holds its children.
    pub fn bounds(&self) -> Rect {
        match self.kind {
            NodeKind::Page | NodeKind::Group => self.children.iter().map(|child| child.transform.transform_rect_bbox(child.bounds())).reduce(|a, b| a.union(b)).unwrap_or_default(),
            _ => Rect::from_origin_size((0.0, 0.0), self.size),
        }
    }

    /// Whether `point`, in this node's own coordinates, is on its shape.
    /// Pages and groups have no shape of their own.
    fn covers(&self, point: Point) -> bool {
        let inside = Rect::from_origin_size((0.0, 0.0), self.size).contains(point);
        match self.kind {
            NodeKind::Page | NodeKind::Group => false,
            NodeKind::Frame { .. } | NodeKind::Rectangle => inside,
            NodeKind::Ellipse => {
                let (x, y) = (point.x / self.size.width * 2.0 - 1.0, point.y / self.size.height * 2.0 - 1.0);
                inside && x * x + y * y <= 1.0
            }
        }
    }

    /// The nodes under `point` (in this node's coordinates), from this
    /// node's child down to the deepest, taking the front-most at each
    /// level. Hidden and locked nodes aren't there to be hit. Empty if the
    /// point is on nothing.
    pub fn hit(&self, point: Point) -> Vec<NodeId> {
        for child in self.children.iter().rev().filter(|child| child.visible && !child.locked) {
            // A transform that can't be undone puts the point nowhere.
            let local = child.transform.inverse() * point;
            let covered = child.covers(local);
            // A frame that clips shows nothing of its children outside itself.
            let clipped = matches!(child.kind, NodeKind::Frame { clip: true }) && !covered;
            let mut chain = if clipped { Vec::new() } else { child.hit(local) };
            if covered || !chain.is_empty() {
                chain.insert(0, child.id);
                return chain;
            }
        }
        Vec::new()
    }
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
    /// A document read from a file. Ids already in `pages` are never handed
    /// out again, whatever the file said the next one was.
    pub(crate) fn from_parts(pages: Vec<Arc<Node>>, next_id: u64) -> Self {
        fn highest(nodes: &[Arc<Node>]) -> u64 {
            nodes.iter().map(|node| node.id.0.max(highest(&node.children))).max().unwrap_or(0)
        }
        let next_id = next_id.max(highest(&pages).saturating_add(1));
        Self { pages, next_id }
    }

    pub(crate) fn next_id(&self) -> u64 {
        self.next_id
    }

    /// A new node of `kind` with an id of its own and Figma's fill for the
    /// kind, not yet in the tree.
    pub fn create(&mut self, kind: NodeKind, size: Size) -> Node {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        Node { id, name: kind.label().into(), visible: true, locked: false, opacity: 1.0, transform: Affine::IDENTITY, size, fills: kind.fills(), stroke: Stroke::default(), kind, children: Vec::new() }
    }

    /// A copy of `node` and everything in it, with ids of their own.
    pub fn copy_of(&mut self, node: &Node) -> Node {
        let mut copy = node.clone();
        copy.id = NodeId(self.next_id);
        self.next_id += 1;
        copy.children = node.children.iter().map(|child| Arc::new(self.copy_of(child))).collect();
        copy
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

    /// From `id`'s own coordinates to its page's: every transform from the
    /// page down to it.
    pub fn to_page(&self, id: NodeId) -> Option<Affine> {
        let mut nodes = &self.pages;
        let mut transform = Affine::IDENTITY;
        for index in self.path(id)? {
            let node = nodes.get(index)?;
            (transform, nodes) = (transform * node.transform, &node.children);
        }
        Some(transform)
    }

    pub(crate) fn container(&self, id: NodeId) -> Result<&Node, Error> {
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
