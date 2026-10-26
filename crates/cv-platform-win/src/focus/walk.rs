//! The bounded walk of a dialog's tree (ADR 0016), as pure logic. A `Source` fetches one level of
//! children (UI Automation or MSAA, in dialog.rs); `walk` decides what to enter and when to stop.
//! Tested here with a fake tree, because the real ones need a dialog on screen.

use cv_core::{
    DialogNode, Role,
    focus::{DIALOG_CALLS, DIALOG_DEPTH, DIALOG_NODES},
};

/// A call into the dialog's program timed out; the walk ends and the window counts as hung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Hung;

/// The children of one node, each with a handle when it could have children of its own.
pub(super) type Children<H> = Vec<(DialogNode, Option<H>)>;

/// One level of a tree.
pub(super) trait Source {
    /// What `children` needs to fetch the next level below a node.
    type Handle;

    /// The children of `parent`, in tree order, each with a handle when it could have children of
    /// its own. The node's `depth` is set by the walk. A refused or failed call is an empty list;
    /// a timeout is `Err(Hung)`.
    fn children(&mut self, parent: &Self::Handle) -> Result<Children<Self::Handle>, Hung>;
}

/// What a walk found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Walk {
    /// Depth first: a container, then its children.
    pub nodes: Vec<DialogNode>,
    /// False when the budget ran out or a container was too deep to open.
    pub complete: bool,
    /// `children` calls made.
    pub calls: usize,
}

/// A role the walk goes into: containers with no meaning of their own. Lists, trees, tables,
/// documents, edits, combo boxes, toolbars, menus and tabs are not in this list, so a file view
/// is never walked.
pub(super) fn is_container(role: Role) -> bool {
    matches!(role, Role::Pane | Role::Group | Role::Custom | Role::Unknown | Role::Window)
}

/// Whether the walk goes into a node. A focusable node is a control, never a container.
fn enters(node: &DialogNode) -> bool {
    is_container(node.role) && !node.focusable && !node.offscreen
}

/// Walks below `root` with the budget `DIALOG_CALLS`, `DIALOG_DEPTH` and `DIALOG_NODES`.
pub(super) fn walk<S: Source>(source: &mut S, root: &S::Handle) -> Result<Walk, Hung> {
    let mut out = Walk { nodes: Vec::new(), complete: true, calls: 0 };
    visit(source, root, 0, &mut out)?;
    Ok(out)
}

fn visit<S: Source>(source: &mut S, parent: &S::Handle, depth: usize, out: &mut Walk) -> Result<(), Hung> {
    if out.calls >= DIALOG_CALLS {
        out.complete = false;
        return Ok(());
    }
    out.calls += 1;
    for (mut node, handle) in source.children(parent)? {
        if out.nodes.len() >= DIALOG_NODES {
            out.complete = false;
            return Ok(());
        }
        node.depth = depth;
        let enter = enters(&node);
        out.nodes.push(node);
        if let (true, Some(handle)) = (enter, handle) {
            if depth + 1 >= DIALOG_DEPTH {
                out.complete = false;
            } else {
                visit(source, &handle, depth + 1, out)?;
            }
        }
    }
    Ok(())
}

/// Whether the MSAA fallback should replace the walk: only for the standard dialog class, and only
/// when UI Automation found no text at all (its Win32 proxy answering `Pane` for everything, as in
/// the elevated Save As).
pub(super) fn needs_msaa(class: &str, nodes: &[DialogNode]) -> bool {
    class == "#32770" && !nodes.iter().any(|n| n.role == Role::Text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree by index: `kids[i]` are the children of node `i`; node 0 is the root.
    struct Fake {
        kids: Vec<Vec<DialogNode>>,
        /// Which child lists have a node of their own to go into, by parent index.
        next: Vec<Vec<usize>>,
        calls: Vec<usize>,
        hang_on: Option<usize>,
    }

    impl Source for Fake {
        type Handle = usize;

        fn children(&mut self, parent: &usize) -> Result<Children<usize>, Hung> {
            self.calls.push(*parent);
            if self.hang_on == Some(*parent) {
                return Err(Hung);
            }
            Ok(self.kids[*parent]
                .iter()
                .cloned()
                .zip(self.next[*parent].iter().map(|&n| (n != 0).then_some(n)))
                .collect())
        }
    }

    fn node(name: &str, role: Role) -> DialogNode {
        DialogNode::new(name, role, 0)
    }

    fn fake(kids: Vec<Vec<DialogNode>>, next: Vec<Vec<usize>>) -> Fake {
        Fake { kids, next, calls: Vec::new(), hang_on: None }
    }

    fn names(walk: &Walk) -> Vec<(&str, usize)> {
        walk.nodes.iter().map(|n| (n.name.as_str(), n.depth)).collect()
    }

    #[test]
    fn a_container_is_followed_by_its_children_depth_first() {
        // root: Pane "p" (-> 1), Button "ok"; node 1: Text "hello", Text "again"
        let mut f = fake(
            vec![
                vec![node("p", Role::Pane), node("ok", Role::Button)],
                vec![node("hello", Role::Text), node("again", Role::Text)],
            ],
            vec![vec![1, 0], vec![0, 0]],
        );
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(names(&walk), [("p", 0), ("hello", 1), ("again", 1), ("ok", 0)]);
        assert!(walk.complete);
        assert_eq!(walk.calls, 2);
    }

    #[test]
    fn lists_trees_documents_toolbars_and_focusable_nodes_are_never_entered() {
        let mut focusable_pane = node("pane", Role::Pane);
        focusable_pane.focusable = true;
        let mut f = fake(
            vec![
                vec![
                    node("files", Role::List),
                    node("nav", Role::Tree),
                    node("doc", Role::Document),
                    node("bar", Role::ToolBar),
                    node("name", Role::Edit),
                    node("kind", Role::ComboBox),
                    node("menu", Role::Menu),
                    node("tabs", Role::Tab),
                    node("table", Role::Table),
                    focusable_pane,
                ],
                vec![node("never read", Role::Text)],
            ],
            // Every node would lead to node 1 if it were entered.
            vec![vec![1; 10], vec![0]],
        );
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(f.calls, [0]);
        assert_eq!(walk.nodes.len(), 10);
        assert!(walk.complete);
    }

    #[test]
    fn an_offscreen_container_is_not_entered_and_the_walk_is_still_complete() {
        let mut hidden = node("hidden", Role::Pane);
        hidden.offscreen = true;
        let mut f = fake(vec![vec![hidden], vec![node("x", Role::Text)]], vec![vec![1], vec![0]]);
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(f.calls, [0]);
        assert!(walk.complete);
    }

    #[test]
    fn a_node_without_a_handle_is_not_entered() {
        let mut f = fake(vec![vec![node("p", Role::Pane)]], vec![vec![0]]);
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(walk.calls, 1);
        assert!(walk.complete);
    }

    #[test]
    fn a_container_below_the_depth_limit_makes_the_walk_incomplete() {
        // A chain of panes, one per node: 0 -> 1 -> 2 -> ... deeper than DIALOG_DEPTH.
        let n = DIALOG_DEPTH + 2;
        let kids = (0..n).map(|i| vec![node(&format!("p{i}"), Role::Pane)]).collect();
        let next = (0..n).map(|i| vec![i + 1]).collect();
        let mut f = fake(kids, next);
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(walk.nodes.len(), DIALOG_DEPTH);
        assert_eq!(walk.nodes.last().unwrap().depth, DIALOG_DEPTH - 1);
        assert!(!walk.complete);
        assert_eq!(walk.calls, DIALOG_DEPTH);
    }

    #[test]
    fn the_call_budget_stops_the_walk_and_marks_it_incomplete() {
        // One root with more panes than the call budget, each holding one text.
        let panes = DIALOG_CALLS + 3;
        let mut kids = vec![(0..panes).map(|i| node(&format!("p{i}"), Role::Pane)).collect()];
        let mut next = vec![(1..=panes).collect()];
        for i in 0..panes {
            kids.push(vec![node(&format!("t{i}"), Role::Text)]);
            next.push(vec![0]);
        }
        let mut f = fake(kids, next);
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(walk.calls, DIALOG_CALLS);
        assert_eq!(f.calls.len(), DIALOG_CALLS);
        assert!(!walk.complete);
    }

    #[test]
    fn the_node_budget_stops_the_walk_and_marks_it_incomplete() {
        let many = (0..DIALOG_NODES + 5).map(|i| node(&format!("t{i}"), Role::Text)).collect();
        let mut f = fake(vec![many], vec![vec![0; DIALOG_NODES + 5]]);
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(walk.nodes.len(), DIALOG_NODES);
        assert!(!walk.complete);
    }

    #[test]
    fn exactly_the_node_budget_is_complete() {
        let many = (0..DIALOG_NODES).map(|i| node(&format!("t{i}"), Role::Text)).collect();
        let mut f = fake(vec![many], vec![vec![0; DIALOG_NODES]]);
        let walk = walk(&mut f, &0).unwrap();
        assert_eq!(walk.nodes.len(), DIALOG_NODES);
        assert!(walk.complete);
    }

    #[test]
    fn a_timeout_anywhere_ends_the_walk_as_hung() {
        let mut f = fake(
            vec![vec![node("p", Role::Pane)], vec![node("t", Role::Text)]],
            vec![vec![1], vec![0]],
        );
        f.hang_on = Some(1);
        assert_eq!(walk(&mut f, &0), Err(Hung));
    }

    #[test]
    fn an_empty_dialog_is_a_complete_walk_with_no_nodes() {
        let mut f = fake(vec![vec![]], vec![vec![]]);
        let walk = walk(&mut f, &0).unwrap();
        assert!(walk.nodes.is_empty());
        assert!(walk.complete);
        assert_eq!(walk.calls, 1);
    }

    #[test]
    fn msaa_replaces_the_walk_only_for_the_standard_class_without_any_text() {
        let panes = [node("a", Role::Pane), node("b", Role::Button)];
        let with_text = [node("a", Role::Pane), node("hello", Role::Text)];
        assert!(needs_msaa("#32770", &panes));
        assert!(needs_msaa("#32770", &[]));
        assert!(!needs_msaa("#32770", &with_text));
        assert!(!needs_msaa("NUIDialog", &panes));
        assert!(!needs_msaa("Shell_Dialog", &[]));
    }
}
