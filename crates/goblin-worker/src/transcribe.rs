//! Speech to text for the phone's dictation bar.
//!
//! The phone records short segments and sends them over the relay; this module
//! asks the provider the client names for the words in them. The key lives here,
//! in the worker's vault, so the phone never holds a speech key: a `voice_api_key`
//! whose provider matches wins, and an `llm_api_key` for the same provider is
//! the fallback (the OpenAI key transcribes with the OpenAI models).
//!
//! Both supported providers want a `multipart/form-data` upload. The workspace's
//! `reqwest` is built without the `multipart` feature, so the body is written
//! here: a text part naming the model and a file part carrying the bytes.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context};
use goble_core::secret::Secret;
use serde_json::Value;

/// Largest segment accepted from a client, after base64 decoding. A live
/// dictation segment is a few seconds of speech — tens of kilobytes — so this
/// is a guard against a malformed upload rather than a size the app approaches.
pub const MAX_AUDIO_BYTES: usize = 10 * 1024 * 1024;

/// How long one transcription may take before it is given up on.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Whether the worker can ask `provider` for a transcript. The phone offers
/// dictation for the providers listed as able to transcribe; the worker is the
/// side that has to know how to call them.
pub fn supports_transcription(provider: &str) -> bool {
    matches!(normalize(provider).as_str(), "openai" | "elevenlabs")
}

/// Transcribe one recorded segment with the provider's default endpoint (or the
/// one the environment overrides it with).
pub async fn transcribe(
    secrets: &HashMap<String, Secret>,
    provider: &str,
    model: &str,
    mime: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<String> {
    transcribe_at(secrets, provider, model, mime, bytes, None).await
}

/// Transcribe `bytes` and return the words the provider heard. `endpoint` is an
/// explicit base URL (tests name a mock server); `None` reads the provider's
/// environment override and then its public endpoint.
pub async fn transcribe_at(
    secrets: &HashMap<String, Secret>,
    provider: &str,
    model: &str,
    mime: &str,
    bytes: Vec<u8>,
    endpoint: Option<&str>,
) -> anyhow::Result<String> {
    if bytes.is_empty() {
        bail!("the recording was empty");
    }
    if bytes.len() > MAX_AUDIO_BYTES {
        bail!(
            "the recording is {} bytes; the limit is {}",
            bytes.len(),
            MAX_AUDIO_BYTES
        );
    }
    let provider = normalize(provider);
    if !supports_transcription(&provider) {
        bail!(
            "this worker has no transcription for {}: it can ask openai and elevenlabs",
            if provider.is_empty() {
                "the unnamed provider"
            } else {
                provider.as_str()
            }
        );
    }
    let key = provider_key(secrets, &provider).ok_or_else(|| {
        anyhow::anyhow!(
            "no {provider} key on this worker: add a voice_api_key for {provider} to its vault, \
             or point the phone at a provider it holds a key for"
        )
    })?;
    let model = model.trim();
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("build the transcription client")?;
    let boundary = format!("goble-{}", uuid::Uuid::new_v4().simple());

    let response = match provider.as_str() {
        "openai" => {
            let base = endpoint
                .map(str::to_string)
                .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
                .unwrap_or_else(|| "https://api.openai.com".to_string());
            let url = format!("{}/v1/audio/transcriptions", base.trim_end_matches('/'));
            let body = multipart_body(
                &boundary,
                &[
                    Field::text("model", model_or(model, "gpt-4o-mini-transcribe")),
                    Field::file("file", &filename(mime), mime, &bytes),
                ],
            );
            client
                .post(url)
                .header("authorization", format!("Bearer {key}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body)
                .send()
                .await
        }
        "elevenlabs" => {
            let base = endpoint
                .map(str::to_string)
                .or_else(|| std::env::var("ELEVENLABS_BASE_URL").ok())
                .unwrap_or_else(|| "https://api.elevenlabs.io".to_string());
            let url = format!("{}/v1/speech-to-text", base.trim_end_matches('/'));
            let body = multipart_body(
                &boundary,
                &[
                    Field::text("model_id", model_or(model, "scribe_v1")),
                    Field::file("file", &filename(mime), mime, &bytes),
                ],
            );
            client
                .post(url)
                .header("xi-api-key", key)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body)
                .send()
                .await
        }
        // `supports_transcription` answered above, so this arm is unreachable;
        // it keeps the match total without a panic.
        other => bail!("this worker has no transcription for {other}"),
    }
    .context("reach the transcription provider")?;

    read_transcript(response).await
}

/// The transcript in a provider's answer, or the provider's own refusal. Both
/// providers answer `{"text": "..."}` on success.
async fn read_transcript(response: reqwest::Response) -> anyhow::Result<String> {
    let status = response.status();
    let body = response.text().await.context("read the provider's answer")?;
    if !status.is_success() {
        let detail = body.trim();
        let detail: String = detail.chars().take(300).collect();
        bail!("the provider refused the recording ({status}): {detail}");
    }
    let snippet: String = body.chars().take(300).collect();
    let value: Value = serde_json::from_str(&body)
        .with_context(|| format!("the provider's answer is not JSON: {snippet}"))?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("the provider's answer carries no text"))?;
    Ok(text.trim().to_string())
}

/// A key for `provider`: the speech key when the vault has one, else the LLM
/// key for the same provider. A key issued for another provider is never used.
fn provider_key(secrets: &HashMap<String, Secret>, provider: &str) -> Option<String> {
    let named = |name: &str| {
        secrets
            .values()
            .find(|secret| {
                secret.name == name && secret.provider.eq_ignore_ascii_case(provider)
            })
            .and_then(|secret| String::from_utf8(secret.encrypted_value.clone()).ok())
    };
    named("voice_api_key").or_else(|| named("llm_api_key"))
}

fn normalize(provider: &str) -> String {
    provider.trim().to_lowercase()
}

/// The model to name, or the provider's default when the phone named none.
fn model_or<'a>(model: &'a str, fallback: &'a str) -> &'a str {
    if model.is_empty() {
        fallback
    } else {
        model
    }
}

/// The file name the provider reads the container from. The extension is the
/// one part of the upload a provider can refuse over, so it follows `mime`.
fn filename(mime: &str) -> String {
    let extension = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .strip_prefix("audio/")
        .unwrap_or("webm");
    let extension = match extension {
        "mpeg" | "mp3" => "mp3".to_string(),
        "mp4" | "m4a" | "x-m4a" => "m4a".to_string(),
        "wave" | "x-wav" => "wav".to_string(),
        "ogg" | "opus" => "ogg".to_string(),
        other => {
            let cleaned: String = other
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .take(8)
                .collect();
            if cleaned.is_empty() {
                "webm".to_string()
            } else {
                cleaned
            }
        }
    };
    format!("audio.{extension}")
}

/// One part of the multipart body.
enum Field<'a> {
    Text(&'a str, &'a str),
    File {
        name: &'a str,
        filename: &'a str,
        mime: &'a str,
        bytes: &'a [u8],
    },
}

impl<'a> Field<'a> {
    fn text(name: &'a str, value: &'a str) -> Self {
        Self::Text(name, value)
    }

    fn file(name: &'a str, filename: &'a str, mime: &'a str, bytes: &'a [u8]) -> Self {
        Self::File {
            name,
            filename,
            mime,
            bytes,
        }
    }
}

/// A `multipart/form-data` body: one part per field, closed by the boundary.
fn multipart_body(boundary: &str, fields: &[Field<'_>]) -> Vec<u8> {
    let mut body = Vec::new();
    for field in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match field {
            Field::Text(name, value) => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
            }
            Field::File {
                name,
                filename,
                mime,
                bytes,
            } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(format!("Content-Type: {mime}\r\n\r\n").as_bytes());
                body.extend_from_slice(bytes);
            }
        }
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn vault() -> HashMap<String, Secret> {
        let mut secrets = HashMap::new();
        for secret in [
            Secret::new("llm_api_key", "openai", b"sk-llm".to_vec()),
            Secret::new("voice_api_key", "openai", b"sk-voice".to_vec()),
            Secret::new("voice_api_key", "elevenlabs", b"el-key".to_vec()),
        ] {
            secrets.insert(secret.id.clone(), secret);
        }
        secrets
    }

    #[test]
    fn the_speech_key_wins_and_only_for_its_own_provider() {
        let secrets = vault();
        assert_eq!(provider_key(&secrets, "openai").as_deref(), Some("sk-voice"));
        assert_eq!(
            provider_key(&secrets, "elevenlabs").as_deref(),
            Some("el-key")
        );
        assert_eq!(
            provider_key(&secrets, "anthropic"),
            None,
            "a key issued for another provider is never used"
        );
    }

    #[test]
    fn the_audio_extension_follows_the_container() {
        assert_eq!(filename("audio/webm"), "audio.webm");
        assert_eq!(filename("audio/webm;codecs=opus"), "audio.webm");
        assert_eq!(filename("audio/m4a"), "audio.m4a");
        assert_eq!(filename("audio/mp4"), "audio.m4a");
        assert_eq!(filename("audio/mpeg"), "audio.mp3");
        assert_eq!(filename(""), "audio.webm");
    }

    #[test]
    fn the_body_carries_the_model_and_the_audio() {
        let bytes = b"RIFF....".to_vec();
        let body = multipart_body(
            "goble-test",
            &[
                Field::text("model", "whisper-1"),
                Field::file("file", "audio.wav", "audio/wav", &bytes),
            ],
        );
        let body = String::from_utf8_lossy(&body);
        assert!(body.starts_with("--goble-test\r\n"));
        assert!(body.contains("name=\"model\"\r\n\r\nwhisper-1\r\n"));
        assert!(body.contains("filename=\"audio.wav\""));
        assert!(body.contains("Content-Type: audio/wav\r\n\r\nRIFF...."));
        assert!(body.ends_with("--goble-test--\r\n"));
    }

    #[tokio::test]
    async fn a_missing_key_names_the_provider_it_would_need() {
        let error = transcribe_at(
            &HashMap::new(),
            "openai",
            "whisper-1",
            "audio/webm",
            b"segment".to_vec(),
            None,
        )
        .await
        .expect_err("no key, no transcript");
        let message = error.to_string();
        assert!(message.contains("voice_api_key for openai"), "{message}");
    }

    #[tokio::test]
    async fn openai_answers_with_its_transcript() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .and(header_exists("authorization"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": " hello "})),
            )
            .mount(&server)
            .await;

        let transcript = transcribe_at(
            &vault(),
            "openai",
            "gpt-4o-mini-transcribe",
            "audio/webm",
            b"segment".to_vec(),
            Some(&server.uri()),
        )
        .await
        .expect("the provider answers");
        assert_eq!(transcript, "hello");
    }

    #[tokio::test]
    async fn a_refusal_keeps_the_providers_words() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/audio/transcriptions"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": {"message": "bad audio"}})),
            )
            .mount(&server)
            .await;

        let error = transcribe_at(
            &vault(),
            "openai",
            "whisper-1",
            "audio/webm",
            b"segment".to_vec(),
            Some(&server.uri()),
        )
        .await
        .expect_err("a refusal is an error");
        let message = error.to_string();
        assert!(message.contains("400"), "{message}");
        assert!(message.contains("bad audio"), "{message}");
    }

    #[tokio::test]
    async fn a_provider_without_a_transcription_route_says_so() {
        let error = transcribe_at(
            &vault(),
            "cartesia",
            "sonic-2",
            "audio/webm",
            b"segment".to_vec(),
            None,
        )
        .await
        .expect_err("cartesia cannot transcribe");
        assert!(error.to_string().contains("no transcription for cartesia"));
    }
}
