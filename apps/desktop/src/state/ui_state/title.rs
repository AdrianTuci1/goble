//! The subject a conversation is named with — an LLM-generated title from the
//! first prompt, or a deterministic keyword subject when no model can answer.
//!
//! A conversation is created with the placeholder [`NEW_CONVERSATION_TITLE`] and
//! is named from the first prompt sent into it, before the agent has replied.
//! That is the DeepSeek Harness title design: the session-title service derives a
//! deterministic fallback from the first eligible human message (a few leading
//! words), then an optional model-backed provider replaces it with a generated
//! subject; the title is durable and a user rename pins it
//! (`packages/session/session-title/src/index.ts`, and
//! `packages/session/session-title-first-prompt-llm/src/index.ts` for the title
//! request). Goble mirrors the timing and the shape of the request:
//!
//! * the first prompt writes a deterministic subject at once, so a workspace
//!   with no provider key still has a named conversation;
//! * the configured provider is asked, in a short auxiliary request that never
//!   blocks the turn, to summarise that prompt into a subject; its answer
//!   replaces the fallback when it arrives;
//! * the subject is the conversation's topic, not a restatement of the message:
//!   the model is told so, its answer is clamped to a handful of words, and a
//!   fallback that would echo the prompt verbatim is not used;
//! * a title the user typed is never taken back.

use std::sync::Arc;

use goble_core::llm::{CompletionRequest, LlmProvider};
use goble_desktop_service::DesktopState;

use super::NEW_CONVERSATION_TITLE;

/// The most words a subject carries.
const MAX_WORDS: usize = 6;

/// The most characters a subject carries.
const MAX_CHARS: usize = 48;

/// The most words a model's line may carry before it stops reading as a subject.
/// A longer first line is an explanation ("Here is a title for..."), not a
/// title; it is rejected and the deterministic subject stands.
const MAX_EXPLANATION_WORDS: usize = 10;

/// The output-token cap for the title request. A subject is a handful of words,
/// so the auxiliary call stays cheap next to the turn it describes.
pub const MAX_TITLE_TOKENS: u32 = 48;

/// Sampling temperature for the title request: a little variation without
/// drifting into commentary.
const TITLE_TEMPERATURE: f32 = 0.2;

/// The instruction the title model receives. It is DeepSeek Harness's
/// first-prompt system instruction (one line, plain text, the messages'
/// language, a few words, best-effort) with the subject-not-the-message rule
/// this app needs spelled out.
pub const TITLE_SYSTEM_PROMPT: &str = "\
Create a concise title for an AI coding-assistant session from the supplied human messages.
Return only the title on one line, in plain text of natural language, with no quotes, prefix, explanation, Markdown, XML, or terminal control codes. No code is allowed.
Use the language of the messages.
The title is the conversation's subject or topic, never a restatement or echo of the message itself. Aim for about 3 to 6 words.
If the messages give little to name, still return a short best-effort title, such as Greeting, instead of explaining.";

/// Words that phrase a request rather than name its topic. They are dropped
/// while they lead a prompt, so `Arata un text colorat in chat` starts at
/// `text` and `Can you add a title` starts at `title`.
const LEADING_REQUESTS: &[&str] = &[
    "a",
    "add",
    "adauga",
    "adaugă",
    "afiseaza",
    "afișează",
    "ajuta",
    "ajută",
    "an",
    "arata",
    "arată",
    "ask",
    "build",
    "can",
    "check",
    "could",
    "create",
    "creeaza",
    "creează",
    "cum",
    "do",
    "does",
    "explain",
    "fa",
    "fă",
    "find",
    "fix",
    "give",
    "help",
    "how",
    "i",
    "implement",
    "just",
    "let",
    "make",
    "modifica",
    "modifică",
    "my",
    "o",
    "our",
    "please",
    "pls",
    "poti",
    "poți",
    "pot",
    "scrie",
    "show",
    "spune",
    "tell",
    "the",
    "test",
    "this",
    "un",
    "update",
    "use",
    "vreau",
    "want",
    "what",
    "when",
    "where",
    "which",
    "who",
    "why",
    "with",
    "would",
    "write",
    "you",
];

/// Words a subject never carries: articles, pronouns, auxiliaries, politeness
/// and the connectives a phrase can do without. They are dropped anywhere, so a
/// keyword subject is the topics the prompt names.
const STOP_WORDS: &[&str] = &[
    "a", "about", "acel", "acea", "acest", "aceasta", "aceste", "acesti", "ai", "al", "ale", "am",
    "an", "and", "are", "as", "at", "avem", "aveți", "be", "been", "being", "bine", "but", "by",
    "ca", "că", "can", "care", "ce", "could", "cu", "da", "dar", "de", "did", "din", "do", "does",
    "dacă", "după", "ea", "ei", "el", "ele", "era", "este", "eu", "fara", "fără", "fie", "foarte",
    "for", "from", "had", "has", "have", "he", "her", "his", "how", "i", "if", "in", "intre",
    "între", "is", "it", "its", "just", "la", "let", "mai", "me", "meu", "my", "noi", "nostru",
    "not", "now", "nu", "o", "ok", "okay", "or", "our", "over", "pe", "please", "pls", "pot",
    "poti", "poți", "prin", "sau", "sa", "să", "she", "should", "si", "și", "so", "sunt", "sub",
    "thank", "thanks", "that", "the", "their", "them", "then", "these", "they", "this", "those",
    "to", "tot", "toate", "totți", "tu", "un", "under", "unei", "unui", "us", "vă", "voastru",
    "voi", "vrea", "vreau", "was", "we", "were", "what", "when", "where", "which", "who", "why",
    "will", "with", "would", "yes", "you", "your", "și",
];

/// Connectives that read as part of a topic phrase. A subject keeps them when
/// they sit between the words it keeps, so the fallback reads
/// `Text colorat in chat` rather than `Text colorat chat`.
const CONNECTORS: &[&str] = &[
    "about", "and", "at", "by", "cu", "de", "despre", "din", "for", "from", "in", "în", "la", "of",
    "on", "or", "pe", "pentru", "sau", "și", "to", "via", "vs", "with",
];

/// Messages that are only a greeting or a pleasantry name no topic at all.
const GREETINGS: &[&str] = &[
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
    "thanks",
    "thank",
    "there",
    "yo",
    "ok",
    "okay",
    "yes",
    "no",
];

/// The subject a prompt names deterministically, or `None` when it names no
/// topic at all (`thanks!`, `hello`, an empty line).
///
/// This is the fallback for a workspace with no provider key (and the immediate
/// name a conversation gets while the model is asked): the keywords the prompt
/// is about, in their own order, with request phrasing, articles, pronouns and
/// pleasantries removed and a trailing connective cut. It is a subject-like
/// phrase — `Text colorat in chat` for `Arata un text colorat in chat` — never
/// the prompt handed back, and never longer than [`MAX_WORDS`] words.
///
/// It is deliberately not routed through a reply: the topic is what the user
/// asked for, and a reply-derived title would echo a message just as the prompt
/// would.
pub fn deterministic_subject(prompt: &str) -> Option<String> {
    let tokens = words(prompt);
    if tokens.is_empty() {
        return None;
    }
    // Everything the message says is a greeting or a pleasantry: there is no
    // topic to name, so the neutral placeholder stands (and a reply cannot name
    // it either).
    if tokens.iter().all(|word| is_one_of(word, GREETINGS)) {
        return None;
    }
    let kinds: Vec<Kind> = tokens.iter().map(|word| kind_of(word)).collect();
    // The phrase runs from the first topic word to the last: a prompt that opens
    // with a request (`fix the sidebar`) starts at `sidebar` because the request
    // verb is filler, and its own wording never leads the subject.
    let first = kinds.iter().position(|kind| *kind == Kind::Content)?;
    let last = kinds.iter().rposition(|kind| *kind == Kind::Content)?;
    let kept: Vec<&str> = tokens[first..=last]
        .iter()
        .zip(&kinds[first..=last])
        .filter(|(_, kind)| **kind != Kind::Filler)
        .map(|(word, _)| word.as_str())
        .collect();
    let subject = shorten_subject(&kept)?;
    // The placeholder is not a subject: a prompt that only says it names
    // nothing new.
    (normalized(&subject) != normalized(NEW_CONVERSATION_TITLE)).then_some(subject)
}

/// The user turn the title request carries: the prompt, framed as a JSON array
/// so the message's own text cannot read as part of the instructions. This is
/// DeepSeek Harness's framing (`session-title-first-prompt-llm/src/index.ts`,
/// `frameMessages`), narrowed to the one message this app has.
pub fn title_request_input(prompt: &str) -> String {
    let messages = serde_json::json!([{ "text": prompt }]);
    format!("Generate the session title from this JSON array of human messages:\n{messages}")
}

/// The completion request one title generation sends to `provider`.
pub(crate) fn title_request(provider_name: &str, model: &str, prompt: &str) -> CompletionRequest {
    CompletionRequest::new(provider_name, model)
        .with_system(TITLE_SYSTEM_PROMPT)
        .with_user(title_request_input(prompt))
        .with_temperature(Some(TITLE_TEMPERATURE))
        .with_max_tokens(MAX_TITLE_TOKENS)
}

/// The title a model's answer carries, or `None` when the answer is not one.
///
/// The title is the first non-empty line, with the wrapping emphasis, quotes,
/// list markers, backticks and a `Title:`-style prefix models put around it
/// removed, and its trailing sentence punctuation cut — DeepSeek Harness's
/// `titleFromOutput` and `normalizeSessionTitle` rules, applied to one line. A
/// line that reads as an explanation (more than [`MAX_EXPLANATION_WORDS`] words)
/// or as a refusal is rejected so the deterministic subject stands.
pub fn parse_title(output: &str) -> Option<String> {
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let mut line = clean_title_line(line);
    if line.is_empty() {
        return None;
    }
    let lower = line.to_lowercase();
    const REFUSALS: &[&str] = &[
        "i'm sorry",
        "i am sorry",
        "i cannot",
        "i can't",
        "sorry,",
        "unfortunately",
        "as an ai",
    ];
    if REFUSALS.iter().any(|refusal| lower.starts_with(refusal)) {
        return None;
    }
    // A commentary line ("Here is a short title for the conversation ...") is
    // not a subject; a model that did not obey the one-line rule keeps the
    // fallback.
    if words(&line).len() > MAX_EXPLANATION_WORDS {
        return None;
    }
    let named = words(&line);
    if named.is_empty() {
        return None;
    }
    line = shorten_subject(&named.iter().map(String::as_str).collect::<Vec<_>>())?;
    Some(line)
}

/// One line of model output, stripped of the wrappers a title comes wrapped in.
fn clean_title_line(line: &str) -> String {
    let mut line = line.trim().to_string();
    // A list marker or heading hash in front of the title.
    line = line
        .trim_start_matches(['-', '*', '•', '+'])
        .trim_start_matches('#')
        .trim()
        .to_string();
    // `Title: ...` / `Session title: ...` / `Subject: ...` — the label a model
    // puts before a compliant title, and the sentence a model puts before a
    // non-compliant one (`Here is the title: ...`).
    if let Some((prefix, suffix)) = line.split_once(':') {
        let prefix_words = words(prefix).len();
        let suffix = suffix.trim();
        if prefix_words <= 5 && !prefix.contains('`') && !suffix.is_empty() {
            line = suffix.to_string();
        }
    }
    // Wrapping emphasis, quotes and code ticks, possibly nested.
    loop {
        let before = line.len();
        for (open, close) in [
            ("**", "**"),
            ("__", "__"),
            ("*", "*"),
            ("_", "_"),
            ("\"", "\""),
            ("'", "'"),
            ("`", "`"),
            ("“", "”"),
            ("‘", "’"),
            ("«", "»"),
            ("„", "“"),
        ] {
            if let Some(inner) = line
                .strip_prefix(open)
                .and_then(|rest| rest.strip_suffix(close))
            {
                line = inner.trim().to_string();
            }
        }
        if line.len() == before {
            break;
        }
    }
    // Emphasis marks inside the title are markdown, not part of the subject.
    line = line.replace("**", " ").replace('`', " ");
    // Trailing sentence punctuation (the owner's "no trailing punctuation").
    line.trim_end_matches(['.', ',', ';', ':', '!', '?', '…', '-', '–', '—', '"', '\''])
        .trim()
        .to_string()
}

/// Shorten a topic phrase to a subject: the first [`MAX_WORDS`] words, cut to
/// [`MAX_CHARS`] on a word boundary, first letter capitalised.
fn shorten_subject(words: &[&str]) -> Option<String> {
    if words.is_empty() {
        return None;
    }
    let joined = words
        .iter()
        .take(MAX_WORDS)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let mut title = joined;
    if title.chars().count() > MAX_CHARS {
        title = title.chars().take(MAX_CHARS).collect();
        if let Some(last_space) = title.rfind(' ') {
            title.truncate(last_space);
        }
    }
    let title = capitalize_first(title.trim());
    (!title.is_empty()).then_some(title)
}

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The words of `text`, each trimmed of the punctuation around it and
/// normalised for the case of their first letter.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Lower-case words with the punctuation dropped: how two titles are compared
/// for "is this the same string".
fn normalized(text: &str) -> String {
    words(text)
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// How one word of a prompt reads in a subject.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Names a topic: kept.
    Content,
    /// Reads as part of a phrase: kept between content words.
    Connector,
    /// Phrasing, not content: dropped.
    Filler,
}

fn kind_of(word: &str) -> Kind {
    let lower = word.to_lowercase();
    if CONNECTORS.contains(&lower.as_str()) {
        Kind::Connector
    } else if STOP_WORDS.contains(&lower.as_str())
        || LEADING_REQUESTS.contains(&lower.as_str())
        || GREETINGS.contains(&lower.as_str())
    {
        Kind::Filler
    } else {
        Kind::Content
    }
}

fn is_one_of(word: &str, set: &[&str]) -> bool {
    let lower = word.to_lowercase();
    set.contains(&lower.as_str())
}

/// Generate a subject title for `prompt` with `provider`, or `None` when the
/// call or its answer does not yield one (a failure is ignored: the title the
/// prompt pass already wrote stands).
///
/// `provider_name`/`model` are the route recorded on the request; `provider` is
/// the resolved client. The call carries its own small output-token cap and its
/// own system instruction, so it is a separate, cheap request — never the turn's.
pub async fn generate_title(
    provider: &dyn LlmProvider,
    provider_name: &str,
    model: &str,
    prompt: &str,
) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() || model.trim().is_empty() {
        return None;
    }
    let response = provider
        .complete(title_request(provider_name, model, prompt))
        .await
        .ok()?;
    let title = parse_title(&response.content)?;
    // The owner's rule, enforced at the last line of defence as well: a model
    // that hands the message back has not named the conversation.
    (normalized(&title) != normalized(prompt)).then_some(title)
}

/// Whether `current` is a title the app may replace: the placeholder, an empty
/// name, or the deterministic subject [`generate_and_store_title`] derives from
/// the same prompt. Anything else is the user's (the create row's draft, the
/// rename row) and stands.
pub fn title_owned_by_app(current: &str, fallback: Option<&str>) -> bool {
    let current = current.trim();
    current.is_empty()
        || current == NEW_CONVERSATION_TITLE
        || fallback.is_some_and(|fallback| current == fallback)
}

/// Ask `provider` for a subject and write it over the app-owned title of
/// `conversation_id`, returning the title written.
///
/// The pin is checked twice: before the call (a conversation the user already
/// renamed is not worth a model request) and after it (the user may have renamed
/// it while the request was in flight). The write goes through
/// [`DesktopState::set_chat_title`], so it is durable and emits
/// `chats:updated` — the sidebar row, the tab label and the pane bar update
/// without a restart.
pub async fn generate_and_store_title(
    desktop: &DesktopState,
    provider: &dyn LlmProvider,
    provider_name: &str,
    model: &str,
    conversation_id: &str,
    prompt: &str,
) -> Option<String> {
    if conversation_id.is_empty() {
        return None;
    }
    let fallback = deterministic_subject(prompt);
    let current = conversation_title(desktop, conversation_id)?;
    if !title_owned_by_app(&current, fallback.as_deref()) {
        return None;
    }
    let title = generate_title(provider, provider_name, model, prompt).await?;
    let current = conversation_title(desktop, conversation_id)?;
    if !title_owned_by_app(&current, fallback.as_deref()) {
        return None;
    }
    if let Err(error) = desktop.set_chat_title(conversation_id, &title) {
        log::warn!("set_chat_title for {conversation_id} failed: {error}");
        return None;
    }
    Some(title)
}

/// Spawn [`generate_and_store_title`] off the UI thread for the conversation
/// `conversation_id` was just named from its first prompt. Returns whether a
/// request was started.
///
/// A machine with no provider key resolves to the deterministic mock; no model
/// generated the fallback the prompt pass wrote, so no request is made and the
/// subject stands. The task owns its own failure: a title that does not arrive
/// leaves the fallback in place.
pub fn spawn_title_generation(
    desktop: &Arc<DesktopState>,
    conversation_id: &str,
    prompt: &str,
    provider_name: &str,
    model: &str,
) -> bool {
    if conversation_id.is_empty() || prompt.trim().is_empty() {
        return false;
    }
    let (provider, resolved_model) = desktop.resolve_llm_provider(provider_name, model);
    if provider.name() == "mock" || resolved_model.trim().is_empty() {
        return false;
    }
    let desktop = Arc::clone(desktop);
    let conversation_id = conversation_id.to_string();
    let prompt = prompt.to_string();
    tokio::spawn(async move {
        if let Some(title) = generate_and_store_title(
            &desktop,
            provider.as_ref(),
            provider.name(),
            &resolved_model,
            &conversation_id,
            &prompt,
        )
        .await
        {
            log::debug!("conversation {conversation_id} titled: {title}");
        }
    });
    true
}

/// The stored title of `conversation_id`, when the conversation is still there.
fn conversation_title(desktop: &DesktopState, conversation_id: &str) -> Option<String> {
    desktop
        .list_chats()
        .into_iter()
        .find(|chat| chat.id == conversation_id)
        .map(|chat| chat.title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::llm::{CompletionResponse, Role};
    use std::sync::Mutex;

    /// A provider that records the request it was sent and returns one answer,
    /// so the shape of the title request is asserted rather than assumed.
    struct RecordingProvider {
        name: String,
        reply: Result<String, String>,
        request: Mutex<Option<CompletionRequest>>,
    }

    impl RecordingProvider {
        fn answering(reply: &str) -> Arc<Self> {
            Self::new(Ok(reply.to_string()))
        }

        fn new(reply: Result<String, String>) -> Arc<Self> {
            Arc::new(Self {
                name: "title-test".to_string(),
                reply,
                request: Mutex::new(None),
            })
        }

        fn take_request(&self) -> CompletionRequest {
            self.request
                .lock()
                .expect("the recorded request")
                .take()
                .expect("the provider was called")
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for RecordingProvider {
        fn name(&self) -> &str {
            &self.name
        }

        async fn complete(&self, request: CompletionRequest) -> anyhow::Result<CompletionResponse> {
            *self.request.lock().expect("the recorded request") = Some(request);
            match &self.reply {
                Ok(content) => Ok(CompletionResponse::new(content.clone(), Vec::new())),
                Err(error) => Err(anyhow::anyhow!("{error}")),
            }
        }

        async fn complete_stream(
            &self,
            _request: CompletionRequest,
        ) -> anyhow::Result<
            std::pin::Pin<
                Box<dyn futures::Stream<Item = goble_core::llm::CompletionStreamEvent> + Send>,
            >,
        > {
            anyhow::bail!("the title request uses complete(), not the stream")
        }
    }

    /// A title is a subject, not the prompt handed back: the deterministic
    /// fallback names the topic in a handful of words and never carries the
    /// request's own phrasing or the whole message.
    #[test]
    fn a_deterministic_subject_names_the_topic_not_the_message() {
        let prompt = "Arata un text colorat in chat";
        let subject = deterministic_subject(prompt).expect("a subject");
        assert_eq!(subject, "Text colorat in chat");
        assert_ne!(subject, prompt, "the subject is not the message");
        assert!(subject.split_whitespace().count() <= MAX_WORDS);

        assert_eq!(
            deterministic_subject("Can you add a title to the sidebar?").as_deref(),
            Some("Title to sidebar")
        );
        assert_eq!(
            deterministic_subject("fix the sidebar").as_deref(),
            Some("Sidebar")
        );
    }

    /// The topic words survive in their own order with the phrasing dropped; a
    /// long prompt is cut to a subject, not echoed.
    #[test]
    fn a_long_prompt_is_cut_to_a_subject() {
        let prompt = "please refactor the renderer so the sidebar title column wraps without \
                      truncating the timestamp of every conversation in the list";
        let subject = deterministic_subject(prompt).expect("a subject");
        assert_ne!(subject, prompt);
        assert!(subject.split_whitespace().count() <= MAX_WORDS, "{subject}");
        assert!(subject.chars().count() <= MAX_CHARS, "{subject}");
        assert!(
            !subject.ends_with('…'),
            "a subject is not a truncated sentence"
        );
    }

    /// A greeting or a pleasantry names no topic at all: the neutral placeholder
    /// stands rather than the message being shown as the title.
    #[test]
    fn a_pleasantry_names_no_subject() {
        assert_eq!(deterministic_subject("Thanks!"), None);
        assert_eq!(deterministic_subject("Hello"), None);
        assert_eq!(deterministic_subject("   "), None);
        assert_eq!(deterministic_subject("?"), None);
        assert_eq!(deterministic_subject(NEW_CONVERSATION_TITLE), None);
    }

    /// The same prompt always names the same subject: the derivation reads the
    /// words, never a clock, a counter or a model.
    #[test]
    fn the_deterministic_subject_is_deterministic() {
        let prompt = "data durable in sqlite?";
        assert_eq!(deterministic_subject(prompt), deterministic_subject(prompt));
        assert_eq!(
            deterministic_subject(prompt).as_deref(),
            Some("Data durable in sqlite")
        );
    }

    /// The request the title pass sends is the model-backed provider's own: a
    /// system instruction asking for a subject, one framed user message, a small
    /// output-token cap and the route recorded. It carries no tools.
    #[test]
    fn the_title_request_asks_for_a_subject() {
        let request = title_request("openai", "gpt-4o-mini", "Arata un text colorat in chat");
        assert_eq!(request.provider, "openai");
        assert_eq!(request.model, "gpt-4o-mini");
        assert_eq!(request.max_tokens, Some(MAX_TITLE_TOKENS));
        assert!(request.max_tokens.expect("a cap") <= 64);
        assert!(request.tools.is_empty());
        assert_eq!(
            request.messages[0].role,
            Role::System,
            "the instruction leads"
        );
        assert_eq!(request.messages[0].content, TITLE_SYSTEM_PROMPT);
        assert!(request.messages[0].content.contains("concise title"));
        assert!(request.messages[0].content.contains("subject or topic"));
        assert!(request.messages[0].content.contains("never a restatement"));
        assert_eq!(request.messages[1].role, Role::User);
        assert_eq!(
            request.messages[1].content,
            title_request_input("Arata un text colorat in chat")
        );
        assert!(
            request.messages[1]
                .content
                .contains(r#"[{"text":"Arata un text colorat in chat"}]"#),
            "the prompt rides in the framed JSON array"
        );
    }

    /// The model's answer is taken as the subject: one line, quotes, emphasis,
    /// a label prefix and trailing punctuation stripped.
    #[test]
    fn the_model_answer_is_read_as_a_title() {
        assert_eq!(
            parse_title("Colorful chat text").as_deref(),
            Some("Colorful chat text")
        );
        assert_eq!(
            parse_title("  **Colorful chat text**  ").as_deref(),
            Some("Colorful chat text")
        );
        assert_eq!(
            parse_title("\"Colorful chat text\"").as_deref(),
            Some("Colorful chat text")
        );
        assert_eq!(
            parse_title("Title: `Colorful chat text`.").as_deref(),
            Some("Colorful chat text")
        );
        assert_eq!(
            parse_title("Here is the title: Colorful chat text").as_deref(),
            Some("Colorful chat text")
        );
        assert_eq!(
            parse_title("Colorful chat text.\nIt names the first prompt.").as_deref(),
            Some("Colorful chat text"),
            "the first non-empty line is the title"
        );
        assert_eq!(
            parse_title("Colorful chat text…").as_deref(),
            Some("Colorful chat text")
        );
        // A long explanation is not a title, and neither is a refusal.
        assert_eq!(
            parse_title(
                "Here is a concise subject for this conversation that describes \
                 the colorful text the user asked about in the chat"
            ),
            None
        );
        assert_eq!(parse_title("I'm sorry, I could not name it."), None);
        assert_eq!(parse_title(""), None);
        assert_eq!(parse_title("   \n  "), None);
    }

    /// The generation the app's title pass runs asks the model and parses its
    /// answer: a subject comes back, and the request is the bounded title
    /// request (not a turn's).
    #[test]
    fn generation_returns_the_parsed_subject() {
        let provider = RecordingProvider::answering("Colorful chat text");
        let runtime = tokio::runtime::Runtime::new().expect("build runtime");
        let title = runtime
            .block_on(generate_title(
                provider.as_ref(),
                "title-test",
                "title-model",
                "Arata un text colorat in chat",
            ))
            .expect("a generated subject");
        assert_eq!(title, "Colorful chat text");
        let request = provider.take_request();
        assert_eq!(request.messages[0].content, TITLE_SYSTEM_PROMPT);
        assert_eq!(request.max_tokens, Some(MAX_TITLE_TOKENS));
    }

    /// A model that answers with the message verbatim (or fails) leaves the
    /// deterministic subject in place: an echoed message is not a title.
    #[test]
    fn an_echoed_prompt_or_a_failed_call_is_not_a_title() {
        let runtime = tokio::runtime::Runtime::new().expect("build runtime");
        let echo = RecordingProvider::answering("Arata un text colorat in chat");
        assert_eq!(
            runtime.block_on(generate_title(
                echo.as_ref(),
                "title-test",
                "title-model",
                "Arata un text colorat in chat",
            )),
            None,
            "the message handed back is not a subject"
        );
        let failed = RecordingProvider::new(Err("no route".to_string()));
        assert_eq!(
            runtime.block_on(generate_title(
                failed.as_ref(),
                "title-test",
                "title-model",
                "Arata un text colorat in chat",
            )),
            None
        );
    }

    fn placeholder_conversation(desktop: &DesktopState) -> String {
        desktop
            .create_chat(NEW_CONVERSATION_TITLE, None, None)
            .expect("a conversation nobody has named")
    }

    /// The generated subject replaces the app-owned fallback and reaches the
    /// store; a title the user typed is never taken back.
    #[test]
    fn a_generated_subject_lands_in_the_store_over_the_fallback() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        );
        let runtime = tokio::runtime::Runtime::new().expect("build runtime");
        let provider = RecordingProvider::answering("Colorful chat text");
        let prompt = "Arata un text colorat in chat";

        let chat = placeholder_conversation(&desktop);
        desktop
            .set_chat_title(&chat, &deterministic_subject(prompt).expect("the fallback"))
            .expect("write the fallback");
        let title = runtime
            .block_on(generate_and_store_title(
                &desktop,
                provider.as_ref(),
                "title-test",
                "title-model",
                &chat,
                prompt,
            ))
            .expect("the generated subject");
        assert_eq!(title, "Colorful chat text");
        assert_eq!(desktop.list_chats()[0].title, "Colorful chat text");

        // A user's name is not the app's: no generation is started over it.
        let named = desktop
            .create_chat("Planul de lansare", None, None)
            .expect("a conversation the user named");
        assert_eq!(
            runtime.block_on(generate_and_store_title(
                &desktop,
                provider.as_ref(),
                "title-test",
                "title-model",
                &named,
                prompt,
            )),
            None
        );
        assert_eq!(
            desktop
                .list_chats()
                .into_iter()
                .find(|chat| chat.id == named)
                .map(|chat| chat.title),
            Some("Planul de lansare".to_string())
        );
    }

    /// With no provider configured nothing is requested and the deterministic
    /// subject written by the prompt pass stands — a subject, not the message.
    #[test]
    fn no_provider_keeps_the_deterministic_subject() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let chat = placeholder_conversation(&desktop);
        let prompt = "Arata un text colorat in chat";
        let fallback = deterministic_subject(prompt).expect("the fallback");
        desktop
            .set_chat_title(&chat, &fallback)
            .expect("write the fallback");

        assert!(
            !spawn_title_generation(&desktop, &chat, prompt, "openai", "gpt-4o-mini"),
            "a machine with no key requests no title"
        );
        assert_eq!(desktop.list_chats()[0].title, fallback);
        assert_ne!(desktop.list_chats()[0].title, prompt);
    }
}
