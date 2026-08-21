use goble_desktop_native::app::GobleApp;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let app = GobleApp::new()?;
    app.run()
}
