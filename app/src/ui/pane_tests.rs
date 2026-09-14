use super::*;

fn chat_leaf(id: u64) -> Pane {
    Pane::Leaf { id, kind: PaneKind::Chat }
}

#[test]
fn split_adds_second_pane_and_returns_its_id() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let new_id = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    assert_eq!(new_id, 3); // split node = 2, new leaf = 3
    assert_eq!(space.root.id(), 2);
    assert_eq!(space.root.max_id(), 3);
    assert_eq!(space.root.first_leaf_id(), 1);
}

#[test]
fn close_replaces_split_with_sibling() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    space.split(1, SplitDir::Vertical, &mut next).unwrap();
    // Close the new leaf (3); the split collapses to the surviving leaf (1).
    let focus = space.close(3).unwrap();
    assert_eq!(focus, 1);
    assert!(matches!(space.root, Pane::Leaf { id: 1, .. }));
}

#[test]
fn set_ratio_updates_the_split_node() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    let split_id = space.root.id();
    assert!(space.set_ratio(split_id, 0.25));
    match &space.root {
        Pane::Split { ratio, .. } => assert!((*ratio - 0.25).abs() < 1e-6),
        _ => panic!("expected split"),
    }
}

#[test]
fn set_ratio_clamps_to_the_ratio_floor() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    let split_id = space.root.id();
    // Out-of-range ratios are clamped into [0.05, 0.95] (the coarse ratio
    // floor); the per-pixel minimum is enforced by the split widget, which
    // knows the actual size.
    assert!(space.set_ratio(split_id, 0.0));
    match &space.root {
        Pane::Split { ratio, .. } => assert!((*ratio - 0.05).abs() < 1e-6),
        _ => panic!("expected split"),
    }
    assert!(space.set_ratio(split_id, 1.0));
    match &space.root {
        Pane::Split { ratio, .. } => assert!((*ratio - 0.95).abs() < 1e-6),
        _ => panic!("expected split"),
    }
}

#[test]
fn split_creates_two_distinct_chat_leaves() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let new_id = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    // The two leaves have distinct ids, so each can host an independent
    // session (per-pane conversation transcript) in app state.
    assert_ne!(space.root.first_leaf_id(), new_id);
    match &space.root {
        Pane::Split { first, second, .. } => {
            assert!(matches!(**first, Pane::Leaf { kind: PaneKind::Chat, .. }));
            assert!(matches!(**second, Pane::Leaf { kind: PaneKind::Chat, .. }));
        }
        _ => panic!("expected a split"),
    }
}

#[test]
fn navigate_right_moves_between_horizontal_leaves() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let right = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    assert_eq!(space.navigate(1, NavDir::Right), Some(right));
    assert_eq!(space.navigate(right, NavDir::Left), Some(1));
}

#[test]
fn navigate_down_moves_between_vertical_leaves() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let down = space.split(1, SplitDir::Vertical, &mut next).unwrap();
    assert_eq!(space.navigate(1, NavDir::Down), Some(down));
    assert_eq!(space.navigate(down, NavDir::Up), Some(1));
}

#[test]
fn navigate_edge_returns_none_for_out_of_bounds() {
    let space = Space::new("S", chat_leaf(1));
    // A single pane has no neighbor in any direction.
    assert_eq!(space.navigate(1, NavDir::Left), None);
    assert_eq!(space.navigate(1, NavDir::Right), None);
    assert_eq!(space.navigate(1, NavDir::Up), None);
    assert_eq!(space.navigate(1, NavDir::Down), None);
}

#[test]
fn nested_split_navigates_to_nearest_sibling() {
    // Space 1 split right -> [A | B]. Split B downward: [A | (B / C)].
    // From C, Up lands on B; from B, Down lands on C; from A, Right lands
    // on the nearest leaf to the right (B).
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let b = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    let c = space.split(b, SplitDir::Vertical, &mut next).unwrap();
    assert_eq!(space.navigate(c, NavDir::Up), Some(b));
    assert_eq!(space.navigate(b, NavDir::Down), Some(c));
    assert_eq!(space.navigate(1, NavDir::Right), Some(b));
    // From C, left goes to A (the only leaf to the left).
    assert_eq!(space.navigate(c, NavDir::Left), Some(1));
}
