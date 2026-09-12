    use super::*;

    /// A UiState with three named single-pane spaces and `active` selected.
    fn three_spaces(active: usize) -> UiState {
        let mut state = UiState::mock();
        state.spaces = vec![
            Space::new("A", Pane::Leaf { id: 1, kind: PaneKind::Chat }),
            Space::new("B", Pane::Leaf { id: 2, kind: PaneKind::Chat }),
            Space::new("C", Pane::Leaf { id: 3, kind: PaneKind::Chat }),
        ];
        state.active_space = active;
        state
    }

    fn names(state: &UiState) -> Vec<&str> {
        state.spaces.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn reorder_moves_space_to_target_position() {
        let mut state = three_spaces(0);
        state.reorder_space(0, 2);
        assert_eq!(names(&state), vec!["B", "C", "A"]);
        assert_eq!(state.active_space, 2, "active space A follows to index 2");
    }

    #[test]
    fn reorder_keeps_active_space_when_a_earlier_space_moves_past_it() {
        let mut state = three_spaces(1);
        state.reorder_space(0, 2);
        assert_eq!(names(&state), vec!["B", "C", "A"]);
        // Space B (active) slid down to index 0.
        assert_eq!(state.active_space, 0);
    }

    #[test]
    fn reorder_keeps_active_space_when_a_later_space_moves_before_it() {
        let mut state = three_spaces(1);
        state.reorder_space(2, 0);
        assert_eq!(names(&state), vec!["C", "A", "B"]);
        // Space B (active) slid up to index 2.
        assert_eq!(state.active_space, 2);
    }

    #[test]
    fn reorder_to_same_index_is_a_no_op() {
        let mut state = three_spaces(1);
        state.reorder_space(1, 1);
        assert_eq!(names(&state), vec!["A", "B", "C"]);
        assert_eq!(state.active_space, 1);
    }

    #[test]
    fn reorder_out_of_range_is_a_no_op() {
        let mut state = three_spaces(1);
        state.reorder_space(0, 9);
        assert_eq!(names(&state), vec!["A", "B", "C"]);
        state.reorder_space(9, 0);
        assert_eq!(names(&state), vec!["A", "B", "C"]);
    }
