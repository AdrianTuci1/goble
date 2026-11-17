//! The agent composer: the rich input bar and the command-proposal card.
//!
//! One module per surface: the input bar itself ([`composer`]) and the command
//! the harness proposed and is waiting on ([`proposal`]). The re-exports below
//! keep the module's public paths (`elements::chat_composer::ChatComposer`,
//! `elements::chat_composer::CommandProposalUi`) unchanged.

mod composer;
mod diff;
mod proposal;
#[cfg(test)]
mod tests;

pub use composer::{
    ChatComposer, COMPOSER_BAR_RESERVED_HEIGHT, COMPOSER_CARD_GUTTER, COMPOSER_CARD_INSET,
    COMPOSER_CARD_RADIUS, COMPOSER_EDITOR_MAX_HEIGHT, COMPOSER_EDITOR_MIN_HEIGHT,
    COMPOSER_FOOTER_GAP, COMPOSER_FOOTER_HEIGHT,
    COMPOSER_HINTS_ROW_HEIGHT, COMPOSER_INPUT_MIN_HEIGHT, COMPOSER_INPUT_RADIUS,
    COMPOSER_SEND_SIZE, CONTEXT_SEGMENT_HEIGHT,
};
/// The words the model control reads while no model is configured. The element
/// that draws the label owns them; the app re-exports this constant rather than
/// keeping a copy that could drift.
pub use composer::MODEL_NOT_CONFIGURED;
pub use diff::{DiffFileStat, DiffSummary};
pub use proposal::CommandProposalUi;
