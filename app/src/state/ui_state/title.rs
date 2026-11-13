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
    let exchanges = exchanges(messages);
    let derived: Vec<Option<String>> = exchanges
        .iter()
        .map(|(question, reply)| derive(question, reply))
        .collect();
    // A title an earlier version derived from the prompt alone still counts as
    // the agent's own, so the answer's subject can replace it. A title the user
    // typed matches neither rule and stands.
    let prompted: Vec<Option<String>> = exchanges
        .iter()
        .map(|(question, _)| prompt_subject_of(question))
        .collect();
    // A follow-up that only thanks or agrees names nothing, so it leaves the
    // conversation under the subject of the last exchange that did.
    let subject = derived.iter().rev().flatten().next()?.clone();
    if subject == stored_title || subject == NEW_CONVERSATION_TITLE {
        return None;
    }
    let user_named = stored_title != NEW_CONVERSATION_TITLE
        && !derived
            .iter()
            .chain(prompted.iter())
            .any(|s| s.as_deref() == Some(stored_title));
    (!user_named).then_some(subject)
}

/// Pair each prompt with the agent's final answer to it, in order. A turn can
/// write several assistant rows — an announcement before each tool call and
/// the answer after them — and the answer is the one that names the work. A
/// tool result is the agent's own doing rather than an answer to a question,
/// so it pairs with nothing.
fn exchanges<'a>(messages: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut pairs = Vec::new();
    let mut asked: Option<&'a str> = None;
    let mut answered: Option<&'a str> = None;
    for &(role, content) in messages {
        match role {
            "user" => {
                if let (Some(question), Some(reply)) = (asked.take(), answered.take()) {
                    pairs.push((question, reply));
                }
                asked = Some(content);
            }
            "assistant" if asked.is_some() && !content.trim().is_empty() => {
                answered = Some(content);
            }
            _ => {}
        }
    }
    if let (Some(question), Some(reply)) = (asked, answered) {
        pairs.push((question, reply));
    }
    pairs
}

/// The subject one exchange names, or `None` when it names nothing at all.
///
/// The agent's answer names the work: what it says it did, found or decided is
/// the thing the conversation turned out to be about, and the prompt is the
/// fallback for a reply that only acknowledges or narrates its own steps.
fn derive(question: &str, reply: &str) -> Option<String> {
    let asked = words(question);
    // A greeting opens onboarding however the agent answered it.
    if !asked.is_empty() && is_greeting(&asked) {
        return Some(GREETING_SUBJECT.to_string());
    }
    reply_subject(reply).or_else(|| prompt_subject_of(question))
}

/// The subject the prompt alone names, using the opener and greeting rules.
fn prompt_subject_of(question: &str) -> Option<String> {
    let asked = words(question);
    if asked.is_empty() {
        return None;
    }
    if is_greeting(&asked) {
        return Some(GREETING_SUBJECT.to_string());
    }
    let named = strip_openers(&asked);
    // A message made only of asking words — `thanks!`, `ok` — responds to the
    // conversation instead of moving it, and names nothing.
    (!named.is_empty()).then(|| shorten(named))
}

/// The subject the agent's own words name: its first heading or sentence that
/// states something, skipping procedural narration ("Let me read…") and bare
/// acknowledgements ("Done."). A fenced code block is skipped, because a title
/// is prose.
fn reply_subject(reply: &str) -> Option<String> {
    let mut in_fence = false;
    for line in reply.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || line.is_empty() {
            continue;
        }
        if let Some(heading) = heading_text(line) {
            let named = words(&heading);
            if !is_structural_heading(&named) && names_work(&named) {
                return Some(shorten(&named));
            }
        }
        for sentence in sentences(strip_list_marker(line)) {
            let cleaned = clean_markdown(strip_lead_in(sentence));
            let named = words(&cleaned);
            if names_work(&named) {
                return Some(shorten(&named));
            }
        }
    }
    None
}

/// A markdown heading's text, without its hashes.
fn heading_text(line: &str) -> Option<String> {
    let text = line.strip_prefix('#')?.trim_start_matches('#').trim();
    Some(clean_markdown(text.trim_end_matches('#').trim()))
}

/// Headings that label a section rather than name the work.
fn is_structural_heading(words: &[String]) -> bool {
    let lower: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
    matches!(
        lower.join(" ").as_str(),
        "summary" | "overview" | "plan" | "next steps" | "notes" | "example" | "usage"
    )
}

/// A sentence's worth of the line: a run of text ending at a period, a bang or
/// a question mark that is followed by a space or the end. A dot inside a file
/// name or an abbreviation does not split.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, character) in line.char_indices() {
        if !matches!(character, '.' | '!' | '?') {
            continue;
        }
        let end = index + character.len_utf8();
        let next = line[end..].chars().next();
        if next.is_none() || next.is_some_and(char::is_whitespace) {
            out.push(&line[start..end]);
            start = end;
        }
    }
    if start < line.len() {
        out.push(&line[start..]);
    }
    out
}

/// Drop a short conversational lead-in before a colon or a dash, so
/// `Short version: in Goble the cloud is a worker` names the statement and
/// `Fair challenge — the docs are stale` names the claim.
fn strip_lead_in(sentence: &str) -> &str {
    let trimmed = sentence.trim_start();
    for (index, character) in trimmed.char_indices() {
        let limit = match character {
            ':' => 8,
            '—' | '–' => 2,
            _ => continue,
        };
        let prefix = trimmed[..index].trim();
        if prefix.split_whitespace().count() <= limit && !prefix.contains('`') {
            return trimmed[index + character.len_utf8()..].trim_start();
        }
        break;
    }
    trimmed
}

/// A list item's marker (`- `, `* `, `1. `) without its marker; other lines are
/// returned as they are.
fn strip_list_marker(line: &str) -> &str {
    let line = line.trim_start();
    for marker in ["- ", "* ", "• ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest;
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        if let Some(rest) = line[digits..].strip_prefix(". ") {
            return rest;
        }
    }
    line
}

/// Remove inline markdown from a candidate: emphasis marks, backticks, heading
/// hashes and the brackets of a link's label. Underscores stay, because they
/// are part of an identifier like `is_palindrome`.
fn clean_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '`' | '*' | '#' | '>' => {}
            '[' => {
                let mut label = String::new();
                while let Some(&next) = chars.peek() {
                    if next == ']' {
                        chars.next();
                        break;
                    }
                    label.push(next);
                    chars.next();
                }
                if chars.peek() == Some(&'(') {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                    }
                }
                out.push_str(&label);
            }
            _ => out.push(character),
        }
    }
    out.trim_start_matches(['·', '•', '-', ' ', '\t'])
        .trim()
        .to_string()
}

/// Whether `words` states something rather than acknowledging the turn or
/// narrating the next tool call. At least two words are needed to name work;
/// the conversational phrases are the ones an agent opens with on its way to
/// the answer.
fn names_work(words: &[String]) -> bool {
    if words.len() < 2 {
        return false;
    }
    let lower: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
    let phrase = lower.join(" ");
    const NOT_WORK: &[&str] = &[
        "absolutely",
        "any time",
        "done",
        "fair challenge",
        "good question",
        "got it",
        "great",
        "happy to help",
        "here is",
        "here's",
        "i am going",
        "i checked",
        "i grepped",
        "i have",
        "i looked",
        "i mapped",
        "i need to",
        "i opened",
        "i ran",
        "i read",
        "i searched",
        "i started",
        "i will",
        "i'll",
        "i'm checking",
        "i'm going",
        "i'm reading",
        "i've",
        "let me",
        "let's",
        "no problem",
        "ok",
        "okay",
        "on it",
        "perfect",
        "sounds good",
        "sure",
        "thanks",
        "thinking",
        "thought",
        "understood",
        "will do",
        "you are welcome",
        "you're welcome",
    ];
    !NOT_WORK.iter().any(|opening| {
        phrase == *opening
            || phrase.starts_with(&format!("{opening} "))
            || phrase.starts_with(&format!("{opening},"))
    })
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

/// The subject a prompt alone names, or `None` when it names nothing.
///
/// A CLI harness keeps its transcript in its own store, so the exchange a
/// native conversation is named from is not there to read; the prompt the user
/// sent into the harness is the one part of it goble sees. Everything else is
/// [`agent_subject`]'s own rule — the same words dropped and the same length —
/// applied to that prompt as the opening of a conversation nobody has named.
pub fn prompt_subject(prompt: &str) -> Option<String> {
    agent_subject(
        NEW_CONVERSATION_TITLE,
        &[("user", prompt), ("assistant", "")],
    )
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

    /// The agent's answer names the conversation: it states what the work
    /// turned out to be, which the prompt alone cannot.
    #[test]
    fn the_agents_answer_names_the_conversation() {
        assert_eq!(
            derive("what does the store do?", "SQLite keeps every message durable.").as_deref(),
            Some("SQLite keeps every message durable")
        );
        assert_eq!(
            derive("what did you find?", "Major finding — the worker has no cloud provider.")
                .as_deref(),
            Some("The worker has no cloud provider")
        );
        assert_eq!(
            derive(
                "check the sidebar",
                "Short version: the cloud is just a worker."
            )
            .as_deref(),
            Some("The cloud is just a worker"),
        );
    }

    /// The reply's own narration is not a subject: a model that announces the
    /// tool it is about to call still gets named by the statement after it.
    #[test]
    fn the_replys_procedural_openers_are_skipped() {
        assert_eq!(
            derive(
                "look at the renderer",
                "Let me read the element tree.\nThe sidebar orders cards by recency."
            )
            .as_deref(),
            Some("The sidebar orders cards by recency")
        );
        assert_eq!(
            derive("fix it", "Sure. Done.").as_deref(),
            Some("Fix it"),
            "a reply of acknowledgements leaves the prompt to name the work"
        );
    }

    /// A title an earlier derivation wrote from the prompt is still the
    /// agent's own, so the answer replaces it. A title the user typed does not.
    #[test]
    fn the_answers_subject_replaces_an_earlier_prompt_title() {
        let messages = [
            ("user", "fix the sidebar"),
            ("assistant", "The sidebar now orders its cards by recency."),
        ];
        assert_eq!(
            agent_subject("Fix the sidebar", &messages).as_deref(),
            Some("The sidebar now orders its cards…")
        );
        assert_eq!(agent_subject("Launch plan", &messages), None);
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
    fn a_prompt_names_a_conversation_on_its_own() {
        assert_eq!(
            prompt_subject("Arata un text colorat in chat").as_deref(),
            Some("Arata un text colorat in chat")
        );
        assert_eq!(
            prompt_subject("Can you add a title to the sidebar?").as_deref(),
            Some("Add a title to the sidebar")
        );
        assert_eq!(prompt_subject("Thanks!"), None);
        assert_eq!(prompt_subject("   "), None);
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
