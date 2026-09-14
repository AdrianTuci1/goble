//! Shell layout: topbar, main content switcher, and the fixed-width
//! sidebar+main split.
//!
//! One module per surface of the shell: the topbar with its workspace strip and
//! "+ ▾" environment control ([`topbar`]), the tab-driven main content switcher
//! ([`content`]) and the sidebar+main split element with its drag handle
//! ([`layout`]). What they share — the toolbar height and the topbar's re-export
//! surface — is declared here.

mod content;
mod layout;
mod topbar;

#[cfg(test)]
mod tests;

pub use content::build_main;
pub use layout::SidebarLayout;
pub use topbar::{build_topbar, TOPBAR_HEIGHT};
