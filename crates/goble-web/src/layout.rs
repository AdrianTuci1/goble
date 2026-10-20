//! Tab and focus model for the terminal client.
//!
//! Pure data: the DOM layer owns canvases and sockets, this owns which tab is
//! visible and how focus moves when tabs are opened, closed, or lose their
//! worker.

/// One terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: u64,
    pub worker_id: String,
    pub title: String,
}

/// Ordered tabs with one active tab.
#[derive(Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<u64>,
    next_id: u64,
}

impl Tabs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a tab and focus it.
    pub fn open(&mut self, worker_id: impl Into<String>, title: impl Into<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab {
            id,
            worker_id: worker_id.into(),
            title: title.into(),
        });
        self.active = Some(id);
        id
    }

    /// Close `id`, returning the removed tab. Focus moves to the tab that took
    /// its slot, or the new last tab.
    pub fn close(&mut self, id: u64) -> Option<Tab> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        let removed = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = if self.tabs.is_empty() {
                None
            } else {
                Some(self.tabs[index.min(self.tabs.len() - 1)].id)
            };
        }
        Some(removed)
    }

    pub fn select(&mut self, id: u64) -> bool {
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active = Some(id);
            true
        } else {
            false
        }
    }

    /// Update a tab's label, e.g. from the shell's `OSC 2` title. Returns
    /// `true` when the title actually changed.
    pub fn set_title(&mut self, id: u64, title: impl Into<String>) -> bool {
        let title = title.into();
        match self.tabs.iter_mut().find(|tab| tab.id == id) {
            Some(tab) if tab.title != title => {
                tab.title = title;
                true
            }
            _ => false,
        }
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        match self.tabs.get(index) {
            Some(tab) => {
                self.active = Some(tab.id);
                true
            }
            None => false,
        }
    }

    /// Focus the next tab, wrapping.
    pub fn next_tab(&mut self) -> Option<u64> {
        self.cycle(1)
    }

    /// Focus the previous tab, wrapping.
    pub fn previous_tab(&mut self) -> Option<u64> {
        self.cycle(-1)
    }

    fn cycle(&mut self, step: isize) -> Option<u64> {
        if self.tabs.is_empty() {
            return None;
        }
        let current = self.active_index().unwrap_or(0) as isize;
        let len = self.tabs.len() as isize;
        let next = (current + step).rem_euclid(len) as usize;
        self.active = Some(self.tabs[next].id);
        Some(self.tabs[next].id)
    }

    #[must_use]
    pub fn active_index(&self) -> Option<usize> {
        let id = self.active?;
        self.tabs.iter().position(|tab| tab.id == id)
    }

    #[must_use]
    pub fn active(&self) -> Option<&Tab> {
        let index = self.active_index()?;
        self.tabs.get(index)
    }

    #[must_use]
    pub fn active_id(&self) -> Option<u64> {
        self.active
    }

    #[must_use]
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    #[must_use]
    pub fn get(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Tabs attached to `worker_id`.
    #[must_use]
    pub fn for_worker(&self, worker_id: &str) -> Vec<u64> {
        self.tabs
            .iter()
            .filter(|tab| tab.worker_id == worker_id)
            .map(|tab| tab.id)
            .collect()
    }

    /// Close every tab whose worker is no longer connected. Returns the closed
    /// tabs in order. Focus is preserved as long as the active tab survives.
    pub fn retain_workers(&mut self, connected: &[String]) -> Vec<Tab> {
        let doomed: Vec<u64> = self
            .tabs
            .iter()
            .filter(|tab| !connected.iter().any(|id| id == &tab.worker_id))
            .map(|tab| tab.id)
            .collect();
        doomed.into_iter().filter_map(|id| self.close(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs() -> Tabs {
        let mut tabs = Tabs::new();
        tabs.open("w1", "one");
        tabs.open("w1", "two");
        tabs.open("w2", "three");
        tabs
    }

    #[test]
    fn open_appends_and_focuses() {
        let mut tabs = Tabs::new();
        assert_eq!(tabs.len(), 0);
        let first = tabs.open("w1", "one");
        let second = tabs.open("w1", "two");
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.active_id(), Some(second));
        assert_ne!(first, second);
        assert_eq!(tabs.tabs()[0].title, "one");
    }

    #[test]
    fn close_active_selects_the_slot_to_the_right() {
        let mut tabs = tabs();
        let ids: Vec<u64> = tabs.tabs().iter().map(|tab| tab.id).collect();
        tabs.select(ids[1]);
        let removed = tabs.close(ids[1]).unwrap();
        assert_eq!(removed.title, "two");
        assert_eq!(tabs.active_id(), Some(ids[2]));
        assert_eq!(tabs.len(), 2);
    }

    #[test]
    fn close_last_tab_selects_the_previous() {
        let mut tabs = tabs();
        let last = tabs.tabs().last().unwrap().id;
        tabs.close(last);
        assert_eq!(tabs.active_id(), Some(tabs.tabs().last().unwrap().id));
    }

    #[test]
    fn close_inactive_tab_keeps_focus() {
        let mut tabs = tabs();
        let ids: Vec<u64> = tabs.tabs().iter().map(|tab| tab.id).collect();
        tabs.select(ids[2]);
        tabs.close(ids[0]);
        assert_eq!(tabs.active_id(), Some(ids[2]));
    }

    #[test]
    fn closing_the_only_tab_clears_focus() {
        let mut tabs = Tabs::new();
        let id = tabs.open("w1", "one");
        assert!(tabs.close(id).is_some());
        assert!(tabs.is_empty());
        assert_eq!(tabs.active_id(), None);
        assert!(tabs.close(id).is_none());
    }

    #[test]
    fn next_and_previous_wrap() {
        let mut tabs = tabs();
        let ids: Vec<u64> = tabs.tabs().iter().map(|tab| tab.id).collect();
        tabs.select(ids[0]);
        assert_eq!(tabs.next_tab(), Some(ids[1]));
        assert_eq!(tabs.next_tab(), Some(ids[2]));
        assert_eq!(tabs.next_tab(), Some(ids[0]));
        assert_eq!(tabs.previous_tab(), Some(ids[2]));
    }

    #[test]
    fn cycling_empty_is_none() {
        let mut tabs = Tabs::new();
        assert_eq!(tabs.next_tab(), None);
        assert_eq!(tabs.previous_tab(), None);
    }

    #[test]
    fn select_index_bounds() {
        let mut tabs = tabs();
        assert!(tabs.select_index(1));
        assert_eq!(tabs.active().unwrap().title, "two");
        assert!(!tabs.select_index(9));
        assert_eq!(tabs.active().unwrap().title, "two");
    }

    #[test]
    fn select_unknown_id_is_rejected() {
        let mut tabs = tabs();
        assert!(!tabs.select(999));
    }

    #[test]
    fn worker_disappearing_closes_its_tabs() {
        let mut tabs = tabs();
        let ids: Vec<u64> = tabs.tabs().iter().map(|tab| tab.id).collect();
        tabs.select(ids[2]);
        let closed = tabs.retain_workers(&["w2".to_string()]);
        assert_eq!(closed.len(), 2);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active_id(), Some(ids[2]));
    }

    #[test]
    fn worker_disappearing_with_active_tab_restores_an_index() {
        let mut tabs = tabs();
        let ids: Vec<u64> = tabs.tabs().iter().map(|tab| tab.id).collect();
        tabs.select(ids[1]);
        let closed = tabs.retain_workers(&["w2".to_string()]);
        assert_eq!(closed.len(), 2);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active_id(), Some(ids[2]));
    }

    #[test]
    fn retain_all_workers_is_a_noop() {
        let mut tabs = tabs();
        let before = tabs.active_id();
        let closed = tabs.retain_workers(&["w1".to_string(), "w2".to_string()]);
        assert!(closed.is_empty());
        assert_eq!(tabs.len(), 3);
        assert_eq!(tabs.active_id(), before);
    }

    #[test]
    fn for_worker_filters() {
        let tabs = tabs();
        assert_eq!(tabs.for_worker("w1").len(), 2);
        assert_eq!(tabs.for_worker("w2").len(), 1);
        assert!(tabs.for_worker("missing").is_empty());
    }

    #[test]
    fn get_by_id() {
        let tabs = tabs();
        let id = tabs.tabs()[0].id;
        assert_eq!(tabs.get(id).unwrap().title, "one");
        assert!(tabs.get(999).is_none());
    }

    #[test]
    fn set_title_reports_changes() {
        let mut tabs = tabs();
        let id = tabs.tabs()[0].id;
        assert!(tabs.set_title(id, "shell"));
        assert_eq!(tabs.get(id).unwrap().title, "shell");
        assert!(!tabs.set_title(id, "shell"));
        assert!(!tabs.set_title(999, "missing"));
    }
}
