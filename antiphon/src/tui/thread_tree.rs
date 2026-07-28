use std::collections::HashMap;

/// One message's reply relationship, all the tree needs to
/// know about it. Ids are bare Message-IDs so they compare
/// equal to the References/In-Reply-To entries.
pub(super) struct Reply<'a> {
    pub(super) id: &'a str,
    pub(super) in_reply_to: Option<&'a str>,
    pub(super) references: Vec<&'a str>,
    pub(super) date_unix: i64,
}

/// A node in pre-order: its depth, who it answers, its replies
/// (all in pre-order index space), the size of its subtree and
/// whether that subtree is folded away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ThreadNode {
    pub(super) depth: usize,
    pub(super) parent: Option<usize>,
    pub(super) children: Vec<usize>,
    pub(super) descendants: usize,
    pub(super) collapsed: bool,
}

/// The reply tree over one thread's messages, indexed 1:1 with
/// the message list once it has been reordered into pre-order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ThreadTree {
    pub(super) nodes: Vec<ThreadNode>,
}

/// Builds the tree from a thread's messages. Returns the
/// pre-order of the input (original indices, roots first then
/// each reply nested under its parent, siblings in date order)
/// alongside the nodes aligned to that pre-order.
pub(super) fn build(items: &[Reply]) -> (Vec<usize>, ThreadTree) {
    let index = index_by_id(items);
    let parent = resolve_parents(items, &index);
    let children = children_of(&parent, items);
    let roots = roots_of(&parent, items);
    let order = preorder(&roots, &children);
    let tree = assemble(&order, &parent, &children);
    (order, tree)
}

fn index_by_id<'a>(items: &'a [Reply]) -> HashMap<&'a str, usize> {
    let mut index = HashMap::new();
    for (position, item) in items.iter().enumerate() {
        index.entry(item.id).or_insert(position);
    }
    index
}

fn resolve_parents(
    items: &[Reply],
    index: &HashMap<&str, usize>,
) -> Vec<Option<usize>> {
    let mut parent: Vec<Option<usize>> = items
        .iter()
        .enumerate()
        .map(|(position, item)| parent_candidate(item, index, position))
        .collect();
    break_cycles(&mut parent);
    parent
}

/// The nearest ancestor present in the set: In-Reply-To if it
/// is here, else the most recent surviving entry of References.
/// A message whose parent is absent stays a root.
fn parent_candidate(
    item: &Reply,
    index: &HashMap<&str, usize>,
    position: usize,
) -> Option<usize> {
    let mut candidates = item.references.clone();
    if let Some(parent) = item.in_reply_to
        && candidates.last() != Some(&parent)
    {
        candidates.push(parent);
    }
    candidates.iter().rev().find_map(|id| {
        let found = *index.get(id)?;
        (found != position).then_some(found)
    })
}

/// A partial thread can name a parent that in turn descends
/// from the child; any such loop is cut at the offending edge
/// so the message re-roots rather than vanishing.
fn break_cycles(parent: &mut [Option<usize>]) {
    let count = parent.len();
    for start in 0..count {
        let mut steps = 0;
        let mut cursor = parent[start];
        while let Some(node) = cursor {
            if node == start || steps > count {
                parent[start] = None;
                break;
            }
            cursor = parent[node];
            steps += 1;
        }
    }
}

fn children_of(
    parent: &[Option<usize>],
    items: &[Reply],
) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); parent.len()];
    for (child, slot) in parent.iter().enumerate() {
        if let Some(parent) = slot {
            children[*parent].push(child);
        }
    }
    for group in &mut children {
        group.sort_by(|a, b| sibling_key(items, *a, *b));
    }
    children
}

fn roots_of(parent: &[Option<usize>], items: &[Reply]) -> Vec<usize> {
    let mut roots: Vec<usize> = (0..parent.len())
        .filter(|position| parent[*position].is_none())
        .collect();
    roots.sort_by(|a, b| sibling_key(items, *a, *b));
    roots
}

/// Siblings read oldest first, ties broken by id so the order
/// is total and stable.
fn sibling_key(
    items: &[Reply],
    a: usize,
    b: usize,
) -> std::cmp::Ordering {
    items[a]
        .date_unix
        .cmp(&items[b].date_unix)
        .then_with(|| items[a].id.cmp(items[b].id))
}

fn preorder(roots: &[usize], children: &[Vec<usize>]) -> Vec<usize> {
    let mut order = Vec::with_capacity(children.len());
    let mut stack: Vec<usize> = roots.iter().rev().copied().collect();
    while let Some(node) = stack.pop() {
        order.push(node);
        stack.extend(children[node].iter().rev().copied());
    }
    order
}

fn assemble(
    order: &[usize],
    parent: &[Option<usize>],
    children: &[Vec<usize>],
) -> ThreadTree {
    let position: HashMap<usize, usize> = order
        .iter()
        .enumerate()
        .map(|(slot, origin)| (*origin, slot))
        .collect();
    let mut nodes: Vec<ThreadNode> = order
        .iter()
        .map(|origin| ThreadNode {
            depth: 0,
            parent: parent[*origin].map(|p| position[&p]),
            children: children[*origin]
                .iter()
                .map(|child| position[child])
                .collect(),
            descendants: 0,
            collapsed: false,
        })
        .collect();
    for slot in 0..nodes.len() {
        if let Some(parent) = nodes[slot].parent {
            nodes[slot].depth = nodes[parent].depth + 1;
        }
    }
    for slot in (0..nodes.len()).rev() {
        let descendants = nodes[slot]
            .children
            .iter()
            .map(|child| 1 + nodes[*child].descendants)
            .sum();
        nodes[slot].descendants = descendants;
    }
    ThreadTree { nodes }
}

impl ThreadTree {
    pub(super) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Hidden when any ancestor is folded shut; roots are
    /// always visible.
    pub(super) fn is_visible(&self, position: usize) -> bool {
        let mut cursor =
            self.nodes.get(position).and_then(|node| node.parent);
        while let Some(parent) = cursor {
            if self.nodes[parent].collapsed {
                return false;
            }
            cursor = self.nodes[parent].parent;
        }
        true
    }

    pub(super) fn visible(&self) -> Vec<usize> {
        (0..self.nodes.len())
            .filter(|position| self.is_visible(*position))
            .collect()
    }

    pub(super) fn next_visible(&self, from: usize) -> usize {
        ((from + 1)..self.nodes.len())
            .find(|position| self.is_visible(*position))
            .unwrap_or(from)
    }

    pub(super) fn prev_visible(&self, from: usize) -> usize {
        (0..from)
            .rev()
            .find(|position| self.is_visible(*position))
            .unwrap_or(from)
    }

    pub(super) fn last_visible(&self) -> usize {
        (0..self.nodes.len())
            .rev()
            .find(|position| self.is_visible(*position))
            .unwrap_or(0)
    }

    /// Folds or unfolds a subtree; only a node with replies can
    /// change, so the caller can report a bare leaf.
    pub(super) fn set_collapsed(
        &mut self,
        position: usize,
        collapsed: bool,
    ) -> bool {
        let Some(node) = self.nodes.get_mut(position) else {
            return false;
        };
        if node.descendants == 0 {
            return false;
        }
        node.collapsed = collapsed;
        true
    }

    pub(super) fn toggle(&mut self, position: usize) -> bool {
        let collapsed =
            self.nodes.get(position).is_some_and(|node| node.collapsed);
        self.set_collapsed(position, !collapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply<'a>(
        id: &'a str,
        parent: Option<&'a str>,
        date: i64,
    ) -> Reply<'a> {
        Reply {
            id,
            in_reply_to: parent,
            references: parent.into_iter().collect(),
            date_unix: date,
        }
    }

    /// Ids in visible pre-order, so a test can assert the shape
    /// the reader sees.
    fn shape(order: &[usize], items: &[Reply]) -> Vec<String> {
        order.iter().map(|i| items[*i].id.to_string()).collect()
    }

    #[test]
    fn replies_nest_under_their_parent_in_date_order() {
        let items = [
            reply("root", None, 0),
            reply("b", Some("root"), 20),
            reply("a", Some("root"), 10),
            reply("a1", Some("a"), 15),
        ];
        let (order, tree) = build(&items);
        assert_eq!(shape(&order, &items), ["root", "a", "a1", "b"]);
        let depths: Vec<usize> =
            tree.nodes.iter().map(|node| node.depth).collect();
        assert_eq!(depths, [0, 1, 2, 1]);
        assert_eq!(tree.nodes[0].descendants, 3);
        assert_eq!(tree.nodes[1].descendants, 1);
    }

    #[test]
    fn an_orphan_reply_attaches_to_the_root() {
        // "stray" answers a message not in the set, so it
        // becomes its own root; roots sort by date.
        let items = [
            reply("root", None, 0),
            reply("child", Some("root"), 5),
            reply("stray", Some("missing@elsewhere"), 3),
        ];
        let (order, tree) = build(&items);
        assert_eq!(shape(&order, &items), ["root", "child", "stray"]);
        assert_eq!(tree.nodes[0].parent, None);
        assert_eq!(tree.nodes[1].parent, Some(0));
        assert_eq!(tree.nodes[2].parent, None);
    }

    #[test]
    fn references_pick_the_nearest_present_ancestor() {
        // grandchild's In-Reply-To names a gap; the last
        // surviving reference (root) becomes its parent.
        let grandchild = Reply {
            id: "gc",
            in_reply_to: Some("gone@x"),
            references: vec!["root", "gone@x"],
            date_unix: 9,
        };
        let items = [reply("root", None, 0), grandchild];
        let (_order, tree) = build(&items);
        assert_eq!(tree.nodes[1].parent, Some(0));
        assert_eq!(tree.nodes[1].depth, 1);
    }

    #[test]
    fn a_reference_cycle_re_roots_rather_than_looping() {
        let items = [
            Reply {
                id: "x",
                in_reply_to: Some("y"),
                references: vec!["y"],
                date_unix: 0,
            },
            Reply {
                id: "y",
                in_reply_to: Some("x"),
                references: vec!["x"],
                date_unix: 1,
            },
        ];
        let (order, tree) = build(&items);
        assert_eq!(order.len(), 2);
        let roots = tree
            .nodes
            .iter()
            .filter(|node| node.parent.is_none())
            .count();
        assert!(roots >= 1, "the cycle must yield a root");
    }

    #[test]
    fn a_collapsed_subtree_hides_its_descendants() {
        let items = [
            reply("root", None, 0),
            reply("a", Some("root"), 10),
            reply("a1", Some("a"), 15),
            reply("b", Some("root"), 20),
        ];
        let (_order, mut tree) = build(&items);
        assert_eq!(tree.visible(), [0, 1, 2, 3]);
        assert!(tree.set_collapsed(1, true));
        assert_eq!(tree.visible(), [0, 1, 3]);
        assert_eq!(tree.nodes[1].descendants, 1);
        assert!(tree.toggle(1));
        assert_eq!(tree.visible(), [0, 1, 2, 3]);
        assert!(!tree.set_collapsed(2, true), "a leaf has no fold");
    }

    #[test]
    fn navigation_steps_over_the_visible_nodes_only() {
        let items = [
            reply("root", None, 0),
            reply("a", Some("root"), 10),
            reply("a1", Some("a"), 15),
            reply("b", Some("root"), 20),
        ];
        let (_order, mut tree) = build(&items);
        assert_eq!(tree.next_visible(0), 1);
        assert_eq!(tree.next_visible(3), 3, "clamps at the last node");
        assert_eq!(tree.prev_visible(2), 1);
        tree.set_collapsed(1, true);
        assert_eq!(tree.next_visible(1), 3, "the fold is skipped");
        assert_eq!(tree.prev_visible(3), 1);
        assert_eq!(tree.last_visible(), 3);
    }
}
