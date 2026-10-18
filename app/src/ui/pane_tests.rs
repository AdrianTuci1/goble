use super::*;
use super::pane::default_floating_rect;

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

fn close_to(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn bsp_regions_follow_the_split_tree_and_close_collapses_it() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let right = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    let down = space.split(right, SplitDir::Vertical, &mut next).unwrap();

    let regions = space.layout_regions();
    assert_eq!(regions.len(), 3, "one region per leaf");
    let region = |id: u64| {
        regions
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, rect)| *rect)
            .expect("the leaf has a region")
    };
    // [1 | (right / down)]: 1 takes the left half, the right half is halved.
    assert!(close_to(region(1).width(), 0.5));
    assert!(close_to(region(right).min_x(), 0.5));
    assert!(close_to(region(right).height(), 0.5));
    assert!(close_to(region(down).min_y(), 0.5));

    // Closing the lower leaf gives its sibling the whole right column.
    assert_eq!(space.close(down), Some(right));
    let regions = space.layout_regions();
    assert_eq!(regions.len(), 2);
    let right_rect = regions
        .iter()
        .find(|(pane, _)| *pane == right)
        .map(|(_, rect)| *rect)
        .unwrap();
    assert!(close_to(right_rect.min_x(), 0.5));
    assert!(close_to(right_rect.height(), 1.0));
}

#[test]
fn master_stack_gives_the_first_pane_sixty_percent_and_stacks_the_rest() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let second = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    let third = space.split(second, SplitDir::Horizontal, &mut next).unwrap();
    space.set_layout(LayoutKind::MasterStack);

    let regions = space.layout_regions();
    assert_eq!(regions.len(), 3);
    let rect = |id: u64| {
        regions
            .iter()
            .find(|(pane, _)| *pane == id)
            .map(|(_, rect)| *rect)
            .unwrap()
    };
    let master = rect(1);
    assert!(close_to(master.min_x(), 0.0));
    assert!(close_to(master.width(), 0.6));
    assert!(close_to(master.height(), 1.0));

    let top = rect(second);
    let bottom = rect(third);
    assert!(close_to(top.min_x(), 0.6));
    assert!(close_to(top.width(), 0.4));
    assert!(close_to(top.min_y(), 0.0));
    assert!(close_to(top.height(), 0.5));
    assert!(close_to(bottom.min_y(), 0.5));
    assert!(close_to(bottom.height(), 0.5));

    // Closing one stack pane leaves the other with the whole column.
    assert_eq!(space.close(third), Some(second));
    let regions = space.layout_regions();
    assert_eq!(regions.len(), 2);
    let remaining = regions
        .iter()
        .find(|(pane, _)| *pane == second)
        .map(|(_, rect)| *rect)
        .unwrap();
    assert!(close_to(remaining.min_x(), 0.6));
    assert!(close_to(remaining.height(), 1.0));
}

#[test]
fn switching_to_floating_gives_every_pane_a_rect_and_splits_and_closes_keep_it_current() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let second = space.split(1, SplitDir::Vertical, &mut next).unwrap();
    space.set_layout(LayoutKind::Floating);
    assert_eq!(space.layout, LayoutKind::Floating);
    assert_eq!(space.floating.len(), 2, "both leaves got a rect");
    assert!(space.floating_rect(1).is_some());
    assert!(space.floating_rect(second).is_some());

    // A split in the floating layout gets a rect of its own.
    let third = space.split(second, SplitDir::Horizontal, &mut next).unwrap();
    assert_eq!(space.floating.len(), 3);
    assert!(space.floating_rect(third).is_some());

    // Moving a pane stores the rect, and the stored rect is what the layout
    // reports.
    let moved = PaneRect::new(0.2, 0.15, 0.5, 0.4);
    space.set_floating_rect(third, moved);
    assert_eq!(space.floating_rect(third), Some(moved));
    let region = space
        .layout_regions()
        .into_iter()
        .find(|(id, _)| *id == third)
        .map(|(_, rect)| rect)
        .unwrap();
    assert!(close_to(region.min_x(), 0.2));
    assert!(close_to(region.width(), 0.5));

    // Closing removes the pane's rect and leaves the others where they were.
    assert_eq!(space.close(third), Some(second));
    assert_eq!(space.floating.len(), 2);
    assert_eq!(space.floating_rect(third), None);
    assert_eq!(space.floating_rect(1), Some(default_floating_rect(0)));
}

#[test]
fn floating_rects_are_clamped_into_the_area_and_raise_pane_moves_it_to_the_top() {
    let mut space = Space::new("S", chat_leaf(1));
    let mut next = 2;
    let second = space.split(1, SplitDir::Horizontal, &mut next).unwrap();
    space.set_layout(LayoutKind::Floating);

    // A rect dragged past the edge keeps its size inside the unit square.
    space.set_floating_rect(1, PaneRect::new(0.9, 0.9, 0.5, 0.5));
    let rect = space.floating_rect(1).unwrap();
    assert!(rect.x <= 0.5 + 1e-6);
    assert!(rect.y <= 0.5 + 1e-6);

    // The last floating entry draws on top; a click raises the pane it lands
    // on by moving it to the end of the z-order.
    assert_eq!(space.floating.last().map(|pane| pane.pane_id), Some(second));
    space.raise_pane(1);
    assert_eq!(space.floating.last().map(|pane| pane.pane_id), Some(1));
    assert_eq!(space.layout_regions().last().map(|(id, _)| *id), Some(1));
}

#[test]
fn a_floating_drag_moves_and_resizes_in_normalized_units() {
    let original = PaneRect::new(0.1, 0.1, 0.4, 0.4);
    let area = goble_ui::geometry::vec2f(1000.0, 500.0);
    let start = goble_ui::geometry::vec2f(100.0, 100.0);

    let moving = FloatingDrag {
        pane_id: 7,
        mode: FloatingDragMode::Move,
        start,
        original,
        area,
    };
    let moved = moving.rect_at(goble_ui::geometry::vec2f(200.0, 150.0));
    assert!(close_to(moved.x, 0.2));
    assert!(close_to(moved.y, 0.2));
    assert!(close_to(moved.w, original.w));

    let resizing = FloatingDrag {
        mode: FloatingDragMode::Resize,
        ..moving
    };
    let resized = resizing.rect_at(goble_ui::geometry::vec2f(300.0, 200.0));
    assert!(close_to(resized.x, original.x));
    assert!(close_to(resized.w, 0.6));
    assert!(close_to(resized.h, 0.6));
}

#[test]
fn the_layout_cycle_is_bsp_master_stack_floating() {
    assert_eq!(LayoutKind::Bsp.cycle(), LayoutKind::MasterStack);
    assert_eq!(LayoutKind::MasterStack.cycle(), LayoutKind::Floating);
    assert_eq!(LayoutKind::Floating.cycle(), LayoutKind::Bsp);
}
