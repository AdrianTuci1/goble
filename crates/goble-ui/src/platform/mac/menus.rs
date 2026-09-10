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

use std::cell::RefCell;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::atomic::{AtomicPtr, Ordering};

use objc2::runtime::AnyObject;
use objc2::{define_class, sel, AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSMenu, NSMenuItem, NSEventModifierFlags,
};
use objc2_foundation::{NSObject, NSString};

use crate::platform::window::{clamp_zoom, WindowControl, ZOOM_STEP};

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

/// Install the application main menu on the shared `NSApplication`, following
/// the macOS convention (app menu + File + Edit + View + Window). This is called
/// from the platform event loop once the app is running.
pub unsafe fn install_main_menu(window_control: WindowControl, ui_zoom: Rc<RefCell<f32>>) {
    let mtm = MainThreadMarker::new_unchecked();
    let mut state = MenuState {
        names: Vec::new(),
        window_control,
        ui_zoom,
    };

    let main_menu = NSMenu::new(mtm);

    let cmd = NSEventModifierFlags::Command;
    let ctrl = NSEventModifierFlags::Control;
    let shift = NSEventModifierFlags::Shift;

    // App menu (category titled by the app name).
    {
        let sub = NSMenu::new(mtm);
        // Standard AppKit items (About, Quit) route through the responder chain
        // with target `nil`; AppKit shows the About panel / terminates the app.
        sub.addItem(&*make_standard_item(
            mtm,
            "About Goble",
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
            "Quit Goble",
            sel!(terminate:),
            Some(("q", cmd)),
        ));
        let app_item = NSMenuItem::new(mtm);
        app_item.setSubmenu(Some(&sub));
        main_menu.addItem(&app_item);
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
        let file_item = NSMenuItem::new(mtm);
        file_item.setSubmenu(Some(&sub));
        main_menu.addItem(&file_item);
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
        let edit_item = NSMenuItem::new(mtm);
        edit_item.setSubmenu(Some(&sub));
        main_menu.addItem(&edit_item);
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
        let view_item = NSMenuItem::new(mtm);
        view_item.setSubmenu(Some(&sub));
        main_menu.addItem(&view_item);
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
        let window_item = NSMenuItem::new(mtm);
        window_item.setSubmenu(Some(&sub));
        main_menu.addItem(&window_item);
    }

    NSApplication::sharedApplication(mtm).setMainMenu(Some(&main_menu));

    // Publish the state for the menu handler. The names vector is final here.
    let prev = MENU_STATE.swap(Box::into_raw(Box::new(state)), Ordering::SeqCst);
    // Free any previous state (should never happen; only installed once).
    if !prev.is_null() {
        let _ = unsafe { Box::from_raw(prev) };
    }
}
