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
/// The words the model control reads while no model is configured. The element
/// that draws the label owns them; the app re-exports this constant rather than
/// keeping a copy that could drift.
pub use composer::MODEL_NOT_CONFIGURED;
pub use proposal::CommandProposalUi;
