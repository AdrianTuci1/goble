//! The subject the agent gives a conversation, derived from what was said in it.
//!
//! A conversation is created with the placeholder [`NEW_CONVERSATION_TITLE`] and
//! nothing else names it, so the agent names it from the conversation itself
//! once it has answered in it. The derivation is a rule-based summariser over
//! the exchange — deterministic, no model consulted — so a workspace with no
//! provider key still gets named conversations, and no sidebar row waits on a
//! model round trip to read its own subject.

use super::NEW_CONVERSATION_TITLE;

/// What a conversation that has only been greeted is called.
const GREETING_SUBJECT: &str = "Initial user onboarding";

/// The most words a derived subject carries.
const MAX_WORDS: usize = 6;

/// The most characters a derived subject carries before the ellipsis.
const MAX_CHARS: usize = 48;

/// Words that *ask* for something rather than name it: dropped from the front
/// of a prompt so `Can you add a title to the sidebar` names the work, and a
/// prompt that is nothing but them (`thanks!`) names nothing at all. The words
/// after the first content word are the user's own and are kept as written.
const OPENERS: &[&str] = &[
    "a", "about", "all", "also", "an", "and", "can", "could", "do", "does", "for", "from", "great",
    "help", "hey", "hi", "hello", "how", "i", "is", "just", "let", "me", "my", "need", "next",
    "no", "now", "ok", "okay", "our", "please", "pls", "so", "thank", "thanks", "that", "the",
    "then", "this", "to", "us", "want", "we", "what", "when", "where", "which", "who", "why",
    "will", "with", "would", "yes", "you", "your",
];

/// A message made only of these is a greeting, not a request.
const GREETINGS: &[&str] = &[
    "again",
    "afternoon",
    "all",
    "bonjour",
    "buna",
    "bună",
    "ciao",
    "day",
    "evening",
    "everyone",
    "good",
    "hallo",
    "hei",
    "hello",
    "helo",
    "hey",
    "hi",
    "hola",
    "morning",
    "salut",
    "there",
    "yo",
];

/// The subject the agent gives the conversation whose stored title is
/// `stored_title`, or `None` when the conversation keeps the title it has.
///
/// `messages` are the conversation's rows in the order they were written, each
/// a `(role, content)` pair; an exchange is one prompt and the reply the agent
/// wrote to it. The newest exchange that names something is the subject, so a
/// conversation a later turn has moved to another subject is retitled. Only the
/// agent's own words are replaced: the placeholder is the agent's to fill in,
/// and so is a subject the agent derived earlier, but a subject the user typed
/// is never taken back.
pub fn agent_subject(stored_title: &str, messages: &[(&str, &str)]) -> Option<String> {
    let derived: Vec<Option<String>> = exchanges(messages)
        .into_iter()
        .map(|(question, reply)| derive(question, reply))
        .collect();
    // A follow-up that only thanks or agrees names nothing, so it leaves the
    // conversation under the subject of the last exchange that did.
    let subject = derived.iter().rev().flatten().next()?.clone();
    if subject == stored_title || subject == NEW_CONVERSATION_TITLE {
        return None;
    }
    // A subject the user typed is not one any exchange of this conversation
    // derives, so it stands.
    let user_named = stored_title != NEW_CONVERSATION_TITLE
        && !derived.iter().any(|s| s.as_deref() == Some(stored_title));
    (!user_named).then_some(subject)
}

/// Pair each reply the agent wrote with the prompt it answered, in order. A
/// tool result is the agent's own doing rather than an answer to a question, so
/// it pairs with nothing.
fn exchanges<'a>(messages: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut pairs = Vec::new();
    let mut asked: Option<&'a str> = None;
    for &(role, content) in messages {
        match role {
            "user" => asked = Some(content),
            "assistant" => {
                if let Some(question) = asked.take() {
                    pairs.push((question, content));
                }
            }
            _ => {}
        }
    }
    pairs
}

/// The subject one exchange names, or `None` when it names nothing at all.
fn derive(question: &str, reply: &str) -> Option<String> {
    let asked = words(question);
    if asked.is_empty() {
        // Nothing the user typed can name this — an attachment-only prompt, a
        // bare "?" — so the agent's own opening words are all there is.
        let answered = words(first_sentence(reply));
        return (!answered.is_empty()).then(|| shorten(&answered));
    }
    if is_greeting(&asked) {
        return Some(GREETING_SUBJECT.to_string());
    }
    let named = strip_openers(&asked);
    // A message made only of asking words — `thanks!`, `ok` — responds to the
    // conversation instead of moving it, and names nothing.
    if named.is_empty() {
        return None;
    }
    Some(shorten(named))
}

/// The words of `text`, each trimmed of the punctuation around it: the word a
/// user asked with is the word, not the mark that ends the question.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// `text` up to its first sentence break: what a reply opens with is what it
/// answered, and a whole reply is far more than a subject needs.
fn first_sentence(text: &str) -> &str {
    text.split(['.', '!', '?', '\n']).next().unwrap_or(text)
}

fn is_greeting(words: &[String]) -> bool {
    words.len() <= 3 && words.iter().all(|word| is_one_of(word, GREETINGS))
}

fn is_opener(word: &str) -> bool {
    is_one_of(word, OPENERS)
}

fn is_one_of(word: &str, set: &[&str]) -> bool {
    let lower = word.to_lowercase();
    set.contains(&lower.as_str())
}

/// `words` from its first content word on.
fn strip_openers(words: &[String]) -> &[String] {
    let from = words
        .iter()
        .position(|word| !is_opener(word))
        .unwrap_or(words.len());
    &words[from..]
}

/// A subject is a handful of words, not the message: the first [`MAX_WORDS`] of
/// them, cut to [`MAX_CHARS`], marked with an ellipsis when more followed, and
/// capitalised so a prompt written in lower case still reads as a subject.
fn shorten(words: &[String]) -> String {
    let mut cut = words.len() > MAX_WORDS;
    let mut title = words
        .iter()
        .take(MAX_WORDS)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    if title.chars().count() > MAX_CHARS {
        title = title.chars().take(MAX_CHARS).collect();
        if let Some(last_space) = title.rfind(' ') {
            title.truncate(last_space);
        }
        cut = true;
    }
    if cut {
        title.push('…');
    }
    capitalize_first(&title)
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_greeting_names_the_onboarding() {
        assert_eq!(
            derive("Hello", "Hi! How can I help?").as_deref(),
            Some("Initial user onboarding")
        );
        assert_eq!(
            derive("Hey there", "").as_deref(),
            Some("Initial user onboarding")
        );
    }

    /// A prompt that only agrees or thanks answers the conversation rather than
    /// naming it, and so does one with no words in it at all.
    #[test]
    fn a_pleasantry_names_nothing() {
        assert_eq!(derive("Thanks!", "Any time."), None);
        assert_eq!(derive("ok", "Done."), None);
        assert_eq!(derive("?", ""), None);
    }

    #[test]
    fn a_request_names_its_own_work() {
        assert_eq!(
            derive("Can you add a title to the sidebar?", "Sure").as_deref(),
            Some("Add a title to the sidebar")
        );
        assert_eq!(
            derive("the sidebar test is failing", "...").as_deref(),
            Some("Sidebar test is failing")
        );
    }

    /// The same exchange always names the same subject: the derivation reads the
    /// content, never a clock, a counter or a model.
    #[test]
    fn the_derivation_is_deterministic() {
        let messages = [
            ("user", "Hello"),
            ("assistant", "Hi!"),
            ("user", "Now fix the failing sidebar test"),
            ("assistant", "Done."),
        ];
        let first = agent_subject(NEW_CONVERSATION_TITLE, &messages);
        assert_eq!(first, agent_subject(NEW_CONVERSATION_TITLE, &messages));
        assert_eq!(first.as_deref(), Some("Fix the failing sidebar test"));
        assert_eq!(
            derive("Now fix the failing sidebar test", "Done."),
            derive("Now fix the failing sidebar test", "Done."),
        );
    }

    /// A conversation named for its first exchange is renamed when a later one
    /// moves it to another subject — the agent does not freeze a subject it
    /// wrote itself.
    #[test]
    fn a_later_exchange_renames_the_conversation() {
        let mut messages = vec![("user", "Hello"), ("assistant", "Hi there!")];
        assert_eq!(
            agent_subject(NEW_CONVERSATION_TITLE, &messages).as_deref(),
            Some("Initial user onboarding")
        );

        messages.push(("user", "Now make the sidebar show conversation titles"));
        messages.push(("assistant", "On it."));
        assert_eq!(
            agent_subject("Initial user onboarding", &messages).as_deref(),
            Some("Make the sidebar show conversation titles"),
            "the later subject replaces the agent's own earlier one"
        );

        // A follow-up that only agrees names nothing, so the subject stands.
        messages.push(("user", "Thanks!"));
        messages.push(("assistant", "Any time."));
        assert_eq!(
            agent_subject("Make the sidebar show conversation titles", &messages),
            None
        );
    }

    /// A subject the user typed is the user's: the agent never takes it back,
    /// however much is said in the conversation afterwards.
    #[test]
    fn a_subject_the_user_typed_stands() {
        let messages = [("user", "Hello"), ("assistant", "Bine ai venit!")];
        assert_eq!(agent_subject("Planul de lansare", &messages), None);
        assert_eq!(agent_subject("New Agent", &messages), None);
    }

    /// Nothing has been answered, so nothing names the conversation: it keeps
    /// whatever it was created with.
    #[test]
    fn an_unanswered_conversation_keeps_its_title() {
        assert_eq!(agent_subject(NEW_CONVERSATION_TITLE, &[]), None);
        assert_eq!(
            agent_subject(NEW_CONVERSATION_TITLE, &[("user", "Hello")]),
            None
        );
        assert_eq!(
            agent_subject(NEW_CONVERSATION_TITLE, &[("tool", "call_1\nfile.txt")]),
            None
        );
    }

    /// A prompt with no words in it is named after the reply: it is still the
    /// conversation's own content, and the user asked for it.
    #[test]
    fn a_wordless_prompt_is_named_after_the_reply() {
        assert_eq!(
            derive("?", "Settings holds the model key").as_deref(),
            Some("Settings holds the model key")
        );
    }

    /// The placeholder is not a subject: a conversation whose content derives it
    /// keeps the title it has rather than being named the placeholder again.
    #[test]
    fn the_placeholder_is_never_the_derived_subject() {
        let messages = [("user", "new conversation"), ("assistant", "ok")];
        assert_eq!(agent_subject(NEW_CONVERSATION_TITLE, &messages), None);
    }

    #[test]
    fn a_subject_is_short() {
        let long = "please refactor the renderer so the sidebar title column wraps without \
                    truncating the timestamp of every conversation in the list";
        let subject = derive(long, "").expect("a subject");
        assert!(subject.chars().count() <= MAX_CHARS + 1, "{subject}");
        assert!(subject.ends_with('…'), "{subject}");
        assert_eq!(subject.split_whitespace().count(), MAX_WORDS);
    }
}
