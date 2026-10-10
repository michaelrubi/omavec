//! Rearranging the tree: grouping, duplicating, restacking, copying and
//! pasting. All of it leaves nodes where they were on the page.

use std::sync::Arc;

use omavec_geom::kurbo::{Affine, Rect};

use crate::document::{Document, Error, Node, NodeId, NodeKind};

/// Where a restack puts nodes among their siblings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    Front,
    Forward,
    Backward,
    Back,
}

impl Document {
    /// `ids` as the document has them, back to front, without pages,
    /// repeats, or anything inside another of them.
    pub fn roots(&self, ids: &[NodeId]) -> Vec<NodeId> {
        fn walk(nodes: &[Arc<Node>], ids: &[NodeId], found: &mut Vec<NodeId>) {
            for node in nodes {
                if ids.contains(&node.id) {
                    found.push(node.id);
                } else {
                    walk(&node.children, ids, found);
                }
            }
        }
        let mut found = Vec::new();
        for page in &self.pages {
            walk(&page.children, ids, &mut found);
        }
        found
    }

    /// `id`'s parent and its place among the parent's children.
    fn place(&self, id: NodeId) -> Result<(NodeId, usize), Error> {
        let parent = self.parent(id).ok_or(Error::NoSuchNode(id))?;
        let index = self.node(parent).and_then(|parent| parent.children.iter().position(|child| child.id == id));
        Ok((parent, index.ok_or(Error::NoSuchNode(id))?))
    }

    /// Wraps `ids` in a new group or frame (`kind`), put where the
    /// front-most of them was. A frame made this way has no fill, so
    /// nothing looks different.
    pub fn group(&mut self, ids: &[NodeId], kind: NodeKind) -> Result<NodeId, Error> {
        let roots = self.roots(ids);
        let top = *roots.last().ok_or(Error::Nothing)?;
        if !matches!(kind, NodeKind::Frame { .. } | NodeKind::Group) {
            return Err(Error::NotAContainer(top));
        }
        let parent = self.parent(top).ok_or(Error::NoSuchNode(top))?;
        let to_parent = self.to_page(parent).ok_or(Error::NoSuchNode(parent))?.inverse();
        let mut members = Vec::new();
        let mut index = 0;
        for id in &roots {
            let on_page = self.to_page(*id).ok_or(Error::NoSuchNode(*id))?;
            // The front-most is last, so the others have gone from under it.
            let from;
            (from, index) = self.place(*id)?;
            let mut node = self.remove(*id)?;
            // A node that stays in its parent isn't sent round by the page,
            // which would cost it its round numbers.
            if from != parent {
                Arc::make_mut(&mut node).transform = to_parent * on_page;
            }
            members.push(node);
        }
        let area = members.iter().map(|node| node.transform.transform_rect_bbox(node.bounds())).reduce(|a, b| a.union(b)).unwrap_or_default();
        let mut group = self.create(kind, area.size());
        group.fills.clear();
        group.transform = Affine::translate(area.origin().to_vec2());
        for node in &mut members {
            Arc::make_mut(node).transform = Affine::translate(-area.origin().to_vec2()) * node.transform;
        }
        group.children = members;
        let id = group.id;
        self.insert(parent, index, group)?;
        Ok(id)
    }

    /// Takes the group or frame `id` away and leaves what was in it in its
    /// place. Returns what was in it.
    pub fn ungroup(&mut self, id: NodeId) -> Result<Vec<NodeId>, Error> {
        self.container(id)?;
        let (parent, index) = self.place(id).map_err(|_| Error::PageInsideNode)?;
        let group = self.remove(id)?;
        let mut freed = Vec::new();
        for (offset, child) in group.children.iter().enumerate() {
            let mut child = child.clone();
            Arc::make_mut(&mut child).transform = group.transform * child.transform;
            freed.push(child.id);
            self.insert(parent, index + offset, child)?;
        }
        Ok(freed)
    }

    /// Copies each of `ids` to just in front of itself. Returns the copies.
    pub fn duplicate(&mut self, ids: &[NodeId]) -> Result<Vec<NodeId>, Error> {
        let mut copies = Vec::new();
        for id in self.roots(ids) {
            let (parent, index) = self.place(id)?;
            let original = self.node(id).ok_or(Error::NoSuchNode(id))?.clone();
            let copy = self.copy_of(&original);
            copies.push(copy.id);
            self.insert(parent, index + 1, copy)?;
        }
        Ok(copies)
    }

    /// Moves `ids` among their siblings: one place forward or back past
    /// whatever isn't moving with them, or all the way.
    pub fn restack(&mut self, ids: &[NodeId], to: Stack) -> Result<(), Error> {
        let roots = self.roots(ids);
        let mut parents: Vec<NodeId> = roots.iter().filter_map(|id| self.parent(*id)).collect();
        parents.sort();
        parents.dedup();
        for parent in parents {
            let children = &mut self.node_mut(parent)?.children;
            let moving: Vec<bool> = children.iter().map(|child| roots.contains(&child.id)).collect();
            let mut order: Vec<usize> = (0..children.len()).collect();
            match to {
                // Stable, so each lot keeps its own order.
                Stack::Front => order.sort_by_key(|i| moving[*i]),
                Stack::Back => order.sort_by_key(|i| !moving[*i]),
                Stack::Forward => {
                    for at in (0..order.len().saturating_sub(1)).rev() {
                        if moving[order[at]] && !moving[order[at + 1]] {
                            order.swap(at, at + 1);
                        }
                    }
                }
                Stack::Backward => {
                    for at in 1..order.len() {
                        if moving[order[at]] && !moving[order[at - 1]] {
                            order.swap(at, at - 1);
                        }
                    }
                }
            }
            *children = order.into_iter().map(|i| children[i].clone()).collect();
        }
        Ok(())
    }

    /// `ids` for the clipboard: each node as it is, but placed on its page
    /// instead of in its parent, so it can be pasted anywhere.
    pub fn copy(&self, ids: &[NodeId]) -> Vec<Arc<Node>> {
        let copy = |id: NodeId| {
            let mut node = self.node(id)?.clone();
            node.transform = self.to_page(id)?;
            Some(Arc::new(node))
        };
        self.roots(ids).into_iter().filter_map(copy).collect()
    }

    /// Puts copies of `nodes` (from [`Self::copy`]) on top in `parent`,
    /// where they were on the page. Returns the copies.
    pub fn paste(&mut self, parent: NodeId, nodes: &[Arc<Node>]) -> Result<Vec<NodeId>, Error> {
        let to_parent = self.to_page(parent).ok_or(Error::NoSuchNode(parent))?.inverse();
        let mut pasted = Vec::new();
        for node in nodes {
            let mut copy = self.copy_of(node);
            copy.transform = to_parent * copy.transform;
            pasted.push(copy.id);
            self.insert(parent, usize::MAX, copy)?;
        }
        Ok(pasted)
    }

    /// The box round all of `ids` on their page; `None` if there are none.
    pub fn area(&self, ids: &[NodeId]) -> Option<Rect> {
        let on_page = |id: &NodeId| Some(self.to_page(*id)?.transform_rect_bbox(self.node(*id)?.bounds()));
        ids.iter().filter_map(on_page).reduce(|a, b| a.union(b))
    }
}
