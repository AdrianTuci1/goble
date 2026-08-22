use goble_desktop_native::app::GobleApp;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Install the ring-backed rustls crypto provider once for the process.
    // goble-desktop-service and its LLM clients require this.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let app = GobleApp::new()?;
    app.run()
}
