use thiserror::Error;

/// What went wrong while talking to a sandbox provider.
#[derive(Debug, Error)]
pub enum SandboxError {
    /// The config table does not carry a credential the provider requires.
    #[error("missing credential for {provider}: set {key}")]
    MissingCredential {
        provider: &'static str,
        key: &'static str,
    },

    /// The provider answered with a non-success HTTP status.
    #[error("{provider} returned HTTP {status}: {body}")]
    Http {
        provider: &'static str,
        status: u16,
        body: String,
    },

    /// A local program (the Modal bridge or the Docker CLI) failed.
    #[error("{command} failed: {message}")]
    Command { command: String, message: String },

    /// A provider response did not have the shape the adapter expects.
    #[error("could not parse {context}: {message}")]
    Parse {
        context: &'static str,
        message: String,
    },

    /// The configured provider has no adapter yet.
    #[error("{provider} is not supported: {message}")]
    Unsupported { provider: String, message: String },

    /// The HTTP request never got an answer (DNS, TLS, connection refused).
    #[error("{provider} request failed: {message}")]
    Transport {
        provider: &'static str,
        message: String,
    },

    /// The request asks for something the provider's API cannot express.
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
}
