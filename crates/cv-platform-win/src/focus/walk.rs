//! The bounded walk of a dialog's tree (ADR 0016, 0018) and of a whole window (ADR 0019), as pure
//! logic. A `Source` fetches one
//! level of children (UI Automation or MSAA, in dialog.rs); `walk` decides what to enter and when
//! to stop. Tested here with a fake tree, because the real ones need a dialog on screen.

use std::time::Instant;

use cv_core::{
    DialogNode, Role,
    focus::{
        DIALOG_CALLS, DIALOG_DEPTH, DIALOG_NODES, WINDOW_CALLS, WINDOW_DEPTH, WINDOW_NODES,
    },
};

/// A call into the dialog's program timed out; the walk ends and the window counts as hung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Hung;

/// The children of one node, each with a handle when it could have children of its own. A node
/// that is itself a dialog (a property page, a nested prompt) has no handle: it is kept but not
/// entered (ADR 0018).
pub(super) type Children<H> = Vec<(DialogNode, Option<H>)>;

/// One level of a tree.
pub(super) trait Source {
    /// What `children` needs to fetch the next level below a node.
    type Handle;

    /// The children of `parent`, in tree order, each with a handle when it could have children of
    /// its own. The node's `depth` is set by the walk. A refused or failed call is an empty list;
    /// a timeout is `Err(Hung)`.
    fn children(&mut self, parent: &Self::Handle) -> Result<Children<Self::Handle>, Hung>;

    /// True once the source stopped partway through a level because the time ran out (the
    /// children it returned are the ones it got to). The walk then ends as incomplete.
    fn out_of_time(&self) -> bool {
        false
    }
}

/// What bounds a walk and what it enters: the dialog walk (ADR 0016, the default) or the window
/// walk (ADR 0019, [`Limits::window`]).
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    /// `DIALOG_MS` after the dialog read started: no further call is made past it and the walk is
    /// incomplete (ADR 0018). `None` for no time limit.
    pub deadline: Option<Instant>,
    /// Stop after the first level when it holds only `Pane` nodes (a `#32770` whose UI Automation
    /// proxy answers `Pane` for everything): the rest of this source's walk would find nothing, and
    /// MSAA takes over (ADR 0017 T2).
    pub panes_stop: bool,
    /// The most `children` calls.
    pub calls: usize,
    /// How deep the walk goes below the root.
    pub depth: usize,
    /// The most nodes collected.
    pub nodes: usize,
    /// A window read: the controls of a window are the point, so a focusable container is entered
    /// and toolbars, menu bars, status bars and tab lists are too (ADR 0019).
    pub window: bool,
}

impl Default for Limits {
    /// The dialog walk: ADR 0016's budgets, no time limit.
    fn default() -> Self {
        Self {
            deadline: None,
            panes_stop: false,
            calls: DIALOG_CALLS,
            depth: DIALOG_DEPTH,
            nodes: DIALOG_NODES,
            window: false,
        }
    }
}

impl Limits {
    /// The window walk: ADR 0019's budgets and enter rule, stopping at `deadline` (`WINDOW_MS`
    /// after the read started).
    pub fn window(deadline: Instant, panes_stop: bool) -> Self {
        Self {
            deadline: Some(deadline),
            panes_stop,
            calls: WINDOW_CALLS,
            depth: WINDOW_DEPTH,
            nodes: WINDOW_NODES,
            window: true,
        }
    }
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
    /// The first level held only panes and `Limits::panes_stop` ended the walk there.
    pub panes_only: bool,
}

/// A role the walk goes into: containers with no meaning of their own. Lists, trees, tables,
/// documents, edits, combo boxes, toolbars, menus and tabs are not in this list, so a file view
/// is never walked.
pub(super) fn is_container(role: Role) -> bool {
    matches!(role, Role::Pane | Role::Group | Role::Custom | Role::Unknown | Role::Window)
}

/// A role the window walk goes into (ADR 0019): the dialog walk's containers, and the bars and tab
/// lists whose children are controls. Lists, trees, tables, documents, text boxes, combo boxes and
/// menus are not in this list, so a file view or a page is never walked.
pub(super) fn is_window_container(role: Role) -> bool {
    is_container(role)
        || matches!(role, Role::ToolBar | Role::MenuBar | Role::StatusBar | Role::Tab | Role::AppBar)
}

/// Whether the walk goes into a node. In a dialog a focusable node is a control, never a
/// container; in a window it may be both. An offscreen node is never entered.
fn enters(node: &DialogNode, window: bool) -> bool {
    if node.offscreen {
        return false;
    }
    if window { is_window_container(node.role) } else { is_container(node.role) && !node.focusable }
}

/// Walks below `root` with the call, depth and node budgets and the time and early-stop rules in
/// `limits`.
pub(super) fn walk<S: Source>(source: &mut S, root: &S::Handle, limits: &Limits) -> Result<Walk, Hung> {
    let mut out = Walk { nodes: Vec::new(), complete: true, calls: 0, panes_only: false };
    visit(source, root, 0, limits, &mut out)?;
    Ok(out)
}

fn visit<S: Source>(
    source: &mut S,
    parent: &S::Handle,
    depth: usize,
    limits: &Limits,
    out: &mut Walk,
) -> Result<(), Hung> {
    if out.calls >= limits.calls || limits.deadline.is_some_and(|d| Instant::now() >= d) {
        out.complete = false;
        return Ok(());
    }
    out.calls += 1;
    let children = source.children(parent)?;
    if source.out_of_time() {
        out.complete = false;
    }
    if depth == 0
        && limits.panes_stop
        && !children.is_empty()
        && children.iter().all(|(node, _)| node.role == Role::Pane)
    {
        // Kept as nodes (they hold no text), so the caller sees a walk without any.
        for (mut node, _) in children {
            node.depth = depth;
            out.nodes.push(node);
        }
        out.panes_only = true;
        return Ok(());
    }
    for (mut node, handle) in children {
        // The title is already said, so a window read leaves the title bar out, and with it its
        // buttons (it is not entered either) (ADR 0019).
        if limits.window && node.role == Role::TitleBar {
            continue;
        }
        if out.nodes.len() >= limits.nodes {
            out.complete = false;
            return Ok(());
        }
        node.depth = depth;
        let enter = enters(&node, limits.window);
        out.nodes.push(node);
        if let (true, Some(handle)) = (enter, handle) {
            if depth + 1 >= limits.depth {
                out.complete = false;
            } else {
                visit(source, &handle, depth + 1, limits, out)?;
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
        /// Reported by `out_of_time`.
        late: bool,
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

        fn out_of_time(&self) -> bool {
            self.late
        }
    }

    fn node(name: &str, role: Role) -> DialogNode {
        DialogNode::new(name, role, 0)
    }

    fn fake(kids: Vec<Vec<DialogNode>>, next: Vec<Vec<usize>>) -> Fake {
        Fake { kids, next, calls: Vec::new(), hang_on: None, late: false }
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
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
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
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
        assert_eq!(f.calls, [0]);
        assert_eq!(walk.nodes.len(), 10);
        assert!(walk.complete);
    }

    #[test]
    fn an_offscreen_container_is_not_entered_and_the_walk_is_still_complete() {
        let mut hidden = node("hidden", Role::Pane);
        hidden.offscreen = true;
        let mut f = fake(vec![vec![hidden], vec![node("x", Role::Text)]], vec![vec![1], vec![0]]);
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
        assert_eq!(f.calls, [0]);
        assert!(walk.complete);
    }

    #[test]
    fn a_node_without_a_handle_is_not_entered() {
        let mut f = fake(vec![vec![node("p", Role::Pane)]], vec![vec![0]]);
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
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
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
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
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
        assert_eq!(walk.calls, DIALOG_CALLS);
        assert_eq!(f.calls.len(), DIALOG_CALLS);
        assert!(!walk.complete);
    }

    #[test]
    fn the_node_budget_stops_the_walk_and_marks_it_incomplete() {
        let many = (0..DIALOG_NODES + 5).map(|i| node(&format!("t{i}"), Role::Text)).collect();
        let mut f = fake(vec![many], vec![vec![0; DIALOG_NODES + 5]]);
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
        assert_eq!(walk.nodes.len(), DIALOG_NODES);
        assert!(!walk.complete);
    }

    #[test]
    fn exactly_the_node_budget_is_complete() {
        let many = (0..DIALOG_NODES).map(|i| node(&format!("t{i}"), Role::Text)).collect();
        let mut f = fake(vec![many], vec![vec![0; DIALOG_NODES]]);
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
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
        assert_eq!(walk(&mut f, &0, &Limits::default()), Err(Hung));
    }

    #[test]
    fn an_empty_dialog_is_a_complete_walk_with_no_nodes() {
        let mut f = fake(vec![vec![]], vec![vec![]]);
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
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

    #[test]
    fn a_walk_past_its_deadline_makes_no_call_and_is_incomplete() {
        let mut f = fake(vec![vec![node("t", Role::Text)]], vec![vec![0]]);
        let limits = Limits { deadline: Some(Instant::now()), ..Limits::default() };
        let walk = walk(&mut f, &0, &limits).unwrap();
        assert!(f.calls.is_empty());
        assert!(walk.nodes.is_empty());
        assert!(!walk.complete);
    }

    #[test]
    fn a_walk_with_time_left_is_not_cut() {
        let mut f = fake(vec![vec![node("t", Role::Text)]], vec![vec![0]]);
        let limits = Limits {
            deadline: Some(Instant::now() + std::time::Duration::from_secs(3600)),
            ..Limits::default()
        };
        let walk = walk(&mut f, &0, &limits).unwrap();
        assert!(walk.complete);
        assert_eq!(walk.nodes.len(), 1);
    }

    #[test]
    fn a_source_that_ran_out_of_time_makes_the_walk_incomplete_and_the_dialog_says_nothing() {
        let mut f = fake(vec![vec![node("t", Role::Text)]], vec![vec![0]]);
        f.late = true;
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
        assert!(!walk.complete);
        assert!(cv_core::focus::dialog_text(&walk.nodes, walk.complete, "").is_empty());
    }

    #[test]
    fn a_first_level_of_only_panes_ends_the_walk_when_asked_to() {
        // The elevated Save As: UI Automation's Win32 proxy answers Pane for everything.
        let mut f = fake(
            vec![vec![node("a", Role::Pane), node("b", Role::Pane)], vec![node("deeper", Role::Text)]],
            vec![vec![1, 0], vec![0]],
        );
        let limits = Limits { panes_stop: true, ..Limits::default() };
        let walk = walk(&mut f, &0, &limits).unwrap();
        assert_eq!(f.calls, [0]);
        assert!(walk.panes_only);
        assert!(needs_msaa("#32770", &walk.nodes));
    }

    #[test]
    fn the_panes_rule_does_not_apply_to_a_level_with_other_nodes_or_none_or_when_not_asked() {
        let mixed = || {
            fake(
                vec![vec![node("a", Role::Pane), node("ok", Role::Button)], vec![node("t", Role::Text)]],
                vec![vec![1, 0], vec![0]],
            )
        };
        let asked = Limits { panes_stop: true, ..Limits::default() };
        let mut f = mixed();
        let w = walk(&mut f, &0, &asked).unwrap();
        assert!(!w.panes_only);
        assert_eq!(f.calls, [0, 1]);

        let mut f = fake(vec![vec![]], vec![vec![]]);
        assert!(!walk(&mut f, &0, &asked).unwrap().panes_only);

        let mut f = fake(vec![vec![node("a", Role::Pane)], vec![node("t", Role::Text)]], vec![vec![1], vec![0]]);
        let w = walk(&mut f, &0, &Limits::default()).unwrap();
        assert!(!w.panes_only);
        assert_eq!(f.calls, [0, 1]);
    }

    #[test]
    fn a_nested_dialog_node_without_a_handle_is_kept_and_its_text_not_walked() {
        // A property page: the sheet's walk keeps the page's node but gets no handle to enter.
        let mut f = fake(
            vec![vec![node("General", Role::Pane), node("ok", Role::Button)], vec![node("page text", Role::Text)]],
            vec![vec![0, 0], vec![0]],
        );
        let walk = walk(&mut f, &0, &Limits::default()).unwrap();
        assert_eq!(f.calls, [0]);
        assert_eq!(names(&walk), [("General", 0), ("ok", 0)]);
        assert!(walk.complete);
    }

    fn window_limits() -> Limits {
        Limits::window(Instant::now() + std::time::Duration::from_secs(3600), false)
    }

    #[test]
    fn a_window_walk_enters_bars_tab_lists_and_focusable_containers() {
        let mut focusable_group = node("g", Role::Group);
        focusable_group.focusable = true;
        let entered = vec![
            node("tools", Role::ToolBar),
            node("menus", Role::MenuBar),
            node("status", Role::StatusBar),
            node("tabs", Role::Tab),
            node("apps", Role::AppBar),
            focusable_group,
        ];
        let n = entered.len();
        let mut kids = vec![entered];
        let mut next = vec![(1..=n).collect::<Vec<_>>()];
        for i in 0..n {
            kids.push(vec![node(&format!("inside{i}"), Role::Button)]);
            next.push(vec![0]);
        }
        let mut f = fake(kids, next);
        let walk = walk(&mut f, &0, &window_limits()).unwrap();
        assert_eq!(f.calls, (0..=n).collect::<Vec<_>>());
        assert_eq!(walk.nodes.len(), 2 * n);
        assert!(walk.complete);
    }

    #[test]
    fn the_dialog_walk_enters_neither_those_bars_nor_a_focusable_container() {
        let mut focusable_group = node("g", Role::Group);
        focusable_group.focusable = true;
        let mut f = fake(
            vec![
                vec![node("tools", Role::ToolBar), node("tabs", Role::Tab), focusable_group],
                vec![node("t", Role::Text)],
            ],
            vec![vec![1, 1, 1], vec![0]],
        );
        walk(&mut f, &0, &Limits::default()).unwrap();
        assert_eq!(f.calls, [0]);
    }

    #[test]
    fn a_window_walk_never_enters_lists_trees_tables_documents_text_boxes_menus_or_offscreen() {
        let mut hidden = node("hidden", Role::Group);
        hidden.offscreen = true;
        let mut f = fake(
            vec![
                vec![
                    node("files", Role::List),
                    node("nav", Role::Tree),
                    node("grid", Role::Table),
                    node("page", Role::Document),
                    node("name", Role::Edit),
                    node("kind", Role::ComboBox),
                    node("menu", Role::Menu),
                    hidden,
                ],
                vec![node("never read", Role::Text)],
            ],
            vec![vec![1; 8], vec![0]],
        );
        let walk = walk(&mut f, &0, &window_limits()).unwrap();
        assert_eq!(f.calls, [0]);
        assert_eq!(walk.nodes.len(), 8);
        assert!(walk.complete);
    }

    #[test]
    fn a_window_walk_does_not_enter_a_nested_dialog_without_a_handle() {
        let mut f = fake(
            vec![vec![node("General", Role::Pane)], vec![node("page text", Role::Text)]],
            vec![vec![0], vec![0]],
        );
        let walk = walk(&mut f, &0, &window_limits()).unwrap();
        assert_eq!(f.calls, [0]);
        assert!(walk.complete);
    }

    #[test]
    fn a_window_walk_has_its_own_budgets() {
        // Deeper than the dialog walk goes, within the window walk's depth.
        let n = DIALOG_DEPTH + 2;
        assert!(n < WINDOW_DEPTH);
        let mut kids: Vec<Vec<DialogNode>> =
            (0..n).map(|i| vec![node(&format!("p{i}"), Role::Pane)]).collect();
        let mut next: Vec<Vec<usize>> = (0..n).map(|i| vec![i + 1]).collect();
        // The last pane holds a leaf.
        kids.push(vec![node("end", Role::Text)]);
        next.push(vec![0]);
        let w = walk(&mut fake(kids, next), &0, &window_limits()).unwrap();
        assert_eq!(w.nodes.len(), n + 1);
        assert!(w.complete);

        // Past the window walk's depth.
        let n = WINDOW_DEPTH + 2;
        let kids = (0..n).map(|i| vec![node(&format!("p{i}"), Role::Pane)]).collect();
        let next = (0..n).map(|i| vec![i + 1]).collect();
        let w = walk(&mut fake(kids, next), &0, &window_limits()).unwrap();
        assert_eq!(w.nodes.len(), WINDOW_DEPTH);
        assert!(!w.complete);

        // More nodes than a dialog keeps, up to the window's limit.
        let count = DIALOG_NODES + 20;
        assert!(count <= WINDOW_NODES);
        let many = (0..count).map(|i| node(&format!("t{i}"), Role::Text)).collect();
        let w = walk(&mut fake(vec![many], vec![vec![0; count]]), &0, &window_limits()).unwrap();
        assert_eq!(w.nodes.len(), count);
        assert!(w.complete);
        let many = (0..WINDOW_NODES + 5).map(|i| node(&format!("t{i}"), Role::Text)).collect();
        let w = walk(&mut fake(vec![many], vec![vec![0; WINDOW_NODES + 5]]), &0, &window_limits())
            .unwrap();
        assert_eq!(w.nodes.len(), WINDOW_NODES);
        assert!(!w.complete);

        // The call budget.
        let panes = WINDOW_CALLS + 3;
        let mut kids = vec![(0..panes).map(|i| node(&format!("p{i}"), Role::Pane)).collect()];
        let mut next = vec![(1..=panes).collect()];
        for i in 0..panes {
            kids.push(vec![node(&format!("t{i}"), Role::Text)]);
            next.push(vec![0]);
        }
        let w = walk(&mut fake(kids, next), &0, &window_limits()).unwrap();
        assert_eq!(w.calls, WINDOW_CALLS);
        assert!(!w.complete);
    }

    #[test]
    fn a_window_walk_leaves_the_title_bar_out_but_a_dialog_walk_does_not() {
        let kids = || {
            fake(
                vec![
                    vec![node("Untitled - Notepad", Role::TitleBar), node("File", Role::MenuItem)],
                    vec![node("Close", Role::Button)],
                ],
                vec![vec![1, 0], vec![0]],
            )
        };
        let mut f = kids();
        let w = walk(&mut f, &0, &window_limits()).unwrap();
        assert_eq!(names(&w), [("File", 0)]);
        assert_eq!(f.calls, [0]);
        assert!(w.complete);

        let w = walk(&mut kids(), &0, &Limits::default()).unwrap();
        assert_eq!(names(&w), [("Untitled - Notepad", 0), ("File", 0)]);
    }
}
