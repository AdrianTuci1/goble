//! Native macOS application menu bar, following the warp-new approach of a real
//! AppKit `NSMenu` installed as `[NSApp mainMenu]`.
//!
//! A single Objective-C handler object is the target of every custom menu item.
//! Items carry their command name in the item `tag` (an `isize` index into a
//! shared [`MenuState`] name list); on selection the handler reads the tag and
//! dispatches to the app command registry ([`WindowControl::run_command`]) or
//! adjusts the whole-app zoom. This avoids wiring a bespoke selector per item.
//!
//! The handler is built with `objc2`'s `define_class!`. The shared state is
//! stashed in a process-global pointer because the handler is a plain AppKit
//! object (no Rust ivars). It is only ever touched on the main thread, after
//! [`install_main_menu`] has run once.
//!
//! Two things AppKit would otherwise take from the executable's name are set
//! here as well, because the app normally runs as a bare binary rather than
//! from a `.app` bundle: the application name ([`APP_NAME`], or the bundle's
//! own name when there is a bundle) and the dock icon (the placeholder in
//! `assets/icons/app-icon-placeholder.png`, until the real artwork lands).

use std::cell::RefCell;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::atomic::{AtomicPtr, Ordering};

use objc2::runtime::AnyObject;
use objc2::{define_class, sel, AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSImage, NSMenu, NSMenuItem, NSRunningApplication, NSEventModifierFlags,
};
use objc2_foundation::{NSBundle, NSData, NSObject, NSString};

use crate::platform::window::{clamp_zoom, WindowControl, ZOOM_STEP};

/// The application's name, and the single place to change it.
///
/// It titles the application menu and the items that carry the name (About,
/// Hide, Quit) when the process runs without a bundle. A bundled build reads
/// `CFBundleName`/`CFBundleDisplayName` instead, so a rename for releases
/// belongs in `packaging/macos/Info.plist`.
pub const APP_NAME: &str = "Goble";

/// Placeholder dock icon, embedded so the bare binary needs no bundle
/// resources. Replace this one file with the real artwork — a square PNG, 512
/// or 1024 px, same path — and the dock picks it up with no code change:
/// `crates/goble-ui/assets/icons/app-icon-placeholder.png`.
static APP_ICON_PLACEHOLDER_PNG: &[u8] =
    include_bytes!("../../../assets/icons/app-icon-placeholder.png");

/// Command names recognized specially by [`run_handler`] (zoom, not app actions).
const ZOOM_IN: &str = "__zoom_in";
const ZOOM_OUT: &str = "__zoom_out";
const ZOOM_RESET: &str = "__zoom_reset";

/// State the menu handler reads when an item is selected. Owned by the global
/// pointer; only accessed on the main thread.
struct MenuState {
    /// Command name loaded into each item's `tag`, indexed by the item tag.
    names: Vec<String>,
    window_control: WindowControl,
    ui_zoom: Rc<RefCell<f32>>,
}

static MENU_STATE: AtomicPtr<MenuState> = AtomicPtr::new(null_mut());

fn menu_state() -> &'static MenuState {
    // SAFETY: set exactly once by `install_main_menu` on the main thread and
    // only read (main thread) by menu selections; never mutated after install.
    unsafe { &*MENU_STATE.load(Ordering::SeqCst) }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "GobleMenuHandler"]
    struct GobleMenuHandler;

    impl GobleMenuHandler {
        #[unsafe(method(run:))]
        fn run(&self, sender: &AnyObject) {
            run_handler(sender);
        }
    }
);

/// Dispatch a menu selection. Adds `sender.tag` (from `NSMenuItem`); reads the
/// corresponding name and routes it (zoom commands vs. app commands).
fn run_handler(sender: &AnyObject) {
    // `sender` is the `NSMenuItem` that fired the action.
    let item = unsafe { &*(sender as *const AnyObject as *const NSMenuItem) };
    let tag = item.tag() as usize;
    let state = menu_state();
    let name = state
        .names
        .get(tag)
        .cloned()
        .unwrap_or_default();
    match name.as_str() {
        ZOOM_IN | ZOOM_OUT | ZOOM_RESET => {
            let mut zoom = state.ui_zoom.borrow_mut();
            *zoom = match name.as_str() {
                ZOOM_IN => clamp_zoom(*zoom + ZOOM_STEP),
                ZOOM_OUT => clamp_zoom(*zoom - ZOOM_STEP),
                _ => 1.0,
            };
            // Redraw is forced every frame by `about_to_wait`, so the new zoom
            // shows up immediately without needing the window here.
            let _ = zoom;
        }
        other => state.window_control.run_command(other),
    }
}

/// Build a `Retained<NSMenuItem>` that dispatches a custom app command on
/// selection. The command name is appended to [`MenuState::names`] and its index
/// stored in the item `tag`; [`run_handler`] looks the name back up and routes
/// it to the app command registry (or the zoom handlers).
///
/// Use [`make_standard_item`] for items AppKit should handle on its own
/// (About, Quit, Minimize…) rather than routing through [`run_handler`].
fn make_item(
    mtm: MainThreadMarker,
    title: &str,
    key: Option<(&str, NSEventModifierFlags)>,
    command: &str,
    state: &mut MenuState,
) -> objc2::rc::Retained<NSMenuItem> {
    let title_ns = NSString::from_str(title);
    let action = sel!(run:);
    let key_equiv = key.map(|(k, _)| k).unwrap_or("");
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &title_ns,
            Some(action),
            &NSString::from_str(key_equiv),
        )
    };
    if let Some((_, flags)) = key {
        item.setKeyEquivalentModifierMask(flags);
    }
    let idx = state.names.len();
    state.names.push(command.to_string());
    item.setTag(idx as objc2_foundation::NSInteger);
    unsafe { item.setTarget(Some(&*menu_handler())) };
    item
}

/// Build a standard AppKit `NSMenuItem` whose action is routed through the
/// responder chain (target stays `nil`), so AppKit handles it natively — e.g.
/// the About panel (`orderFrontStandardAboutPanel:`), app termination
/// (`terminate:`), or window minimization (`performMiniaturize:`).
fn make_standard_item(
    mtm: MainThreadMarker,
    title: &str,
    action: objc2::runtime::Sel,
    key: Option<(&str, NSEventModifierFlags)>,
) -> objc2::rc::Retained<NSMenuItem> {
    let title_ns = NSString::from_str(title);
    let key_equiv = key.map(|(k, _)| k).unwrap_or("");
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &title_ns,
            Some(action),
            &NSString::from_str(key_equiv),
        )
    };
    if let Some((_, flags)) = key {
        item.setKeyEquivalentModifierMask(flags);
    }
    // No custom target/tag: the responder chain (NSApp/NSWindow) handles it.
    item
}

/// Create (once) and retain the handler object that receives `run:`. A single
/// instance is shared by every custom item.
fn menu_handler() -> &'static objc2::rc::Retained<GobleMenuHandler> {
    // SAFETY: created on the main thread from `install_main_menu`.
    static HANDLER: std::sync::OnceLock<objc2::rc::Retained<GobleMenuHandler>> =
        std::sync::OnceLock::new();
    HANDLER.get_or_init(|| {
        let obj: objc2::rc::Retained<GobleMenuHandler> = unsafe {
            objc2::msg_send![GobleMenuHandler::alloc(), init]
        };
        obj
    })
}

/// Resolve the application name from a bundle's advertised name, falling back
/// to [`APP_NAME`]. A blank or missing bundle name is the bare-binary case, so
/// it falls back too. Split out from the AppKit lookup so the rule is testable.
fn resolve_app_name(bundle_name: Option<&str>) -> String {
    match bundle_name.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => APP_NAME.to_string(),
    }
}

/// The name a real `.app` bundle advertises: `CFBundleDisplayName`, else
/// `CFBundleName`. `None` for a bare binary, whose main bundle has no info
/// dictionary at all.
fn bundle_app_name() -> Option<String> {
    let bundle = NSBundle::mainBundle();
    ["CFBundleDisplayName", "CFBundleName"]
        .into_iter()
        .find_map(|key| {
            let value = bundle.objectForInfoDictionaryKey(&NSString::from_str(key))?;
            let value = value.downcast::<NSString>().ok()?;
            let value = value.to_string();
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        })
}

/// Whether the process runs from a real `.app` bundle, which supplies its own
/// name and icon — neither is overridden in that case.
fn is_bundled() -> bool {
    NSRunningApplication::currentApplication()
        .bundleIdentifier()
        .is_some()
}

/// The name to show in the menu bar.
fn app_name() -> String {
    resolve_app_name(bundle_app_name().as_deref())
}

/// Rename the process when no bundle provides a name.
///
/// With no bundle, AppKit names the bold application menu after the executable
/// — which is why `./target/debug/goble-app` showed `goble-app` there — and the
/// process name is the public way to change it. `NSProcessInfo` has no enabled
/// `objc2-foundation` feature in this crate, so the singleton and its setter
/// are sent untyped.
///
/// [`install_main_menu`] calls this; it is also safe to call earlier (before
/// the event loop exists) and idempotent, which is what a caller needs if the
/// name has to be in place before AppKit builds the menu bar.
pub fn prepare_app_name() {
    if is_bundled() {
        return;
    }
    let process_info: objc2::rc::Retained<AnyObject> = unsafe {
        objc2::msg_send![objc2::class!(NSProcessInfo), processInfo]
    };
    let name = NSString::from_str(&app_name());
    let _: () = unsafe { objc2::msg_send![&*process_info, setProcessName: &*name] };
}

/// Show the placeholder dock icon, so an unbundled run shows Goble's square
/// rather than the generic executable icon. A bundle's `CFBundleIconFile` (and
/// the packaged PNG it points at) is left alone.
fn install_dock_icon_placeholder(mtm: MainThreadMarker) {
    if is_bundled() {
        return;
    }
    let data = NSData::with_bytes(APP_ICON_PLACEHOLDER_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        log::warn!("placeholder dock icon did not decode; keeping the default icon");
        return;
    };
    // SAFETY: `setApplicationIconImage:` retains the image, and the caller holds
    // the main thread (`mtm`).
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
}

/// Add a top-level menu to the menu bar, titled on both the item and its
/// submenu: AppKit renders one of the two in the menu bar, and falls back to
/// the `NSMenuItem` class name when both are empty — the placeholder that
/// showed on every menu.
fn add_top_level_menu(
    mtm: MainThreadMarker,
    main_menu: &NSMenu,
    title: &str,
    submenu: &NSMenu,
) {
    let title_ns = NSString::from_str(title);
    submenu.setTitle(&title_ns);
    let item = NSMenuItem::new(mtm);
    item.setTitle(&title_ns);
    item.setSubmenu(Some(submenu));
    main_menu.addItem(&item);
}

/// Install the application main menu on the shared `NSApplication`, following
/// the macOS convention (app menu + File + Edit + View + Window). This is called
/// from the platform event loop once the app is running.
pub unsafe fn install_main_menu(window_control: WindowControl, ui_zoom: Rc<RefCell<f32>>) {
    let mtm = MainThreadMarker::new_unchecked();
    // Before any menu exists, so AppKit has the name when it first lays out the
    // menu bar.
    prepare_app_name();
    let mut state = MenuState {
        names: Vec::new(),
        window_control,
        ui_zoom,
    };

    let main_menu = NSMenu::new(mtm);

    let cmd = NSEventModifierFlags::Command;
    let ctrl = NSEventModifierFlags::Control;
    let shift = NSEventModifierFlags::Shift;
    let opt = NSEventModifierFlags::Option;

    // App menu (the menu AppKit titles with the application name).
    let name = app_name();
    {
        let sub = NSMenu::new(mtm);
        // Standard AppKit items (About, Hide, Quit) route through the responder
        // chain with target `nil`; AppKit shows the About panel / hides / quits.
        sub.addItem(&*make_standard_item(
            mtm,
            &format!("About {name}"),
            sel!(orderFrontStandardAboutPanel:),
            None,
        ));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&*make_item(
            mtm,
            "Settings…",
            Some((",", cmd)),
            "open_settings",
            &mut state,
        ));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&*make_standard_item(
            mtm,
            &format!("Hide {name}"),
            sel!(hide:),
            Some(("h", cmd)),
        ));
        sub.addItem(&*make_standard_item(
            mtm,
            "Hide Others",
            sel!(hideOtherApplications:),
            Some(("h", cmd | opt)),
        ));
        sub.addItem(&*make_standard_item(
            mtm,
            "Show All",
            sel!(unhideAllApplications:),
            None,
        ));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&*make_standard_item(
            mtm,
            &format!("Quit {name}"),
            sel!(terminate:),
            Some(("q", cmd)),
        ));
        add_top_level_menu(mtm, &main_menu, &name, &sub);
    }

    // File
    {
        let sub = NSMenu::new(mtm);
        sub.addItem(&*make_item(mtm, "New Space", Some(("n", cmd)), "new_space", &mut state));
        sub.addItem(&*make_item(
            mtm,
            "New Terminal",
            Some(("t", cmd | shift)),
            "new_terminal",
            &mut state,
        ));
        sub.addItem(&*make_item(
            mtm,
            "Close Pane",
            Some(("w", cmd)),
            "close_pane",
            &mut state,
        ));
        add_top_level_menu(mtm, &main_menu, "File", &sub);
    }

    // Edit
    {
        let sub = NSMenu::new(mtm);
        sub.addItem(&*make_item(mtm, "Copy", Some(("c", cmd)), "copy", &mut state));
        sub.addItem(&*make_item(
            mtm,
            "Clear Transcript",
            None,
            "clear_transcript",
            &mut state,
        ));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&*make_item(mtm, "Split Right", Some((" ", cmd)), "split_right", &mut state));
        sub.addItem(&*make_item(mtm, "Split Down", Some(("d", cmd | shift)), "split_down", &mut state));
        add_top_level_menu(mtm, &main_menu, "Edit", &sub);
    }

    // View
    {
        let sub = NSMenu::new(mtm);
        sub.addItem(&*make_item(
            mtm,
            "Toggle Fullscreen",
            Some(("f", cmd | ctrl)),
            "toggle_fullscreen",
            &mut state,
        ));
        sub.addItem(&*make_item(
            mtm,
            "Toggle Right Sidebar",
            Some(("b", cmd | ctrl)),
            "toggle_right_sidebar",
            &mut state,
        ));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        sub.addItem(&*make_item(mtm, "Zoom In", Some(("+", cmd)), ZOOM_IN, &mut state));
        sub.addItem(&*make_item(mtm, "Zoom Out", Some(("-", cmd)), ZOOM_OUT, &mut state));
        sub.addItem(&*make_item(mtm, "Reset Zoom", Some(("0", cmd)), ZOOM_RESET, &mut state));
        add_top_level_menu(mtm, &main_menu, "View", &sub);
    }

    // Window
    {
        let sub = NSMenu::new(mtm);
        sub.addItem(&*make_standard_item(
            mtm,
            "Minimize",
            sel!(performMiniaturize:),
            Some(("m", cmd)),
        ));
        sub.addItem(&*make_standard_item(mtm, "Zoom", sel!(performZoom:), None));
        sub.addItem(&NSMenuItem::separatorItem(mtm));
        // The tab strip's own chords: AppKit matches the key equivalent before
        // the window does, so the items carry the same Ctrl+Tab / Ctrl+Shift+Tab
        // and route to the same command the key handler runs.
        sub.addItem(&*make_item(
            mtm,
            "Next Space",
            Some(("\t", ctrl)),
            "next_space",
            &mut state,
        ));
        sub.addItem(&*make_item(
            mtm,
            "Previous Space",
            Some(("\t", ctrl | shift)),
            "previous_space",
            &mut state,
        ));
        add_top_level_menu(mtm, &main_menu, "Window", &sub);
    }

    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));

    // Per-process, so it belongs with the one-shot menu install rather than the
    // per-frame path.
    install_dock_icon_placeholder(mtm);

    // Publish the state for the menu handler. The names vector is final here.
    let prev = MENU_STATE.swap(Box::into_raw(Box::new(state)), Ordering::SeqCst);
    // Free any previous state (should never happen; only installed once).
    if !prev.is_null() {
        let _ = unsafe { Box::from_raw(prev) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one place the app name lives.
    #[test]
    fn app_name_constant_is_goble() {
        assert_eq!(APP_NAME, "Goble");
    }

    /// A bundle that names itself wins over the fallback — the bundled case is
    /// not fought.
    #[test]
    fn a_bundle_name_is_used_as_is() {
        assert_eq!(resolve_app_name(Some("Goble Nightly")), "Goble Nightly");
        assert_eq!(resolve_app_name(Some(" Goble ")), "Goble");
    }

    /// No bundle (the bare `./target/debug/goble-app` the user runs) falls back
    /// to the constant instead of the executable's name.
    #[test]
    fn a_missing_bundle_name_falls_back_to_the_constant() {
        assert_eq!(resolve_app_name(None), APP_NAME);
        assert_eq!(resolve_app_name(Some("")), APP_NAME);
        assert_eq!(resolve_app_name(Some("   ")), APP_NAME);
    }

    /// The embedded dock icon has to be a square PNG (512 or 1024 px) that the
    /// image decoders will accept, since a bad one only shows up at runtime.
    #[test]
    fn placeholder_dock_icon_is_a_square_png() {
        let bytes = APP_ICON_PLACEHOLDER_PNG;
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
        assert_eq!(&bytes[12..16], b"IHDR", "first chunk is IHDR");
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        assert_eq!(width, height, "an app icon is square");
        assert!(
            width == 512 || width == 1024,
            "macOS app icons are 512 or 1024 px, got {width}"
        );
    }
}
