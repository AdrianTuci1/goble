// Workflow: fix the goble native GUI interaction bugs and add the requested
// feature set, self-evaluating progress per feature. Phases run sequentially in
// the same workspace so edits accumulate. Verification is cargo check + cargo
// test; agents must NOT launch the GUI binary (no `cargo run`).

let meta = #{
    name: "agent-gui-fix",
    description: "Fix goble GUI interactions (input pipeline, topbar, rich-input, panes, agent header, sidebar) and self-evaluate each feature",
    when_to_use: "Run against the goble native GUI to fix the interaction bugs and add the requested feature set, reporting per-feature progress without relaunching the app.",
    phases: [
        #{ title: "Input", detail: "fix click dispatch + spacebar coordinate bug" },
        #{ title: "Topbar", detail: "single topbar, env selector in topbar, + env menu" },
        #{ title: "Composer", detail: "rich-input inset + resize pill behavior" },
        #{ title: "Panes", detail: "resize & reposition panes/splits" },
        #{ title: "Header", detail: "agent header 3-dots menu + fullscreen toggle" },
        #{ title: "Sidebar", detail: "switch conversations + env SVGs" },
        #{ title: "Verify", detail: "workspace check + tests + report" },
    ],
};

let impl_schema = #{
    "type": "object",
    "required": ["done", "features", "summary", "issues"],
    "properties": #{
        "done": #{ "type": "boolean" },
        "features": #{ "type": "array", "items": #{ "type": "string" } },
        "summary": #{ "type": "string" },
        "issues": #{ "type": "array", "items": #{ "type": "string" } },
    },
};

let verify_schema = #{
    "type": "object",
    "required": ["checks_pass", "tests_pass", "results"],
    "properties": #{
        "checks_pass": #{ "type": "boolean" },
        "tests_pass": #{ "type": "boolean" },
        "results": #{ "type": "string" },
    },
};

fn is_ok(r) { r != () && r.success }
fn done(r) { r != () && r.success && r.output.done == true }

// ---- Phase: Input -----------------------------------------------------------
phase("Input");
let p = "";
p += "Fix the foundational native-GUI input-pipeline bugs at /Users/adrian.tucicovencogmail.com/Projects/goble. ";
p += "Read the actual files before editing. Do NOT launch the GUI (never run `cargo run`); verify with `cargo check` and `cargo test` only. ";
p += "Use read+write+execute tools (cargo check/test). ";
p += "ROOT CAUSE 1 (systemic): the element tree is rebuilt every frame (app/src/root_view.rs layout calls rebuild() at ~line 541); ";
p += "build_ui at ~520-533 constructs brand-new widgets; crates/goble-ui/src/platform/window.rs about_to_wait at ~220-224 forces a redraw every frame). ";
p += "Each clickable element keeps its press state in a LOCAL InteractiveState (crates/goble-ui/src/elements/interactive.rs handle_mouse_event at ~30-69): ";
p += "a MouseDown sets state.pressed=true and returns true; a MouseUp fires on_click ONLY if state.pressed is still true. ";
p += "Because the widget is rebuilt between down and up, the fresh state.pressed is false, so on_click never fires. ";
p += "Affected: Button (button.rs ~138-160), TopbarButton (topbar.rs ~311-332), HoverButton (hover_button.rs ~99-127), Tab (tab_bar.rs ~96-127), ";
p += "ConversationListItem (conversation_list_item.rs ~294-335), PopupMenu trigger (popup_menu.rs ~234-264), DropdownMenu (~163-190), Select (~130-155). ";
p += "FIX: change handle_mouse_event so a MouseUp whose position is inside the element fires on_click regardless of the local pressed flag (drop the ";
p += "same-instance pressed gate for firing the click), while MouseDown still returns true when inside so parent containers stop propagating. ";
p += "This must NOT affect custom drag elements (SplitNode, SidebarLayout, SpaceBar) which do their own event handling. ";
p += "ROOT CAUSE 2 (independent): app/src/ui/space_bar.rs stores tab and '+' hit-test rects RELATIVE to its own origin in layout ";
p += "(~lines 187-194 push rectf(x, 0.0, size.x, size.y); the '+' add_rect ~205-208), but dispatch_event (~249-336) compares them against ABSOLUTE ";
p += "event positions (contains(r, *position) at ~305 and ~325; tab_index_at ~266/275 uses x only). Because the strip sits below the topbar+divider ";
p += "(its origin.y > 0), tab selection and the trailing '+' new-tab never fire. FIX: make SpaceBar operate on absolute coordinates (offset the rects by ";
p += "the element's origin when building them or when comparing), so tab select and the '+' fire correctly. ";
p += "ACCEPTANCE: (1) `cargo check -p goble-ui -p goble-app` exits 0. (2) `cargo test -p goble-ui -p goble-app` exits 0; if a test relied on the old ";
p += "same-instance pressed gate, update it to the new semantics. (3) Add a regression test proving on_click fires even when the element is rebuilt between ";
p += "MouseDown and MouseUp (e.g. dispatch MouseDown into a Button, construct a fresh Button, dispatch MouseUp inside its bounds, assert the callback fired). ";
p += "Return JSON via output_schema: done = (feature implemented AND cargo check/test green), features = ['buttons','richinput-selectors'], ";
p += "summary = what you changed, issues = anything not completed or any test you had to change.";
let input = agent(p, #{
    label: "input",
    phase: "input",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let input_done = done(input);

// ---- Phase: Topbar ----------------------------------------------------------
phase("Topbar");
let p = "";
p += "Rework the goble native-GUI topbar/space strip at /Users/adrian.tucicovencogmail.com/Projects/goble. Read files before editing. ";
p += "Do NOT run the GUI; verify with `cargo check -p goble-ui -p goble-app` and `cargo test`. ";
p += "ASSERT FIRST: confirm the foundational click fix and SpaceBar coordinate fix from the previous phase are in place (they should be), so buttons and the ";
p += "'+' (new tab) now fire. Then implement three things. ";
p += "(A) SINGLE TOPBAR + new tab: currently there is a library Topbar (crates/goble-ui/src/elements/topbar.rs, built in app/src/ui/shell.rs build_topbar at ~26-47) ";
p += "AND a SpaceBar space strip (app/src/ui/space_bar.rs, added in app/src/ui/mod.rs build_ui at ~1123-1146) stacked. Make sure only ONE visible general toolbar ";
p += "(remove the duplicate/general on redundancy where appropriate) and that pressing the trailing '+' opens a NEW TAB (a new Space) reliably. ";
p += "(B) ENVIRONMENT SELECTOR IN THE TOPBAR: move the environment (Local / Remote) selector OUT of the sidebar and INTO the topbar. Today it is built by ";
p += "app/src/ui/media.rs build_medium_selector and rendered in app/src/ui/sidebar.rs at ~237 (inserted ~230-238). Remove it from the sidebar (and drop the now-unused ";
p += "media/media_actions params from build_sidebar signature) and render a compact environment selector in the topbar (app/src/ui/shell.rs build_topbar or ";
p += "app/src/ui/mod.rs build_ui around the Topbar). It should still set the selected medium (MediaState via on_select_medium) and keep the sidebar conversation ";
p += "filter working (it reads media.selected_medium -> medium_routing). ";
p += "(C) '+' ENVIRONMENT MENU (warp-new style): pressing '+' should NOT immediately create a space; instead open a menu (reuse crates/goble-ui/src/elements/popup_menu.rs) ";
p += "listing the environments (Local, Remote xrdp). Choosing one creates a NEW space/tab whose default environment is that medium (set the new thread's workspace_routing and the space's medium). ";
p += "Also include an 'Add new medium…' option that lets the user add a new environment by name; add it to the selector/menu (extend MediaState to support adding a medium: ";
p += "a method like add_medium(id, label) that pushes a node; give it a reasonable default routing) and persist the choice so it reappears. ";
p += "ACCEPTANCE: `cargo check -p goble-ui -p goble-app` green; `cargo test -p goble-ui -p goble-app` green. Wire the new menu/actions through UiActions (app/src/ui/mod.rs) and actions.rs. ";
p += "Return JSON: done, features = ['topbars-newtab','env-to-topbar','add-medium-menu'], summary, issues.";
let topbar = agent(p, #{
    label: "topbar",
    phase: "topbar",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let topbar_done = done(topbar);

// ---- Phase: Composer --------------------------------------------------------
phase("Composer");
let p = "";
p += "Fix the goble native-GUI rich input (composer) at /Users/adrian.tucicovencogmail.com/Projects/goble. Read files before editing. ";
p += "Do NOT run the GUI; verify with `cargo check -p goble-ui -p goble-app` and `cargo test`. ";
p += "The composer is crates/goble-ui/src/elements/chat_composer.rs (context_pill at ~314, harness pill at ~449, dir pill at ~464, branch pill at ~516; ";
p += "ComposerButton::paint at ~766-776; composer card at ~645-656) plus app/src/ui/chat.rs build_agent_chat (composer wiring at ~152-190). ";
p += "Implement: (A) REMOVE THE GRAY INSET: ComposerButton::paint currently fills with ColorToken::Surface + stroke Border (~766-776); remove the gray background/box so the ";
p += "rich-input pills have no inset (and drop the gray composer card back/border at ~645-656 if it is the 'inset' seen). Keep hover feedback but no filled box. ";
p += "(B) RICH-INPUT PILLS BEHAVE ON RESIZE LIKE warp-new: make the context-pill buttons and their PopupMenu behave correctly when the window/composer is resized — ";
p += "the menu should re-anchor under its trigger and not overflow or cut off; review crates/goble-ui/src/elements/popup_menu.rs layout (~189-214) and the composer footer's ";
p += "Expanded/Clipped handling (~458-508, 640-657) so pills wrap/reflow and the dropdown tracks the trigger on resize. ";
p += "(C) Confirm the rich-input selectors open (the foundational click fix should make the popup trigger fire); if a selector still does not open, fix it. ";
p += "ACCEPTANCE: `cargo check -p goble-ui -p goble-app` green; `cargo test -p goble-ui -p goble-app` green. ";
p += "Return JSON: done, features = ['richinput-inset','richinput-resize'], summary, issues.";
let composer = agent(p, #{
    label: "composer",
    phase: "composer",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let composer_done = done(composer);

// ---- Phase: Panes -----------------------------------------------------------
phase("Panes");
let p = "";
p += "Fix pane resize & repositioning in the goble native GUI at /Users/adrian.tucicovencogmail.com/Projects/goble. Read files before editing. ";
p += "Do NOT run the GUI; verify with `cargo check -p goble-ui -p goble-app` and `cargo test`. ";
p += "Splits: crates/goble-ui/src/elements/split.rs SplitNode (DIVIDER_HALF=4.0 at ~27, handle_bounds ~98, ratio_from_position ~110, with_on_drag_* ~63-76, ";
p += "dispatch_event drag ~175-213). Built by app/src/ui/panes.rs build_pane (~53, SplitNode for each Pane::Split at ~61-85, wiring on_pane_drag_*). ";
p += "Actions: app/src/actions.rs on_pane_drag_start ~852, on_pane_drag_move ~855 (space.set_ratio), on_pane_drag_end ~864; Pane::set_ratio clamps 0.05..0.95 (app/src/ui/mod.rs ~293). ";
p += "Sidebar divider: app/src/ui/shell.rs SidebarLayout (~113; handle_bounds ~165, layout ~177, dispatch_event drag ~228-262), wired in mod.rs build_ui ~1105-1117; ";
p += "actions on_sidebar_drag_start/move/end ~606-617 (sidebar_width clamp 200..480). ";
p += "Fix so that: (1) dragging a pane split divider resizes the panes live and smoothly; (2) dragging the sidebar divider resizes the sidebar; ";
p += "(3) panes can be repositioned/moved (if the design supports moving a pane into another split, wire the existing split/move actions; otherwise ensure the existing ";
p += "split/close/navigate controls work); (4) a pane cannot be shrunk to a useless size (enforce a sensible min pixel size at the split/sidebar level, not just the 0.05 ratio floor). ";
p += "ACCEPTANCE: `cargo check -p goble-ui -p goble-app` green; `cargo test -p goble-ui -p goble-app` green. Add a test for the split ratio clamp / min-size if feasible. ";
p += "Return JSON: done, features = ['resize-reposition'], summary, issues.";
let panes = agent(p, #{
    label: "panes",
    phase: "panes",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let panes_done = done(panes);

// ---- Phase: Header ----------------------------------------------------------
phase("Header");
let p = "";
p += "Add a 3-dots menu + fullscreen toggle to the agent/conversation header in the goble native GUI at /Users/adrian.tucicovencogmail.com/Projects/goble. ";
p += "Read files before editing. Do NOT run the GUI; verify with `cargo check -p goble-ui -p goble-app` and `cargo test`. ";
p += "The header is built in app/src/ui/chat.rs build_agent_header (~341-424): renders agent_name (~356) + TopbarButton actions (copy ~371, restart ~379, crons ~388, ";
p += "right-sidebar toggle ~392-404) and is attached to the chat via ChatView::with_header (chat.rs ~121/25). The container is crates/goble-ui/src/views/chat_view.rs ChatView ";
p += "(~27, with_header ~146, inserted at top of rebuild ~468). ";
p += "Implement: (A) add a three-dot ('dots-horizontal' icon) button in the header that opens a PopupMenu (reuse crates/goble-ui/src/elements/popup_menu.rs) with a few options "; 
p += "(e.g. Rename, Clear transcript, and the fullscreen toggle). (B) Add a fullscreen toggle that makes the current agent/window take the WHOLE screen, toggle-like (warp-new behavior): ";
p += "wire a window fullscreen API. You must add plumbing: a set_fullscreen control in crates/goble-ui/src/platform/window.rs (winit Fullscreen::Borderless) exposed to the UI (likely a new ";
p += "DispatchedEvent/EventContext route or a callback), a new UiActions field (app/src/ui/mod.rs ~780) e.g. on_toggle_fullscreen, and a handler in app/src/actions.rs. ";
p += "Make the toggle state live in app state so it survives the per-frame rebuild and the menu shows Checked/unchecked. ";
p += "ACCEPTANCE: `cargo check -p goble-ui -p goble-app` green; `cargo test -p goble-ui -p goble-app` green. ";
p += "Return JSON: done, features = ['header-menu'], summary, issues.";
let header = agent(p, #{
    label: "header",
    phase: "header",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let header_done = done(header);

// ---- Phase: Sidebar ---------------------------------------------------------
phase("Sidebar");
let p = "";
p += "Fix conversation switching and add environment-specific conversation icons in the goble native GUI at /Users/adrian.tucicovencogmail.com/Projects/goble. ";
p += "Read files before editing. Do NOT run the GUI; verify with `cargo check -p goble-ui -p goble-app` and `cargo test`. ";
p += "Sidebar: app/src/ui/sidebar.rs build_sidebar (search box, conversation cards grouped into folders at ~86-155; 'New agent' HoverButton at ~59-77). ";
p += "Selection: app/src/actions.rs on_select_conversation (~198-204) -> state.bind_active_pane_conversation(id) in app/src/state.rs (~697); conversation display in app/src/root_view.rs. ";
p += "Icons: crates/goble-ui/src/elements/icon.rs (Icon::new, icon_atlas_name mapping ~88-149; 'computer'/'monitor' -> 'agentmode'), crates/goble-ui/src/platform/icon_atlas.rs (ICON_FILES ~18-70, ";
p += "rasterize_icon ~278 via resvg), assets in crates/goble-ui/assets/icons/. ";
p += "Implement: (A) SWITCH CONVERSATIONS end-to-end: clicking a conversation card must select it AND the chat pane must reload that conversation's messages (not keep stale content). ";
p += "Verify bind_active_pane_conversation refreshes the pane transcript; if not, make selecting a conversation reload its messages for the bound pane. ";
p += "(B) CONVERSATION SVG ICONS: add an SVG asset for conversation/agent avatars, and place it ABOVE the 'New agent' row showing the ACTIVE conversations; the Remote environment must use a DIFFERENT ";
p += "svg than Local. Add the .svg to crates/goble-ui/assets/icons/, register it in ICON_FILES (icon_atlas.rs ~18-70), add a logical-name mapping in icon_atlas_name (icon.rs ~88-149), and render the icon in the sidebar ";
p += "above 'New agent' (a small avatar strip of active conversations) keyed off each conversation's workspace_routing (local vs remote). ";
p += "ACCEPTANCE: `cargo check -p goble-ui -p goble-app` green; `cargo test -p goble-ui -p goble-app` green. The icon_atlas test (icon_atlas.rs ~294-312) must still pass — add your new SVGs to it if it enumerates expected icons. ";
p += "Return JSON: done, features = ['switch-conversations','env-svg'], summary, issues.";
let sidebar = agent(p, #{
    label: "sidebar",
    phase: "sidebar",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: impl_schema,
});
let sidebar_done = done(sidebar);

// ---- Phase: Verify ----------------------------------------------------------
phase("Verify");
let p = "";
p += "Self-evaluate the goble native GUI fix at /Users/adrian.tucicovencogmail.com/Projects/goble. Read files to confirm the changes are present; do not edit. ";
p += "Run, and capture the tail of each: `cargo check --workspace --all-targets 2>&1 | tail -40` and `cargo test -p goble-ui -p goble-app -p goble-desktop-service 2>&1 | grep -E 'test result:|FAILED|error\\[' | sort | uniq -c`. ";
p += "Report checks_pass = (cargo check exit 0), tests_pass = (no FAILED/error in the test output), and results = a terse bullet list of (a) the cargo check outcome, (b) the per-crate test counts, (c) any warnings about unused code or leftover references to removed symbols. ";
p += "Do not modify anything.";
let verify = agent(p, #{
    label: "verify",
    phase: "verify",
    capability_mode: "all",
    agent_type: "general-purpose",
    output_schema: verify_schema,
});

let checks_pass = is_ok(verify) && verify.output.checks_pass == true;
let tests_pass = is_ok(verify) && verify.output.tests_pass == true;

let features = [];
// For each declared feature, mark done only if its owning phase succeeded.
features.push(#{ "feature": "resize-reposition", "done": panes_done });
features.push(#{ "feature": "buttons", "done": input_done });
features.push(#{ "feature": "topbars-newtab", "done": topbar_done });
features.push(#{ "feature": "richinput-selectors", "done": input_done });
features.push(#{ "feature": "richinput-inset", "done": composer_done });
features.push(#{ "feature": "richinput-resize", "done": composer_done });
features.push(#{ "feature": "header-menu", "done": header_done });
features.push(#{ "feature": "env-to-topbar", "done": topbar_done });
features.push(#{ "feature": "env-svg", "done": sidebar_done });
features.push(#{ "feature": "add-medium-menu", "done": topbar_done });
features.push(#{ "feature": "switch-conversations", "done": sidebar_done });

let report = #{
    checks_pass: checks_pass,
    tests_pass: tests_pass,
    features: features,
    input: if is_ok(input) { input.output } else { () },
    topbar: if is_ok(topbar) { topbar.output } else { () },
    composer: if is_ok(composer) { composer.output } else { () },
    panes: if is_ok(panes) { panes.output } else { () },
    header: if is_ok(header) { header.output } else { () },
    sidebar: if is_ok(sidebar) { sidebar.output } else { () },
    verify: if is_ok(verify) { verify.output } else { () },
};

let text = "goble GUI fix — self-evaluation\n";
text += "cargo check --workspace --all-targets: " + (if checks_pass { "PASS" } else { "FAIL" }) + "\n";
text += "tests (goble-ui/app/desktop-service): " + (if tests_pass { "PASS" } else { "FAIL" }) + "\n\n";
text += "Features (done = owner phase reported implemented & green):\n";
for f in features {
    text += "  - " + f.feature + ": " + (if f.done { "done" } else { "open" }) + "\n";
}

complete(#{
    checks_pass: checks_pass,
    tests_pass: tests_pass,
    report: text,
    features: features,
});
