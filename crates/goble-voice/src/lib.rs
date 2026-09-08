//! Model-facing voice capability.
//!
//! This crate is the `types` layer of a `types <- protocol <- runtime` split
//! for voice. It owns model-shaped data and the provider seam: an [`AudioChunk`],
//! the [`StreamingSttEvent`] stream the GUI renders, STT/TTS client facades
//! ([`SttClient`], [`TtsClient`]), and an object-safe [`VoiceProvider`] trait
//! with a [`MockProvider`] for tests. There is no native audio backend, no
//! network, and no dependency on `goble-core` or `app/`.
//!
//! Native audio capture/playback would pull in platform backends, so it is gated
//! behind the `audio` feature (off by default). The model types always compile;
//! only the PCM interchange helpers are optional.

use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

/// A chunk of mono audio samples.
///
/// Model-shaped: `samples` are normalized floats in `[-1.0, 1.0]`. Whether they
/// come from a microphone or a file is a concern of a provider implementation,
/// not of this layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioChunk {
    /// Sample rate in Hz (e.g. 16000).
    pub sample_rate: u32,
    /// Mono samples, normalized to `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
}

impl AudioChunk {
    /// A new chunk from any `Vec<f32>`-like collection of samples.
    pub fn new(sample_rate: u32, samples: impl Into<Vec<f32>>) -> Self {
        Self {
            sample_rate,
            samples: samples.into(),
        }
    }

    /// A chunk of `frames` silent samples.
    pub fn silence(sample_rate: u32, frames: usize) -> Self {
        Self::new(sample_rate, vec![0.0; frames])
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Duration of the chunk in whole milliseconds.
    pub fn duration_ms(&self) -> u64 {
        (self.samples.len() as u64 * 1000) / (self.sample_rate.max(1) as u64)
    }
}

/// An event emitted by a streaming speech-to-text session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamingSttEvent {
    /// A provisional recognition of the current utterance.
    Partial {
        text: String,
    },
    /// A settled recognition: the final transcript for an utterance.
    Final {
        text: String,
    },
    /// The session failed; `message` is a human-readable description.
    Error {
        message: String,
    },
}

impl StreamingSttEvent {
    pub fn partial(text: impl Into<String>) -> Self {
        Self::Partial { text: text.into() }
    }

    pub fn final_text(text: impl Into<String>) -> Self {
        Self::Final { text: text.into() }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error { message: message.into() }
    }

    /// The recognized text, if this event carries any (`Partial` / `Final`).
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Partial { text } | Self::Final { text } => Some(text),
            Self::Error { .. } => None,
        }
    }

    /// Whether this event settled an utterance.
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Final { .. })
    }

    /// Whether this event is a terminal failure.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// Configuration for a speech-to-text session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SttConfig {
    /// ISO-639-1 language hint (e.g. "en").
    pub language: Option<String>,
    /// Expected sample rate of incoming audio, in Hz.
    pub sample_rate: u32,
    /// How many alternative transcripts to request, if the provider supports it.
    pub max_alternatives: Option<usize>,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            language: None,
            sample_rate: 16_000,
            max_alternatives: None,
        }
    }
}

impl SttConfig {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            ..Self::default()
        }
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn with_max_alternatives(mut self, n: usize) -> Self {
        self.max_alternatives = Some(n);
        self
    }
}

/// A settled recognition with its availability timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    /// Provider-reported confidence in `[0.0, 1.0]`, if known.
    pub confidence: Option<f32>,
    pub created_at: DateTime<Utc>,
}

impl Transcript {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            confidence: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }
}

/// Configuration for a text-to-speech synthesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Voice to speak in, if the provider supports named voices.
    pub voice: Option<String>,
    /// Sample rate to synthesize at, in Hz.
    pub sample_rate: u32,
    /// Playback speed multiplier (`1.0` is natural).
    pub speed: Option<f32>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            voice: None,
            sample_rate: 24_000,
            speed: None,
        }
    }
}

impl TtsConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_voice(mut self, voice: impl Into<String>) -> Self {
        self.voice = Some(voice.into());
        self
    }

    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = Some(speed);
        self
    }
}

/// Errors surfaced by the STT/TTS clients.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceError {
    #[error("audio chunk is empty; refused to send to the provider")]
    EmptyAudioChunk,
    #[error("text is empty; refused to synthesize")]
    EmptyText,
    #[error("provider produced no audio")]
    EmptySynthesis,
}

/// The seam between the model layer and the voice backend.
///
/// Object-safe: implementors get returned as `Arc<dyn VoiceProvider>` and the
/// clients only ever see the trait. A default [`VoiceProvider::stt_stream`]
/// turns a non-streaming `stt` implementation's transcripts into a [`Final`]
/// event stream, so a backend only has to implement the three core methods.
#[async_trait::async_trait]
pub trait VoiceProvider: Send + Sync {
    /// A human-readable backend name.
    fn name(&self) -> &str;

    /// Transcribe `audio` into one or more settled transcripts.
    async fn stt(&self, audio: &AudioChunk) -> Result<Vec<Transcript>>;

    /// Stream recognition events for `audio`.
    ///
    /// The default wraps [`VoiceProvider::stt`]'s results as [`Final`] events;
    /// override it to surface `Partial` / `Error` events.
    async fn stt_stream(
        &self,
        audio: &AudioChunk,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamingSttEvent> + Send>>> {
        let transcripts = self.stt(audio).await?;
        let events: Vec<_> = transcripts
            .into_iter()
            .map(|t| StreamingSttEvent::final_text(t.text))
            .collect();
        Ok(Box::pin(futures::stream::iter(events)))
    }

    /// Synthesize `text` into audio chunks, optionally with a named `voice`.
    async fn tts(&self, text: &str, voice: Option<&str>) -> Result<Vec<AudioChunk>>;
}

/// A configurable [`VoiceProvider`] for tests and offline use.
pub struct MockProvider {
    name: String,
    transcripts: Vec<Transcript>,
    stream_events: Vec<StreamingSttEvent>,
    audio: Vec<AudioChunk>,
}

impl MockProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            transcripts: Vec::new(),
            stream_events: Vec::new(),
            audio: Vec::new(),
        }
    }

    /// Add a transcript returned by [`MockProvider::stt`].
    pub fn with_stt(mut self, text: impl Into<String>) -> Self {
        self.transcripts.push(Transcript::new(text));
        self
    }

    /// Add a transcript with an explicit confidence.
    pub fn with_stt_confidence(mut self, text: impl Into<String>, confidence: f32) -> Self {
        self.transcripts
            .push(Transcript::new(text).with_confidence(confidence));
        self
    }

    /// Set the exact event sequence returned by [`MockProvider::stt_stream`].
    pub fn with_stream_events(mut self, events: Vec<StreamingSttEvent>) -> Self {
        self.stream_events = events;
        self
    }

    /// Add an audio chunk returned by [`MockProvider::tts`].
    pub fn with_tts(mut self, chunk: AudioChunk) -> Self {
        self.audio.push(chunk);
        self
    }
}

#[async_trait::async_trait]
impl VoiceProvider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn stt(&self, _audio: &AudioChunk) -> Result<Vec<Transcript>> {
        Ok(self.transcripts.clone())
    }

    async fn stt_stream(
        &self,
        _audio: &AudioChunk,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamingSttEvent> + Send>>> {
        Ok(Box::pin(futures::stream::iter(self.stream_events.clone())))
    }

    async fn tts(&self, _text: &str, _voice: Option<&str>) -> Result<Vec<AudioChunk>> {
        Ok(self.audio.clone())
    }
}

/// An in-flight streaming STT session that yields [`StreamingSttEvent`]s.
pub struct StreamingStt {
    stream: Pin<Box<dyn Stream<Item = StreamingSttEvent> + Send>>,
}

impl StreamingStt {
    /// Wrap an existing event stream.
    pub fn from_stream(stream: Pin<Box<dyn Stream<Item = StreamingSttEvent> + Send>>) -> Self {
        Self { stream }
    }

    /// Build a session that yields exactly `events`, in order.
    ///
    /// Useful for tests and for replaying a recorded transcript.
    pub fn from_events(events: Vec<StreamingSttEvent>) -> Self {
        Self::from_stream(Box::pin(futures::stream::iter(events)))
    }

    /// Take the next event, or `None` when the session is exhausted.
    pub async fn next_event(&mut self) -> Option<StreamingSttEvent> {
        self.stream.next().await
    }

    /// Consume the session, collecting every remaining event.
    pub async fn collect(self) -> Vec<StreamingSttEvent> {
        self.stream.collect().await
    }
}

/// A speech-to-text client: transcribes audio into text through a [`VoiceProvider`].
#[derive(Clone)]
pub struct SttClient {
    provider: Arc<dyn VoiceProvider>,
    config: SttConfig,
}

impl SttClient {
    pub fn new(provider: Arc<dyn VoiceProvider>, config: SttConfig) -> Self {
        Self { provider, config }
    }

    pub fn config(&self) -> &SttConfig {
        &self.config
    }

    pub fn provider(&self) -> &dyn VoiceProvider {
        self.provider.as_ref()
    }

    /// Transcribe a chunk of audio into settled transcripts.
    ///
    /// Empty audio is rejected locally rather than forwarded to the provider.
    pub async fn transcribe(&self, audio: &AudioChunk) -> Result<Vec<Transcript>> {
        if audio.is_empty() {
            return Err(VoiceError::EmptyAudioChunk.into());
        }
        self.provider.stt(audio).await
    }

    /// Start a streaming recognition session for `audio`.
    pub async fn stream(&self, audio: &AudioChunk) -> Result<StreamingStt> {
        if audio.is_empty() {
            return Err(VoiceError::EmptyAudioChunk.into());
        }
        let stream = self.provider.stt_stream(audio).await?;
        Ok(StreamingStt::from_stream(stream))
    }
}

/// A text-to-speech client: synthesizes text into audio through a [`VoiceProvider`].
#[derive(Clone)]
pub struct TtsClient {
    provider: Arc<dyn VoiceProvider>,
    config: TtsConfig,
}

impl TtsClient {
    pub fn new(provider: Arc<dyn VoiceProvider>, config: TtsConfig) -> Self {
        Self { provider, config }
    }

    pub fn config(&self) -> &TtsConfig {
        &self.config
    }

    /// Synthesize `text` into audio chunks using the client's configured voice.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<AudioChunk>> {
        self.synthesize_with_voice(text, self.config.voice.as_deref())
            .await
    }

    /// Synthesize `text` with an explicit voice, overriding the config.
    pub async fn synthesize_with_voice(&self, text: &str, voice: Option<&str>) -> Result<Vec<AudioChunk>> {
        if text.is_empty() {
            return Err(VoiceError::EmptyText.into());
        }
        let chunks = self.provider.tts(text, voice).await?;
        if chunks.is_empty() {
            return Err(VoiceError::EmptySynthesis.into());
        }
        Ok(chunks)
    }
}

#[cfg(feature = "audio")]
mod audio {
    //! PCM interchange helpers. No native audio backend is used here — these
    //! convert between normalized `f32` chunks and little-endian i16 PCM so a
    //! capture/playback layer can talk to the outside world.

    use super::AudioChunk;

    /// Encode a chunk as little-endian i16 PCM (16-bit signed mono).
    pub fn to_i16_pcm(chunk: &AudioChunk) -> Vec<u8> {
        chunk
            .samples
            .iter()
            .map(|&s| {
                ((s * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32)) as i16
            })
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }

    /// Decode little-endian i16 PCM bytes back into an [`AudioChunk`].
    pub fn from_i16_pcm(sample_rate: u32, bytes: &[u8]) -> AudioChunk {
        let samples: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
            .collect();
        AudioChunk::new(sample_rate, samples)
    }
}

#[cfg(feature = "audio")]
pub use audio::{from_i16_pcm, to_i16_pcm};

#[cfg(test)]
mod tests {
    use super::*;

    fn mock() -> MockProvider {
        MockProvider::new("mock")
            .with_stt_confidence("hello world", 0.92)
            .with_stream_events(vec![
                StreamingSttEvent::partial("hello"),
                StreamingSttEvent::final_text("hello world"),
            ])
            .with_tts(AudioChunk::silence(16_000, 8))
    }

    #[test]
    fn audio_chunk_helpers() {
        let chunk = AudioChunk::silence(16_000, 160);
        assert_eq!(chunk.len(), 160);
        assert!(!chunk.is_empty());
        assert_eq!(chunk.duration_ms(), 10);
        assert!(AudioChunk::new(16_000, vec![]).is_empty());
    }

    #[test]
    fn stt_event_roundtrip_and_helpers() {
        for event in [
            StreamingSttEvent::partial("hel"),
            StreamingSttEvent::final_text("hello"),
            StreamingSttEvent::error("boom"),
        ] {
            let json = serde_json::to_string(&event).unwrap();
            let decoded: StreamingSttEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, event);
        }

        assert_eq!(StreamingSttEvent::final_text("x").text(), Some("x"));
        assert!(StreamingSttEvent::final_text("x").is_final());
        assert!(StreamingSttEvent::partial("x").is_final() == false);
        assert_eq!(StreamingSttEvent::error("e").text(), None);
        assert!(StreamingSttEvent::error("e").is_error());
    }

    #[tokio::test]
    async fn streaming_stt_yields_events() {
        let events = vec![
            StreamingSttEvent::partial("a"),
            StreamingSttEvent::final_text("ab"),
        ];
        let mut session = StreamingStt::from_events(events.clone());

        let mut got = Vec::new();
        while let Some(ev) = session.next_event().await {
            got.push(ev);
        }
        assert_eq!(got, events);
    }

    #[tokio::test]
    async fn streaming_stt_collect_drains() {
        let session = StreamingStt::from_events(vec![
            StreamingSttEvent::partial("a"),
            StreamingSttEvent::error("boom"),
        ]);
        let events = session.collect().await;
        assert_eq!(events.len(), 2);
        assert!(events[1].is_error());
    }

    #[tokio::test]
    async fn stt_client_transcribes_via_provider() {
        let client = SttClient::new(Arc::new(mock()), SttConfig::default());
        let out = client
            .transcribe(&AudioChunk::silence(16_000, 8))
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello world");
        assert_eq!(out[0].confidence, Some(0.92));
    }

    #[tokio::test]
    async fn stt_client_rejects_empty_audio() {
        let client = SttClient::new(Arc::new(mock()), SttConfig::default());
        let err = client
            .transcribe(&AudioChunk::new(16_000, vec![]))
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<VoiceError>(),
            Some(VoiceError::EmptyAudioChunk)
        ));

        let err = match client.stream(&AudioChunk::new(16_000, vec![])).await {
            Err(e) => e,
            Ok(_) => panic!("expected an empty-audio error from stream"),
        };
        assert!(matches!(
            err.downcast_ref::<VoiceError>(),
            Some(VoiceError::EmptyAudioChunk)
        ));
    }

    #[tokio::test]
    async fn stt_client_streams_events() {
        let client = SttClient::new(Arc::new(mock()), SttConfig::default());
        let session = client
            .stream(&AudioChunk::silence(16_000, 8))
            .await
            .unwrap();
        let events = session.collect().await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].text(), Some("hello"));
        assert!(events[1].is_final());
    }

    #[tokio::test]
    async fn tts_client_synthesizes_audio() {
        let client = TtsClient::new(Arc::new(mock()), TtsConfig::new());
        let chunks = client.synthesize("hi").await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].sample_rate, 16_000);
    }

    #[tokio::test]
    async fn tts_client_rejects_empty_text() {
        let client = TtsClient::new(Arc::new(mock()), TtsConfig::new());
        let err = client.synthesize("").await.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<VoiceError>(),
            Some(VoiceError::EmptyText)
        ));
    }

    #[test]
    fn voice_provider_is_object_safe() {
        let provider: Arc<dyn VoiceProvider> = Arc::new(mock());
        assert_eq!(provider.name(), "mock");
    }

    #[test]
    fn config_builders() {
        let stt = SttConfig::new(8_000)
            .with_language("en")
            .with_max_alternatives(3);
        assert_eq!(stt.sample_rate, 8_000);
        assert_eq!(stt.language.as_deref(), Some("en"));
        assert_eq!(stt.max_alternatives, Some(3));

        let tts = TtsConfig::new().with_voice("alice").with_speed(1.5);
        assert_eq!(tts.voice.as_deref(), Some("alice"));
        assert_eq!(tts.speed, Some(1.5));
    }

    #[cfg(feature = "audio")]
    #[test]
    fn i16_pcm_roundtrip() {
        let chunk = AudioChunk::new(16_000, vec![0.0, 0.5, -0.5, 1.0, -1.0]);
        let bytes = to_i16_pcm(&chunk);
        let decoded = from_i16_pcm(16_000, &bytes);
        assert_eq!(decoded.sample_rate, chunk.sample_rate);
        assert_eq!(decoded.len(), chunk.len());
        // Close within one quantization step.
        for (a, b) in chunk.samples.iter().zip(decoded.samples.iter()) {
            assert!((a - b).abs() < 1.0 / i16::MAX as f32 + 1e-6);
        }
    }
}
