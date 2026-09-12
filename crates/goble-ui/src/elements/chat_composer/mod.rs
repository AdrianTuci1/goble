//! The agent composer: the rich input bar and the command-proposal card.
//!
//! One module per surface: the input bar itself ([`composer`]) and the command
//! the harness proposed and is waiting on ([`proposal`]). The re-exports below
//! keep the module's public paths (`elements::chat_composer::ChatComposer`,
//! `elements::chat_composer::CommandProposalUi`) unchanged.

mod composer;
mod proposal;
#[cfg(test)]
mod tests;

pub use composer::ChatComposer;
pub use proposal::CommandProposalUi;
