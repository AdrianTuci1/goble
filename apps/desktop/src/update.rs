//! The update checker the app runs beside the UI, and the model the Settings →
//! Updates page draws.
//!
//! Nothing here touches the element tree. [`UpdateService`] owns the network
//! side — a check at startup and then at most every [`CHECK_INTERVAL`], on its
//! own thread — and publishes what it found as a [`UpdateView`]. The UI reads
//! that view once per frame and sends two commands back: check now, and install
//! what was offered.
//!
//! # Trust
//!
//! An offer is only shown when the manifest signature verified against the key
//! this build was given ([`goble_update::UpdateClient`]); a release this build
//! cannot authenticate is an error in the section, never a download. The
//! download itself is hashed while it streams and is only kept when the digest
//! and the size match the signed manifest, so a staging directory never holds
//! bytes nobody checked.
//!
//! # State machine
//!
//! ```text
//! Idle -> Checking -> Available -> Downloading -> Ready
//!              |          |            |
//!              +----------+------------+--> Error
//! ```
//!
//! Every transition is a method on [`UpdateViewModel`] that returns whether it
//! was allowed, so a late answer from a thread that was overtaken cannot move
//! the UI backwards.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use goble_update::{
    AvailableUpdate, Channel, CheckOutcome, InstallContext, InstallError, InstallOutcome,
    InstallRequest, UpdateClient, UpdateConfig, UpdateError, UpdatePaths, UpdateState,
};

/// A check at startup and then at most this often.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// How often the checker thread wakes to see whether a check is due. It costs
/// an atomic load and a timestamp comparison.
const CHECK_TICK: Duration = Duration::from_secs(60);

/// How long a freshly installed build must stay up before it is healthy. The
/// value lives in `goble-update`, which owns the marker files.
pub const HEALTHY_AFTER: Duration = goble_update::HEALTHY_AFTER;

/// Where an update is staged before it is installed.
fn staging_dir(paths: &UpdatePaths) -> PathBuf {
    paths.downloads_dir()
}

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

/// What the Updates section is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePhase {
    /// Nothing has been asked yet in this session.
    Idle,
    Checking,
    Available,
    Downloading,
    /// Downloaded, verified, installed; the app is about to restart.
    Ready,
    Error,
}

impl UpdatePhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Not checked yet",
            Self::Checking => "Checking…",
            Self::Available => "Update available",
            Self::Downloading => "Downloading…",
            Self::Ready => "Ready to restart",
            Self::Error => "Update failed",
        }
    }

    /// Whether a check may start from here.
    fn allows_check(self) -> bool {
        matches!(self, Self::Idle | Self::Available | Self::Ready | Self::Error)
    }

    /// Whether an install may start from here.
    fn allows_install(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// An offered release, in the terms the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableSummary {
    pub version: String,
    pub epoch: u64,
    pub channel: String,
    pub size: u64,
    /// `true` when installing moves to an older version: the publisher
    /// withdrew what is running.
    pub rollback: bool,
    /// Whether the running version is on the release's yanked list.
    pub current_yanked: bool,
    pub file_name: String,
}

impl AvailableSummary {
    pub fn from_update(update: &AvailableUpdate) -> Self {
        let file_name = update
            .artifact
            .url
            .rsplit('/')
            .next()
            .unwrap_or("update")
            .to_string();
        Self {
            version: update.version.to_string(),
            epoch: update.epoch,
            channel: update.channel.as_str().to_string(),
            size: update.artifact.size,
            rollback: update.rollback,
            current_yanked: update.current_yanked,
            file_name,
        }
    }

    /// `12.3 MB`, for a size the page can say out loud.
    pub fn size_label(&self) -> String {
        human_bytes(self.size)
    }

    /// `0.2.0 · stable · 12.3 MB`.
    pub fn detail(&self) -> String {
        format!("{} · {} · {}", self.version, self.channel, self.size_label())
    }

    /// Whether this is an ordinary forward update.
    pub fn is_upgrade(&self) -> bool {
        !self.rollback
    }
}

fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KIB {
        return format!("{bytes} B");
    }
    if value < KIB * KIB {
        return format!("{:.1} KB", value / KIB);
    }
    if value < KIB * KIB * KIB {
        return format!("{:.1} MB", value / (KIB * KIB));
    }
    format!("{:.1} GB", value / (KIB * KIB * KIB))
}

/// The banner a withdrawn running version earns: it names the replacement and
/// offers the install that goes back to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YankedBanner {
    pub running: String,
    pub replacement: String,
    /// The line under the heading.
    pub detail: String,
}

/// Everything the Updates page draws, refreshed from the service each frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateView {
    pub current_version: String,
    pub channel: String,
    pub installed_epoch: u64,
    /// The last check, as a relative line (`checked 2h ago`); `None` before
    /// the first check this installation knows about.
    pub last_check: Option<String>,
    pub phase: UpdatePhase,
    /// One line that always says something true about the current phase.
    pub status: String,
    pub available: Option<AvailableSummary>,
    /// The version that was staged and installed, once the app is about to
    /// restart.
    pub ready_version: Option<String>,
    pub error: Option<String>,
    pub banner: Option<YankedBanner>,
    /// The version that was installed and rolled back, from the state file.
    pub failed_version: Option<String>,
}

impl Default for UpdateView {
    fn default() -> Self {
        Self {
            current_version: String::new(),
            channel: String::new(),
            installed_epoch: 0,
            last_check: None,
            phase: UpdatePhase::Idle,
            status: UpdatePhase::Idle.label().to_string(),
            available: None,
            ready_version: None,
            error: None,
            banner: None,
            failed_version: None,
        }
    }
}

impl UpdateView {
    /// Whether the section has an install the user can start.
    pub fn can_install(&self) -> bool {
        self.phase == UpdatePhase::Available && self.available.is_some()
    }

    /// Whether the running version is the one the channel withdrew.
    pub fn is_yanked(&self) -> bool {
        self.banner.is_some()
    }
}

// ---------------------------------------------------------------------------
// The state machine
// ---------------------------------------------------------------------------

/// The update state as the app holds it. Every method returns whether the
/// transition happened, so a caller can tell "started a check" from "a check
/// was already running".
#[derive(Debug, Clone)]
pub struct UpdateViewModel {
    view: UpdateView,
    last_check_at: Option<DateTime<Utc>>,
}

impl UpdateViewModel {
    pub fn new(
        current_version: impl Into<String>,
        channel: impl Into<String>,
        installed_epoch: u64,
    ) -> Self {
        let view = UpdateView {
            current_version: current_version.into(),
            channel: channel.into(),
            installed_epoch,
            ..UpdateView::default()
        };
        Self { view, last_check_at: None }
    }

    pub fn view(&self) -> &UpdateView {
        &self.view
    }

    pub fn phase(&self) -> UpdatePhase {
        self.view.phase
    }

    /// Replay what the state file already knows, without touching the phase.
    pub fn adopt_state(&mut self, state: &UpdateState) {
        self.view.installed_epoch = state.installed_epoch();
        self.view.failed_version = state.failed_version.clone();
        self.view.last_check = state.last_check.map(|at| relative_check(at, Utc::now()));
        self.last_check_at = state.last_check;
    }

    pub fn begin_check(&mut self) -> bool {
        if !self.view.phase.allows_check() {
            return false;
        }
        self.view.phase = UpdatePhase::Checking;
        self.view.status = UpdatePhase::Checking.label().to_string();
        self.view.error = None;
        true
    }

    /// A check came back. `offer` is `None` when there is nothing to install.
    pub fn check_finished(
        &mut self,
        at: DateTime<Utc>,
        offer: Result<Option<AvailableSummary>, String>,
    ) -> bool {
        if self.view.phase != UpdatePhase::Checking {
            return false;
        }
        self.last_check_at = Some(at);
        self.view.last_check = Some(relative_check(at, at));
        match offer {
            Ok(Some(summary)) => {
                self.view.banner = summary.current_yanked.then(|| YankedBanner {
                    running: self.view.current_version.clone(),
                    replacement: summary.version.clone(),
                    detail: format!(
                        "{} was withdrawn from {}. The replacement is {} ({}).",
                        self.view.current_version,
                        self.view.channel,
                        summary.version,
                        summary.size_label()
                    ),
                });
                self.view.status = format!("{} {} is available", summary.channel, summary.version);
                self.view.available = Some(summary);
                self.view.phase = UpdatePhase::Available;
                true
            }
            Ok(None) => {
                self.view.available = None;
                self.view.banner = None;
                self.view.phase = UpdatePhase::Idle;
                self.view.status =
                    format!("Goble {} is the newest {} build", self.view.current_version, self.view.channel);
                true
            }
            Err(reason) => {
                self.fail(reason);
                true
            }
        }
    }

    pub fn begin_download(&mut self) -> bool {
        if !self.view.phase.allows_install() || self.view.available.is_none() {
            return false;
        }
        self.view.phase = UpdatePhase::Downloading;
        self.view.error = None;
        self.view.status = match &self.view.available {
            Some(available) => format!("Downloading {}…", available.detail()),
            None => UpdatePhase::Downloading.label().to_string(),
        };
        true
    }

    /// The stage, digest and install all succeeded: the app is about to
    /// restart into the new version.
    pub fn download_finished(&mut self, version: impl Into<String>) -> bool {
        if self.view.phase != UpdatePhase::Downloading {
            return false;
        }
        let version = version.into();
        self.view.phase = UpdatePhase::Ready;
        self.view.ready_version = Some(version.clone());
        self.view.status = format!("Goble {version} is installed; restart to use it");
        true
    }

    /// Anything that stopped the update where it was.
    pub fn fail(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.view.phase = UpdatePhase::Error;
        self.view.status = UpdatePhase::Error.label().to_string();
        self.view.error = Some(reason);
    }

    /// Forget a finished attempt, keeping what the state file knows.
    pub fn reset(&mut self) {
        if matches!(self.view.phase, UpdatePhase::Checking | UpdatePhase::Downloading) {
            return;
        }
        self.view.phase = UpdatePhase::Idle;
        self.view.status = UpdatePhase::Idle.label().to_string();
        self.view.available = None;
        self.view.ready_version = None;
        self.view.error = None;
    }

    pub fn last_check_at(&self) -> Option<DateTime<Utc>> {
        self.last_check_at
    }

    /// Whether a check is due: the first one of the session, or one that is at
    /// least [`CHECK_INTERVAL`] after the last.
    pub fn check_due(&self, now: DateTime<Utc>) -> bool {
        match self.last_check_at {
            None => true,
            Some(last) => {
                let elapsed = now.signed_duration_since(last);
                elapsed.num_seconds() >= CHECK_INTERVAL.as_secs() as i64
            }
        }
    }
}

/// `checked 2h ago`, `checked just now`.
pub fn relative_check(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = now.signed_duration_since(at).num_seconds().max(0);
    let label = match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86_399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    };
    format!("checked {label}")
}

// ---------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------

/// The check/download threads and the state they publish.
pub struct UpdateService {
    client: Arc<UpdateClient>,
    model: Mutex<Inner>,
    paths: UpdatePaths,
    state_path: PathBuf,
    running_version: String,
    channel: Channel,
    allow_unsigned: bool,
    checking: AtomicBool,
    installing: AtomicBool,
    started: AtomicBool,
}

struct Inner {
    model: UpdateViewModel,
    // The offer as the client described it, kept so the install path downloads
    // exactly what the signature covered.
    offer: Option<AvailableUpdate>,
}

impl std::fmt::Debug for UpdateService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdateService")
            .field("running_version", &self.running_version)
            .field("channel", &self.channel)
            .field("checking", &self.checking.load(Ordering::Relaxed))
            .field("installing", &self.installing.load(Ordering::Relaxed))
            .finish()
    }
}

impl UpdateService {
    /// Build the service from the environment, or `None` when no release host
    /// is configured: a build with nowhere to check does not open sockets.
    ///
    /// `GOBLE_UPDATE_MANIFEST` and `GOBLE_UPDATE_PUBLIC_KEY` are read at run
    /// time; `option_env!` values are the fallback a release build can bake in.
    pub fn from_env(
        running_version: &str,
        channel: Channel,
        installed_epoch: u64,
    ) -> Option<Arc<Self>> {
        let manifest_url = std::env::var("GOBLE_UPDATE_MANIFEST")
            .ok()
            .or_else(|| option_env!("GOBLE_UPDATE_MANIFEST").map(str::to_owned))
            .filter(|url| !url.trim().is_empty())?;
        let public_key = std::env::var("GOBLE_UPDATE_PUBLIC_KEY")
            .ok()
            .or_else(|| option_env!("GOBLE_UPDATE_PUBLIC_KEY").map(str::to_owned))
            .filter(|key| !key.trim().is_empty());
        let allow_unsigned = std::env::var("GOBLE_UPDATE_ALLOW_UNSIGNED").as_deref() == Ok("1");
        let paths = match UpdatePaths::detect() {
            Ok(paths) => paths,
            Err(err) => {
                log::warn!("updates are off for this run: {err}");
                return None;
            }
        };

        let mut config = UpdateConfig::new(manifest_url);
        if let Some(key) = public_key {
            config = config.with_public_key(key);
        }
        config.allow_unsigned = allow_unsigned;

        let client = match UpdateClient::new(config, format!("goble/{running_version}")) {
            Ok(client) => Arc::new(client),
            Err(err) => {
                log::warn!("the update client could not start: {err}");
                return None;
            }
        };

        let state_path = paths.state_path();
        let mut model =
            UpdateViewModel::new(running_version, channel.as_str(), installed_epoch);
        match UpdateState::load(&state_path) {
            Ok(state) => model.adopt_state(&state),
            Err(err) => log::warn!("the update state could not be read: {err}"),
        }

        Some(Arc::new(Self {
            client,
            model: Mutex::new(Inner { model, offer: None }),
            paths,
            state_path,
            running_version: running_version.to_owned(),
            channel,
            allow_unsigned,
            checking: AtomicBool::new(false),
            installing: AtomicBool::new(false),
            started: AtomicBool::new(false),
        }))
    }

    /// The current view, cloned for the frame being built.
    pub fn snapshot(&self) -> UpdateView {
        self.model
            .lock()
            .map(|inner| inner.model.view().clone())
            .unwrap_or_default()
    }

    pub fn paths(&self) -> &UpdatePaths {
        &self.paths
    }

    pub fn channel(&self) -> Channel {
        self.channel
    }

    pub fn running_version(&self) -> &str {
        &self.running_version
    }

    /// Start the startup check and the six-hour clock. Called once.
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        if !self.channel.is_self_updating() {
            log::info!("updates are off for a {} build", self.channel.as_str());
            return;
        }
        let service = Arc::clone(self);
        spawn("goble-update-check", move || loop {
            if service.check_due() {
                service.check_once();
            }
            std::thread::sleep(CHECK_TICK);
        });
        self.start_health_watch();
    }

    fn check_due(&self) -> bool {
        match self.model.lock() {
            Ok(inner) => inner.model.check_due(Utc::now()),
            Err(_) => false,
        }
    }

    /// Check now, from the page's button. Refused while a check or an install
    /// is already running, so two answers cannot race.
    pub fn check_now(self: &Arc<Self>) {
        if !self.mark_checking() {
            return;
        }
        let service = Arc::clone(self);
        spawn("goble-update-check-now", move || {
            service.check_once();
        });
    }

    fn mark_checking(&self) -> bool {
        let mut inner = match self.model.lock() {
            Ok(inner) => inner,
            Err(_) => return false,
        };
        if !inner.model.begin_check() {
            return false;
        }
        drop(inner);
        self.checking.store(true, Ordering::SeqCst);
        true
    }

    /// One check, on the calling thread.
    fn check_once(&self) {
        if self.installing.load(Ordering::SeqCst) {
            return;
        }
        if !self.mark_checking() {
            return;
        }

        let outcome = self.client.check(
            self.channel,
            &self.running_version,
            self.installed_epoch(),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );

        let result = match outcome {
            Ok(CheckOutcome::Available(update)) => {
                if !update.verified && !self.allow_unsigned {
                    Err(
                        "the release is not signed with the key this build trusts, so it was not offered"
                            .to_owned(),
                    )
                } else {
                    let summary = AvailableSummary::from_update(&update);
                    if let Ok(mut inner) = self.model.lock() {
                        inner.offer = Some(*update);
                    }
                    Ok(Some(summary))
                }
            }
            Ok(CheckOutcome::UpToDate { .. }) => Ok(None),
            Ok(CheckOutcome::Unsupported { reason }) => Err(reason),
            Err(err) => Err(check_error(&err)),
        };

        let at = Utc::now();
        if let Ok(mut inner) = self.model.lock() {
            inner.model.check_finished(at, result);
        }
        self.checking.store(false, Ordering::SeqCst);
        self.record_check(at);
        log::info!("update check finished: {}", self.snapshot().status);
    }

    fn installed_epoch(&self) -> u64 {
        self.model.lock().map(|inner| inner.model.view().installed_epoch).unwrap_or(0)
    }

    fn record_check(&self, at: DateTime<Utc>) {
        let mut state = UpdateState::load(&self.state_path).unwrap_or_default();
        state.record_check(at);
        if let Err(err) = state.save(&self.state_path) {
            log::warn!("the update state could not be written: {err}");
        }
    }

    /// Download, verify, stage and install the offered update on a worker
    /// thread. The process exits once the swap is arranged, because an
    /// installer that waits for this process to end has to be given a process
    /// that ends.
    pub fn install_available(self: &Arc<Self>) {
        if self.installing.swap(true, Ordering::SeqCst) {
            return;
        }
        let offer = match self.model.lock() {
            Ok(mut inner) => {
                if !inner.model.begin_download() {
                    self.installing.store(false, Ordering::SeqCst);
                    return;
                }
                inner.offer.clone()
            }
            Err(_) => {
                self.installing.store(false, Ordering::SeqCst);
                return;
            }
        };
        let Some(update) = offer else {
            self.finish_with_error("there is no update to install");
            return;
        };

        let service = Arc::clone(self);
        let started = spawn("goble-update-install", move || {
            service.run_install(update);
            service.installing.store(false, Ordering::SeqCst);
        });
        if !started {
            self.installing.store(false, Ordering::SeqCst);
        }
    }

    fn run_install(&self, update: AvailableUpdate) {
        if !update.verified && !self.allow_unsigned {
            self.finish_with_error(
                "the release is not signed with the key this build trusts, so it was not installed",
            );
            return;
        }

        let directory = staging_dir(&self.paths);
        let staged = match self.client.download(&update, &directory) {
            Ok(staged) => staged,
            Err(err) => {
                self.finish_with_error(download_error(&err));
                return;
            }
        };
        // `download` has hashed the bytes and compared them with the signed
        // manifest; this is the second half of the promise, re-checked here so
        // a bug in staging cannot install unchecked bytes.
        let expected = update.artifact.sha256.to_ascii_lowercase();
        if staged.digest != expected {
            self.finish_with_error(format!(
                "the downloaded file does not match the signature ({expected} expected, {} found)",
                staged.digest
            ));
            return;
        }

        let plan = match goble_update::install::plan_install(&InstallContext::detect(), &staged) {
            Ok(plan) => plan,
            Err(err) => {
                self.finish_with_error(plan_error(&err));
                return;
            }
        };
        let request = InstallRequest::new(
            self.paths.clone(),
            update.version.to_string(),
            update.epoch,
        );
        match plan.apply(&request) {
            Ok(InstallOutcome::Applied { restart_required: true }) => {
                let version = update.version.to_string();
                if let Ok(mut inner) = self.model.lock() {
                    inner.model.download_finished(version.clone());
                }
                log::info!("Goble {version} is installed; restarting to use it");
                // The swap script (macOS) and the installer wait for this
                // process to exit, and the new AppImage replaces this file.
                std::process::exit(0);
            }
            Ok(InstallOutcome::Applied { restart_required: false }) => {
                if let Ok(mut inner) = self.model.lock() {
                    inner.model.download_finished(update.version.to_string());
                }
            }
            Ok(InstallOutcome::NeedsUser { command, hint }) => {
                self.finish_with_error(format!("{hint} Run: {command}"));
            }
            Err(err) => self.finish_with_error(plan_error(&err)),
        }
    }

    fn finish_with_error(&self, reason: impl Into<String>) {
        let reason = reason.into();
        if let Ok(mut inner) = self.model.lock() {
            inner.model.fail(reason.clone());
        }
        log::warn!("update failed: {reason}");
    }

    /// The healthy watch: after [`HEALTHY_AFTER`] of uptime the install counts.
    ///
    /// Until then `~/.goble/updates/pending` stands, and a launch that never
    /// gets here is rolled back by `goble_update::rollback_local()`.
    fn start_health_watch(self: &Arc<Self>) {
        let paths = self.paths.clone();
        let state_path = self.state_path.clone();
        let running = self.running_version.clone();
        spawn("goble-update-healthy", move || {
            std::thread::sleep(HEALTHY_AFTER);
            match confirm_healthy(&paths, &state_path, &running) {
                Ok(Some(version)) => log::info!("Goble {version} stayed up; the install is confirmed"),
                Ok(None) => {}
                Err(err) => log::warn!("the healthy marker could not be written: {err}"),
            }
        });
    }
}

/// Write the healthy marker and record the install it confirms.
///
/// Returns the version confirmed, when the marker cleared a pending install.
pub fn confirm_healthy(
    paths: &UpdatePaths,
    state_path: &std::path::Path,
    running_version: &str,
) -> Result<Option<String>, InstallError> {
    let pending = paths.mark_healthy()?;
    let mut state = UpdateState::load(state_path).map_err(|err| InstallError::State(err.to_string()))?;
    let confirmed = match pending {
        Some(pending) => {
            state.confirm_install(pending.version.clone(), pending.epoch);
            Some(pending.version)
        }
        None => {
            // A first install that never went through a pending marker: record
            // what is running, without inventing an epoch it did not come from.
            if state.installed_version().is_none() {
                state.confirm_install(running_version.to_owned(), 0);
            }
            None
        }
    };
    state.save(state_path).map_err(|err| InstallError::State(err.to_string()))?;
    Ok(confirmed)
}

/// Spawn a named background thread, logging rather than panicking when the
/// machine refuses another thread.
fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    match std::thread::Builder::new().name(name.to_owned()).spawn(work) {
        Ok(_) => true,
        Err(err) => {
            log::warn!("could not start the {name} thread: {err}");
            false
        }
    }
}

/// A check error in the words the page shows.
pub fn check_error(err: &UpdateError) -> String {
    match err {
        UpdateError::Transport { url, .. } => format!("could not reach {url}"),
        UpdateError::Status { url, status } => format!("{url} answered HTTP {status}"),
        UpdateError::Manifest(inner) => format!("the release manifest is unusable: {inner}"),
        UpdateError::UncomparableVersion { version } => {
            format!("this build's version (`{version}`) cannot be compared with the release")
        }
        other => other.to_string(),
    }
}

/// A download error in the words the page shows.
pub fn download_error(err: &UpdateError) -> String {
    match err {
        UpdateError::DigestMismatch { .. } => {
            "the download did not match the signed digest, so it was discarded".to_owned()
        }
        UpdateError::SizeMismatch { .. } => {
            "the download was not the size the signed manifest declares, so it was discarded"
                .to_owned()
        }
        UpdateError::Untrusted => {
            "the release is not signed with the key this build trusts".to_owned()
        }
        UpdateError::TooLarge { limit, .. } => {
            format!("the download is larger than the {limit} bytes this build accepts")
        }
        other => other.to_string(),
    }
}

fn plan_error(err: &InstallError) -> String {
    format!("the update could not be installed: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(version: &str, rollback: bool, yanked: bool) -> AvailableSummary {
        AvailableSummary {
            version: version.to_owned(),
            epoch: 7,
            channel: "stable".to_owned(),
            size: 12_900_000,
            rollback,
            current_yanked: yanked,
            file_name: format!("Goble-{version}-arm64.dmg"),
        }
    }

    fn model() -> UpdateViewModel {
        UpdateViewModel::new("0.1.0", "stable", 1)
    }

    #[test]
    fn the_state_machine_walks_the_happy_path() {
        let mut model = model();
        assert_eq!(model.phase(), UpdatePhase::Idle);
        assert!(model.view().status.contains("Not checked"));

        assert!(model.begin_check());
        assert_eq!(model.phase(), UpdatePhase::Checking);
        // A second check does not restart a running one.
        assert!(!model.begin_check());

        assert!(model.check_finished(Utc::now(), Ok(Some(summary("0.2.0", false, false)))));
        assert_eq!(model.phase(), UpdatePhase::Available);
        assert!(model.view().can_install());
        assert_eq!(model.view().available.as_ref().unwrap().version, "0.2.0");
        assert!(model.view().status.contains("0.2.0"));

        assert!(model.begin_download());
        assert_eq!(model.phase(), UpdatePhase::Downloading);
        assert!(!model.view().can_install(), "nothing to install while downloading");
        assert!(!model.begin_download());
        assert!(!model.begin_check(), "a check cannot cut in front of a download");

        assert!(model.download_finished("0.2.0"));
        assert_eq!(model.phase(), UpdatePhase::Ready);
        assert_eq!(model.view().ready_version.as_deref(), Some("0.2.0"));
        assert!(model.view().status.contains("restart"));
    }

    #[test]
    fn a_failure_is_visible_in_the_view_and_resets() {
        let mut model = model();
        model.begin_check();
        model.check_finished(Utc::now(), Err("could not reach https://downloads.goble.dev".into()));
        assert_eq!(model.phase(), UpdatePhase::Error);
        assert!(model.view().error.as_ref().unwrap().contains("could not reach"));
        // The button still works after a failure.
        assert!(model.begin_check());
        model.check_finished(Utc::now(), Ok(None));
        assert_eq!(model.phase(), UpdatePhase::Idle);
        assert!(model.view().status.contains("newest"));
        model.reset();
        assert_eq!(model.phase(), UpdatePhase::Idle);
    }

    #[test]
    fn a_download_failure_leaves_the_offer_in_place() {
        let mut model = model();
        model.begin_check();
        model.check_finished(Utc::now(), Ok(Some(summary("0.2.0", false, false))));
        model.begin_download();
        model.fail("the download did not match the signed digest, so it was discarded");
        assert_eq!(model.phase(), UpdatePhase::Error);
        assert!(model.view().error.as_ref().unwrap().contains("signed digest"));
        // The release is still offered, so a retry can start from it.
        assert_eq!(model.view().available.as_ref().unwrap().version, "0.2.0");
    }

    #[test]
    fn a_withdrawn_running_version_earns_a_banner() {
        let mut model = model();
        model.begin_check();
        model.check_finished(Utc::now(), Ok(Some(summary("0.0.9", true, true))));
        assert!(model.view().is_yanked());
        let banner = model.view().banner.clone().unwrap();
        assert_eq!(banner.running, "0.1.0");
        assert_eq!(banner.replacement, "0.0.9");
        assert!(banner.detail.contains("withdrawn"));
        let available = model.view().available.clone().unwrap();
        assert!(available.rollback);
        assert!(!available.is_upgrade());
        assert!(model.view().can_install());
    }

    #[test]
    fn a_plain_upgrade_has_no_banner() {
        let mut model = model();
        model.begin_check();
        model.check_finished(Utc::now(), Ok(Some(summary("0.2.0", false, false))));
        assert!(!model.view().is_yanked());
        assert!(model.view().banner.is_none());
    }

    #[test]
    fn nothing_available_clears_a_previous_offer() {
        let mut model = model();
        model.begin_check();
        model.check_finished(Utc::now(), Ok(Some(summary("0.2.0", false, false))));
        model.begin_check();
        model.check_finished(Utc::now(), Ok(None));
        assert_eq!(model.phase(), UpdatePhase::Idle);
        assert!(model.view().available.is_none());
        assert!(model.view().ready_version.is_none());
    }

    #[test]
    fn a_late_answer_cannot_move_a_finished_update_backwards() {
        let mut model = model();
        model.begin_check();
        model.check_finished(Utc::now(), Ok(Some(summary("0.2.0", false, false))));
        model.begin_download();
        // A stale check result arriving now is ignored: the phase is not
        // `Checking`.
        assert!(!model.check_finished(Utc::now(), Ok(None)));
        assert_eq!(model.phase(), UpdatePhase::Downloading);

        assert!(model.download_finished("0.2.0"));
        assert_eq!(model.phase(), UpdatePhase::Ready);
        // And a second finish for the same download cannot rename it.
        assert!(!model.download_finished("0.3.0"));
        assert_eq!(model.view().ready_version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn the_check_clock_is_six_hours() {
        let mut model = model();
        let start = Utc::now();
        assert!(model.check_due(start), "the first check of a session is due");

        model.begin_check();
        model.check_finished(start, Ok(None));
        assert!(!model.check_due(start + chrono::Duration::minutes(59)));
        assert!(!model.check_due(start + chrono::Duration::minutes(60 * 6 - 1)));
        assert!(model.check_due(start + chrono::Duration::minutes(60 * 6)));
        assert!(model.check_due(start + chrono::Duration::hours(9)));
    }

    #[test]
    fn the_state_file_fills_the_view_without_starting_a_check() {
        let mut state = UpdateState::default();
        state.confirm_install("0.1.0", 4);
        state.record_failed("0.2.1", 6);
        let mut model = model();
        model.adopt_state(&state);
        assert_eq!(model.view().installed_epoch, 4);
        assert_eq!(model.view().failed_version.as_deref(), Some("0.2.1"));
        assert_eq!(model.phase(), UpdatePhase::Idle, "reading state is not checking");
        assert!(model.view().last_check.is_none());
    }

    #[test]
    fn the_offer_carries_the_size_channel_and_file() {
        let update = AvailableUpdate {
            channel: Channel::Stable,
            version: goble_update::parse_version("0.2.0").unwrap(),
            epoch: 9,
            rollback: false,
            current_yanked: false,
            artifact: goble_update::Artifact {
                os: "macos".into(),
                arch: "aarch64".into(),
                url: "https://downloads.goble.dev/dl/v0.2.0/Goble-0.2.0-arm64.dmg".into(),
                sha256: "a".repeat(64),
                size: 12_900_000,
            },
            notes_url: None,
            released_at: None,
            verified: true,
        };
        let summary = AvailableSummary::from_update(&update);
        assert_eq!(summary.version, "0.2.0");
        assert_eq!(summary.channel, "stable");
        assert_eq!(summary.epoch, 9);
        assert_eq!(summary.size_label(), "12.3 MB");
        assert_eq!(summary.detail(), "0.2.0 · stable · 12.3 MB");
        assert_eq!(summary.file_name, "Goble-0.2.0-arm64.dmg");
    }

    #[test]
    fn a_relative_check_time_reads_as_elapsed() {
        let now = Utc::now();
        assert_eq!(relative_check(now, now), "checked just now");
        assert_eq!(relative_check(now - chrono::Duration::minutes(5), now), "checked 5m ago");
        assert_eq!(relative_check(now - chrono::Duration::hours(3), now), "checked 3h ago");
        assert_eq!(relative_check(now - chrono::Duration::days(2), now), "checked 2d ago");
    }

    #[test]
    fn a_failed_check_keeps_the_reason_it_was_given() {
        let err = UpdateError::Status {
            url: "https://downloads.goble.dev/channel_versions.json".into(),
            status: 503,
        };
        assert_eq!(check_error(&err), "https://downloads.goble.dev/channel_versions.json answered HTTP 503");
        let err = UpdateError::DigestMismatch {
            url: "https://x/y".into(),
            expected: "a".into(),
            actual: "b".into(),
        };
        assert!(download_error(&err).contains("signed digest"));
    }

    #[test]
    fn health_confirmation_records_the_pending_install() {
        let home = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        let state_path = paths.state_path();
        paths
            .write_pending(&goble_update::PendingInstall::for_appimage(
                "0.2.0",
                7,
                home.path().join("Goble.AppImage"),
            ))
            .unwrap();

        let confirmed = confirm_healthy(&paths, &state_path, "0.2.0").unwrap();
        assert_eq!(confirmed.as_deref(), Some("0.2.0"));
        let state = UpdateState::load(&state_path).unwrap();
        assert_eq!(state.installed_version(), Some("0.2.0"));
        assert_eq!(state.installed_epoch(), 7);
        assert!(paths.is_healthy());
        assert!(paths.pending().unwrap().is_none());
    }

    #[test]
    fn health_confirmation_without_a_pending_install_records_what_runs() {
        let home = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::under(home.path());
        let state_path = paths.state_path();

        assert_eq!(confirm_healthy(&paths, &state_path, "0.1.0").unwrap(), None);
        let state = UpdateState::load(&state_path).unwrap();
        assert_eq!(state.installed_version(), Some("0.1.0"));
        assert_eq!(state.installed_epoch(), 0);
    }
}
