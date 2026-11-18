import {
  forwardRef,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  Animated,
  Easing,
  Modal,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  useWindowDimensions,
  type PointerEvent as RNPointerEvent,
  type ScrollViewInstance,
  type ViewInstance,
} from "react-native";

import {
  BrandMark,
  StatusPill,
  palette,
  relativeTime,
  space,
  typography,
  type ConnectionState,
  type SessionStatus,
} from "@goble/common";

import { SidebarIcon } from "./sidebar-icons";

/**
 * The left sidebar.
 *
 * The conversation list is DeepSeek Harness's "Workspaces" section ported to
 * `/tmp/goble-work/sidebar-design-spec.md` §3: a 36px section header, 32px
 * group/session rows painted 3px inside the column with 2px gaps inside a group
 * and 4px between groups, fixed radius 12, and the DSH state interaction set:
 * view-options menu, hover card (800ms dwell), hover title marquee, in-memory
 * drag reorder with an accent insert marker, keyboard focus/roving tab stop and
 * a "View all (n)"/"Show less" digest. Goble chrome (brand row, Sessions/Shell
 * navigation, gateway footer) is kept and aligned to the same column metrics.
 *
 * The DSH magnifier is deliberately absent: Goble keeps its own persistent
 * search field (desktop) and the web shell has none to fold into the toolbar.
 */

/** What the sidebar needs from a stored conversation; the pane loads the rest. */
export interface ThreadRow {
  id: string;
  title: string;
  preview: string;
  createdAt?: string;
  updatedAt?: string;
  messageCount?: number;
  /** `live` draws DSH's running ring; `waiting` an amber dot; `done` is idle. */
  status?: SessionStatus;
}

/** One workspace/project group: a folder-like heading and its conversations. */
export interface SidebarGroup {
  id: string;
  label: string;
  threads: ThreadRow[];
}

type Grouping = "folder" | "flat";
type SortOrder = "recent" | "manual";

type MenuState =
  | {
      kind: "thread";
      groupLabel: string;
      left: number;
      top: number;
      thread: ThreadRow;
    }
  | { kind: "views"; left: number; top: number };

interface DragRect {
  id: string;
  y: number;
  height: number;
}

interface DropTarget {
  id: string;
  half: "before" | "after";
  top: number;
}

const SIDEBAR_WIDTH = 280;
const ROW_HEIGHT = 32;
const HEADER_HEIGHT = 36;
const SECTION_TOP_INSET = 8;
const PAINT_INSET = 3;
const ROW_PADDING_START = 9; // 12 - PAINT_INSET
const ROW_PADDING_END = 8;
const LABEL_GAP = 6;
const LEADING_SLOT_W = 16;
const LEADING_SLOT_H = 20;
const ROW_GAP = 2;
const GROUP_GAP = 4;
const LIST_BOTTOM_PAD = 16;
const HEADER_BUTTON = 28;
const HEADER_ICON = 16;
const ACTION_BUTTON = 24;
const HOVER_CARD_WIDTH = 244;
/** The row card's right edge inside the column: width - border - padding - inset. */
const ROW_RIGHT_EDGE =
  SIDEBAR_WIDTH - StyleSheet.hairlineWidth - space.sm - PAINT_INSET;
const HOVER_CARD_LEFT = ROW_RIGHT_EDGE + space.sm;
const HOVER_CARD_DELAY_MS = 800;
const MENU_WIDTH = 200;
const VIEW_MENU_WIDTH = 220;
const MENU_ROW_HEIGHT = 32;
const MENU_PADDING = 6;
const VIEW_ROW_MIN_HEIGHT = 44;
const ROW_RADIUS = 12;
const OVERFLOW_HEIGHT = 28;
const OVERFLOW_INDENT = 34;
const DIGEST_COUNT = 4;
const DRAG_START_DISTANCE = 4;
const DRAG_EDGE = 24;
const DRAG_SCROLL_STEP = 8;
const FOCUS_BORDER = 2;
/** The active conversation's row fill; desktop paints the same #2a2a2a. */
const ROW_SELECTED_FILL = "#2a2a2a";

/**
 * Whether the last input was a keyboard. Focus rings are a keyboard
 * affordance (DSH `:focus-visible`), so a mouse click must not paint one.
 */
let keyboardModality = false;

const STATUS_LABEL: Record<SessionStatus, string> = {
  live: "Running",
  waiting: "Waiting for you",
  done: "Done",
};

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * DSH's compact relative time (`now`, `5min`, `17h`, `3d`, `2mo`, `1y`).
 * A conversation with no stored timestamp renders nothing, as DSH's blank rows
 * do.
 */
function compactTime(iso: string | undefined, now = Date.now()): string {
  if (!iso) return "";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const diff = Math.max(0, now - then);
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (diff < minute) return "now";
  if (diff < hour) return `${Math.floor(diff / minute)}min`;
  if (diff < day) return `${Math.floor(diff / hour)}h`;
  if (diff < 30 * day) return `${Math.floor(diff / day)}d`;
  if (diff < 365 * day) return `${Math.floor(diff / (30 * day))}mo`;
  return `${Math.floor(diff / (365 * day))}y`;
}

/** A session key for focus/roving tab state. */
function sessionKey(id: string): string {
  return `session:${id}`;
}

/** A group key for focus/roving tab state. */
function groupKey(id: string): string {
  return `group:${id}`;
}

function statusColor(status: SessionStatus | undefined): string {
  if (status === "waiting") return palette.warning;
  return palette.muted;
}

/**
 * DSH's running ring: the `status-running` spinner rotated at the same 1.5s
 * linear period DSH uses. The web shell can animate it; the Rust sidebar draws
 * the frozen ring.
 */
function RunningRing({ size = 14 }: { size?: number }) {
  const progress = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    const loop = Animated.loop(
      Animated.timing(progress, {
        duration: 1500,
        easing: Easing.linear,
        toValue: 1,
        useNativeDriver: false,
      }),
    );
    loop.start();
    return () => {
      loop.stop();
    };
  }, [progress]);

  const rotate = progress.interpolate({
    inputRange: [0, 1],
    outputRange: ["0deg", "360deg"],
  });

  return (
    <Animated.View style={{ height: size, transform: [{ rotate }], width: size }}>
      <SidebarIcon color={palette.subtle} name="status-running" size={size} />
    </Animated.View>
  );
}

/** DSH paints solid states as a 6px core inside a 10px layout box. */
function StatusDot({ color, size = 10 }: { color: string; size?: number }) {
  const core = Math.round(size * 0.6);
  return (
    <View
      style={{
        alignItems: "center",
        height: size,
        justifyContent: "center",
        width: size,
      }}
    >
      <View
        style={{ backgroundColor: color, borderRadius: core / 2, height: core, width: core }}
      />
    </View>
  );
}

/** The leading 16x20 slot: running ring, amber "needs you", or nothing. */
function StatusMark({ status }: { status?: SessionStatus }) {
  if (status === "live") return <RunningRing size={14} />;
  if (status === "waiting") return <StatusDot color={palette.warning} />;
  return null;
}

/**
 * Session title cell. At rest it clips with DSH's ellipsis; while the row is
 * hovered (or its tray open) a title wider than the cell crawls to its far edge
 * at DSH's 0.03px/ms, and returns in one step when the pointer leaves. Reduced
 * motion jumps straight to the far edge.
 *
 * The overflow is measured from the rendered text node (`scrollWidth` vs
 * `clientWidth`), which is the exact value CSS ellipsis is hiding.
 */
function MarqueeTitle({
  active,
  style,
  text,
}: {
  active: boolean;
  style: object;
  text: string;
}) {
  const anim = useRef(new Animated.Value(0)).current;
  const ref = useRef<ViewInstance>(null);
  const crawl = useRef<Animated.CompositeAnimation | null>(null);
  const measured = useRef<number | null>(null);
  /** Once crawling, the title box widens to its content so no ellipsis shows. */
  const [contentWidth, setContentWidth] = useState<number | null>(null);

  useEffect(() => {
    if (!active) {
      crawl.current?.stop();
      crawl.current = null;
      measured.current = null;
      setContentWidth(null);
      anim.setValue(0);
      return;
    }
    const node = ref.current as unknown as HTMLElement | null;
    if (node === null) return;

    const measure = () => {
      const overflow = Math.max(0, node.scrollWidth - node.clientWidth);
      if (overflow === measured.current) return;
      measured.current = overflow;
      setContentWidth(overflow > 8 ? node.clientWidth + overflow : null);
      crawl.current?.stop();
      crawl.current = null;
      if (overflow <= 8) {
        anim.setValue(0);
        return;
      }
      const reduced =
        window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
      if (reduced) {
        anim.setValue(-overflow);
        return;
      }
      anim.setValue(0);
      const animation = Animated.timing(anim, {
        duration: Math.max(200, Math.round(overflow / 0.03)),
        easing: Easing.linear,
        toValue: -overflow,
        useNativeDriver: false,
      });
      crawl.current = animation;
      animation.start();
    };

    // Re-measure for the whole hover: a webfont settling after the first paint
    // (or a title that changes while the row is under the pointer) changes the
    // real overflow after the effect first ran.
    const frame = requestAnimationFrame(measure);
    const poll = setInterval(measure, 150);
    return () => {
      cancelAnimationFrame(frame);
      clearInterval(poll);
      crawl.current?.stop();
      crawl.current = null;
    };
  }, [active, anim, text]);

  return (
    <View style={styles.titleCell}>
      <Animated.Text
        numberOfLines={1}
        ref={ref}
        style={[
          style,
          contentWidth === null ? null : { width: contentWidth },
          { transform: [{ translateX: anim }] },
        ]}
      >
        {text}
      </Animated.Text>
    </View>
  );
}

/** One header control: a 28px rounded square with a 16px DSH glyph. */
const ToolbarButton = forwardRef<
  ViewInstance,
  {
    disabled?: boolean;
    label: string;
    name: "project-add" | "sliders-two";
    onPress: () => void;
  }
>(function ToolbarButton({ disabled = false, label, name, onPress }, ref) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);

  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      disabled={disabled}
      onBlur={() => setFocused(false)}
      onFocus={() => setFocused(keyboardModality)}
      onHoverIn={() => setHovered(true)}
      onHoverOut={() => setHovered(false)}
      onPress={onPress}
      ref={ref}
      style={({ pressed }) => [
        styles.toolbarButton,
        hovered && !disabled && styles.toolbarButtonHover,
        disabled && styles.toolbarButtonDisabled,
        pressed && !disabled && styles.pressed,
      ]}
    >
      <SidebarIcon
        color={disabled ? palette.subtle : hovered ? palette.text : palette.muted}
        name={name}
        size={HEADER_ICON}
      />
      {focused ? <View pointerEvents="none" style={styles.focusRing} /> : null}
    </Pressable>
  );
});

/** A tray row: 32px high, 12px label, hover ink `Text`. */
function MenuAction({ label, onPress }: { label: string; onPress: () => void }) {
  const [hovered, setHovered] = useState(false);

  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      onHoverIn={() => setHovered(true)}
      onHoverOut={() => setHovered(false)}
      onPress={onPress}
      style={[styles.menuRow, hovered && styles.menuRowHover]}
    >
      <Text style={[styles.menuLabel, hovered && styles.menuLabelHover]}>
        {label}
      </Text>
    </Pressable>
  );
}

/**
 * One view-options row: icon, label + description, trailing accent check on the
 * selected option (DSH ViewOptionsMenu, spec §3.10).
 */
function ViewOptionRow({
  description,
  icon,
  label,
  onPress,
  selected,
}: {
  description: string;
  icon: "folder-close" | "flat-list" | "clock" | "chevrons-up-down";
  label: string;
  onPress: () => void;
  selected: boolean;
}) {
  const [hovered, setHovered] = useState(false);

  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="menuitemradio"
      accessibilityState={{ checked: selected }}
      onHoverIn={() => setHovered(true)}
      onHoverOut={() => setHovered(false)}
      onPress={onPress}
      style={[styles.viewRow, hovered && styles.menuRowHover]}
    >
      <SidebarIcon
        color={selected ? palette.accent : palette.muted}
        name={icon}
        size={16}
      />
      <View style={styles.viewRowText}>
        <Text style={[styles.viewLabel, selected && styles.viewLabelSelected]}>
          {label}
        </Text>
        <Text numberOfLines={1} style={styles.viewDescription}>
          {description}
        </Text>
      </View>
      {selected ? (
        <SidebarIcon color={palette.text} name="check" size={14} />
      ) : null}
    </Pressable>
  );
}

/** A small floating panel, drawn beside the sidebar like DSH's fixed menus. */
function MenuPanel({
  children,
  left,
  onClose,
  top,
  visible,
  width,
}: {
  children: ReactNode;
  left: number;
  onClose: () => void;
  top: number;
  visible: boolean;
  width: number;
}) {
  return (
    <Modal
      animationType="fade"
      onRequestClose={onClose}
      statusBarTranslucent
      transparent
      visible={visible}
    >
      <Pressable
        accessibilityLabel="Close menu"
        onPress={onClose}
        style={StyleSheet.absoluteFill}
      />
      <View style={[styles.menuCard, { left, top, width }]}>{children}</View>
    </Modal>
  );
}

export function Sidebar({
  activeThreadId,
  canShell,
  groups,
  label,
  onNewShell,
  onOpenShell,
  onSelectThread,
  onShowSessions,
  server,
  shellOpen,
  state,
}: {
  activeThreadId: string | null;
  canShell: boolean;
  groups: SidebarGroup[];
  /** Gateway pill label (`Gateway live`, `Demo workspace`, …). */
  label: string;
  onNewShell: () => void;
  onOpenShell: () => void;
  onSelectThread: (id: string) => void;
  onShowSessions: () => void;
  server: string | null;
  /** The shell pane is the open view (highlights its navigation row). */
  shellOpen: boolean;
  state: ConnectionState;
}) {
  const { height: windowHeight, width: windowWidth } = useWindowDimensions();
  const rootRef = useRef<ViewInstance>(null);
  const viewsButtonRef = useRef<ViewInstance>(null);
  const listRef = useRef<ScrollViewInstance>(null);
  const listWrapRef = useRef<ViewInstance>(null);
  const rowRefs = useRef<Record<string, ViewInstance | null>>({});
  const listRect = useRef<{ top: number; bottom: number } | null>(null);
  const scrollOffset = useRef(0);
  const suppressClick = useRef(false);
  const hoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [hoveredKey, setHoveredKey] = useState<string | null>(null);
  const [focusedKey, setFocusedKey] = useState<string | null>(null);
  /** The 2px ring is a keyboard affordance only (DSH `:focus-visible`). */
  const [ringVisible, setRingVisible] = useState(false);
  const [hoverCard, setHoverCard] = useState<{
    groupLabel: string;
    thread: ThreadRow;
    top: number;
  } | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [copied, setCopied] = useState(false);
  const [headerFocused, setHeaderFocused] = useState(false);
  const [navFocused, setNavFocused] = useState(false);
  const [newRowHovered, setNewRowHovered] = useState(false);
  const [newRowFocused, setNewRowFocused] = useState(false);
  const [overflowFocused, setOverflowFocused] = useState(false);

  // View options (per-session UI state, spec §3.10).
  const [grouping, setGrouping] = useState<Grouping>("folder");
  const [sort, setSort] = useState<SortOrder>("recent");
  const [manualOrder, setManualOrder] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);

  // Drag reorder (in-memory manual order, spec §3.13).
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null);
  const [markerTop, setMarkerTop] = useState<number | null>(null);
  const dragRects = useRef<DragRect[]>([]);
  const dragRootY = useRef(0);
  const dragScrollStart = useRef(0);
  const dragGroup = useRef<string | null>(null);

  const overview = !shellOpen && activeThreadId === null;

  const clearHoverTimer = useCallback(() => {
    if (hoverTimer.current !== null) {
      clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
  }, []);

  useEffect(() => clearHoverTimer, [clearHoverTimer]);

  // The rows paint their own 2px inset focus ring; drop the browser's default
  // outline so keyboard focus never shows a double ring.
  useEffect(() => {
    if (typeof document === "undefined") return;
    const node = rootRef.current as unknown as HTMLElement | null;
    if (node === null) return;
    node.setAttribute("data-goble-sidebar", "");
    const styleId = "goble-sidebar-focus";
    if (document.getElementById(styleId) === null) {
      const style = document.createElement("style");
      style.id = styleId;
      style.textContent =
        '[data-goble-sidebar] [role="button"]:focus { outline: none; }';
      document.head.appendChild(style);
    }
  }, []);

  // Track whether the last input was a keyboard, so a mouse click does not
  // leave DSH's 2px Focus ring behind (DSH draws it on :focus-visible).
  useEffect(() => {
    if (typeof document === "undefined") return;
    const onPointerDown = () => {
      keyboardModality = false;
      setRingVisible(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      keyboardModality = true;
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, []);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(timer);
  }, [copied]);

  // Escape closes whichever card is open, as it does in DSH's menus.
  useEffect(() => {
    if (menu === null || typeof window === "undefined") return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenu(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [menu]);

  /** Sort one group's rows by the selected order. */
  const orderThreads = useCallback(
    (threads: ThreadRow[]): ThreadRow[] => {
      if (sort === "manual" && manualOrder.length > 0) {
        const index = new Map(manualOrder.map((id, position) => [id, position]));
        return [...threads].sort(
          (a, b) =>
            (index.get(a.id) ?? Number.MAX_SAFE_INTEGER) -
            (index.get(b.id) ?? Number.MAX_SAFE_INTEGER),
        );
      }
      return [...threads].sort(
        (a, b) => Date.parse(b.updatedAt ?? "") - Date.parse(a.updatedAt ?? ""),
      );
    },
    [manualOrder, sort],
  );

  /** The groups in display order with their rows sorted. */
  const orderedGroups = useMemo(
    () =>
      groups
        .map((group) => ({
          ...group,
          threads: orderThreads(group.threads),
        }))
        .filter((group) => group.threads.length > 0),
    [groups, orderThreads],
  );

  const flatThreads = useMemo(
    () => orderThreads(orderedGroups.flatMap((group) => group.threads)),
    [orderedGroups, orderThreads],
  );

  const totalSessions = flatThreads.length;

  /**
   * Digest: the first `DIGEST_COUNT` conversations across the list, or every
   * conversation once "View all (n)" was pressed.
   */
  const visibleByGroup = useMemo(() => {
    const visible = new Map<string, ThreadRow[]>();
    let budget = expanded ? Number.MAX_SAFE_INTEGER : DIGEST_COUNT;
    for (const group of orderedGroups) {
      if (budget <= 0) {
        visible.set(group.id, []);
        continue;
      }
      const rows = group.threads.slice(0, budget);
      budget -= rows.length;
      visible.set(group.id, rows);
    }
    return visible;
  }, [expanded, orderedGroups]);

  const visibleFlat = useMemo(
    () => flatThreads.slice(0, expanded ? flatThreads.length : DIGEST_COUNT),
    [expanded, flatThreads],
  );

  /** Every keyboard-reachable row key, in paint order. */
  const visibleKeys = useMemo(() => {
    if (grouping === "flat") return visibleFlat.map((thread) => sessionKey(thread.id));
    const keys: string[] = [];
    for (const group of orderedGroups) {
      const rows = visibleByGroup.get(group.id) ?? [];
      if (collapsed[group.id] === true) {
        keys.push(groupKey(group.id));
        continue;
      }
      if (rows.length === 0) continue;
      keys.push(groupKey(group.id));
      for (const thread of rows) keys.push(sessionKey(thread.id));
    }
    return keys;
  }, [collapsed, grouping, orderedGroups, visibleByGroup, visibleFlat]);

  const tabStop =
    focusedKey !== null && visibleKeys.includes(focusedKey)
      ? focusedKey
      : (visibleKeys[0] ?? null);

  /**
   * Dwell on a row for 800ms to open its hover card beside the sidebar, the way
   * DSH's fixed-position card sits next to its anchor. Suppressed while the
   * row's tray or a drag is active.
   */
  const enterRow = useCallback(
    (thread: ThreadRow, groupLabel: string) => {
      setHoveredKey(sessionKey(thread.id));
      clearHoverTimer();
      if (menu !== null) return;
      hoverTimer.current = setTimeout(() => {
        const row = rowRefs.current[sessionKey(thread.id)];
        const root = rootRef.current;
        if (!row || !root) return;
        row.measureInWindow((_x, y, _w, h) => {
          root.measureInWindow((_rx, rootY, _rw, rootHeight) => {
            const estimatedCardHeight = 132;
            if (h <= 0 || y < rootY || y > rootY + rootHeight) return;
            setHoverCard({
              groupLabel,
              thread,
              top: clamp(
                y - rootY - 4,
                6,
                Math.max(6, rootHeight - estimatedCardHeight - 8),
              ),
            });
          });
        });
      }, HOVER_CARD_DELAY_MS);
    },
    [clearHoverTimer, menu],
  );

  const leaveRow = useCallback(() => {
    clearHoverTimer();
    setHoveredKey(null);
    setHoverCard(null);
  }, [clearHoverTimer]);

  /** Open a row's tray under its painted card, clamped to the window. */
  const openThreadMenu = useCallback(
    (thread: ThreadRow, groupLabel: string) => {
      clearHoverTimer();
      setHoverCard(null);
      const row = rowRefs.current[sessionKey(thread.id)];
      const open = (x: number, y: number, width: number, height: number) => {
        setMenu({
          groupLabel,
          kind: "thread",
          left: clamp(
            x + width - MENU_WIDTH,
            8,
            Math.max(8, windowWidth - MENU_WIDTH - 8),
          ),
          top: clamp(
            y + height - 4,
            8,
            Math.max(
              8,
              windowHeight - (MENU_ROW_HEIGHT * 2 + MENU_PADDING * 2) - 8,
            ),
          ),
          thread,
        });
      };
      if (row) row.measureInWindow(open);
      else open(SIDEBAR_WIDTH - MENU_WIDTH, 44, 0, 0);
    },
    [clearHoverTimer, windowHeight, windowWidth],
  );

  /** The sliders button opens the DSH view-options menu (spec §3.10). */
  const openViewsMenu = useCallback(() => {
    clearHoverTimer();
    setHoverCard(null);
    const anchor = viewsButtonRef.current;
    const menuHeight = VIEW_ROW_MIN_HEIGHT * 4 + MENU_PADDING * 2;
    const open = (x: number, y: number, width: number, height: number) => {
      setMenu({
        kind: "views",
        left: clamp(
          x + width - VIEW_MENU_WIDTH,
          8,
          Math.max(8, windowWidth - VIEW_MENU_WIDTH - 8),
        ),
        top: clamp(
          y + height + 4,
          8,
          Math.max(8, windowHeight - menuHeight - 8),
        ),
      });
    };
    if (anchor) anchor.measureInWindow(open);
    else open(SIDEBAR_WIDTH - 8 - VIEW_MENU_WIDTH, HEADER_HEIGHT, 0, 0);
  }, [clearHoverTimer, windowHeight, windowWidth]);

  /** Focus a row element and make it the roving tab stop. */
  const focusRow = useCallback((key: string) => {
    const node = rowRefs.current[key] as unknown as { focus?: () => void } | null;
    node?.focus?.();
    keyboardModality = true;
    setFocusedKey(key);
    setRingVisible(true);
  }, []);

  // Roving keyboard focus inside the list (spec §3.14).
  const keyboardRef = useRef<(event: KeyboardEvent) => void>(() => undefined);
  keyboardRef.current = (event: KeyboardEvent) => {
    const rowNodes = Object.values(rowRefs.current).filter(
      (node): node is ViewInstance => node !== null,
    ) as unknown as HTMLElement[];
    const active = typeof document === "undefined" ? null : document.activeElement;
    if (active === null || !rowNodes.includes(active as HTMLElement)) return;
    const keys = visibleKeys;
    if (keys.length === 0) return;
    const currentIndex = focusedKey === null ? -1 : keys.indexOf(focusedKey);
    const move = (index: number) => {
      const next = clamp(index, 0, keys.length - 1);
      const key = keys[next];
      if (key !== undefined) focusRow(key);
    };
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        move(currentIndex + 1);
        return;
      case "ArrowUp":
        event.preventDefault();
        move(currentIndex <= 0 ? 0 : currentIndex - 1);
        return;
      case "Home":
        event.preventDefault();
        move(0);
        return;
      case "End":
        event.preventDefault();
        move(keys.length - 1);
        return;
      case "Enter":
      case " ":
        if (focusedKey === null) return;
        event.preventDefault();
        if (focusedKey.startsWith("session:")) {
          onSelectThread(focusedKey.slice("session:".length));
        } else {
          const id = focusedKey.slice("group:".length);
          setCollapsed((current) => ({ ...current, [id]: current[id] !== true }));
        }
        return;
      case "Escape":
        setFocusedKey(null);
        setRingVisible(false);
        (active as HTMLElement).blur?.();
        return;
      case "ContextMenu":
        if (focusedKey?.startsWith("session:")) {
          event.preventDefault();
          const thread = findThread(focusedKey.slice("session:".length));
          if (thread) openThreadMenu(thread, findGroupLabel(thread.id));
        }
        return;
      case "F10":
        if (event.shiftKey && focusedKey?.startsWith("session:")) {
          event.preventDefault();
          const thread = findThread(focusedKey.slice("session:".length));
          if (thread) openThreadMenu(thread, findGroupLabel(thread.id));
        }
        return;
      default:
        return;
    }
  };

  useEffect(() => {
    const node = rootRef.current as unknown as HTMLElement | null;
    if (node === null || node?.addEventListener === undefined) return;
    const onKeyDown = (event: KeyboardEvent) => keyboardRef.current(event);
    node.addEventListener("keydown", onKeyDown);
    return () => node.removeEventListener("keydown", onKeyDown);
  }, []);

  function findThread(id: string): ThreadRow | undefined {
    return flatThreads.find((thread) => thread.id === id);
  }

  function findGroupLabel(id: string): string {
    return (
      orderedGroups.find((group) => group.threads.some((thread) => thread.id === id))
        ?.label ?? ""
    );
  }

  /** The ids of the drag scope: the whole flat list, or the row's group. */
  function dragScopeIds(groupId: string | null): string[] {
    if (grouping === "flat" || groupId === null) {
      return visibleFlat.map((thread) => thread.id);
    }
    const group = orderedGroups.find((candidate) => candidate.id === groupId);
    return (visibleByGroup.get(groupId) ?? group?.threads ?? []).map(
      (thread) => thread.id,
    );
  }

  /**
   * Pointer-driven reorder (spec §3.13): >4px of travel starts the drag, the
   * row lifts, an accent insert marker follows the target boundary, and the
   * drop commits the new order into the in-memory manual order and switches the
   * sort to Manual. Cross-group drops are not offered: group membership comes
   * from the conversations' project names, so moving a row to another group
   * would fabricate a project change.
   */
  function onRowPointerDown(
    thread: ThreadRow,
    groupId: string | null,
    event: RNPointerEvent,
  ) {
    const startY = event.nativeEvent.clientY;
    const startX = event.nativeEvent.clientX;
    const scope = dragScopeIds(groupId);
    const rects: DragRect[] = [];
    for (const id of scope) {
      const node = rowRefs.current[sessionKey(id)];
      node?.measureInWindow((_x, y, _w, height) => {
        rects.push({ height, id, y });
      });
    }
    dragRects.current = rects;
    dragGroup.current = groupId;
    dragScrollStart.current = scrollOffset.current;
    const root = rootRef.current;
    root?.measureInWindow((_x, y) => {
      dragRootY.current = y;
    });
    const container = listWrapRef.current as unknown as {
      measureInWindow?: (
        callback: (x: number, y: number, width: number, height: number) => void,
      ) => void;
    } | null;
    container?.measureInWindow?.((_x, y, _w, height) => {
      listRect.current = { bottom: y + height, top: y };
    });

    let dragging = false;
    let target: { id: string; half: "before" | "after" } | null = null;

    const onMove = (move: globalThis.PointerEvent) => {
      if (!dragging) {
        const distance =
          Math.abs(move.clientY - startY) + Math.abs(move.clientX - startX);
        if (distance <= DRAG_START_DISTANCE) return;
        dragging = true;
        suppressClick.current = true;
        setDragId(thread.id);
        setHoveredKey(null);
        setHoverCard(null);
        clearHoverTimer();
      }
      const scrollDelta = scrollOffset.current - dragScrollStart.current;
      let boundary: DropTarget | null = null;
      for (const rect of dragRects.current) {
        const top = rect.y - scrollDelta;
        if (move.clientY < top + rect.height / 2) {
          boundary = { half: "before", id: rect.id, top };
          break;
        }
        boundary = { half: "after", id: rect.id, top: top + rect.height };
      }
      target = boundary === null ? null : { half: boundary.half, id: boundary.id };
      if (boundary !== null && boundary.id !== thread.id) {
        const groupIdForTarget = findGroupOf(boundary.id);
        if (grouping === "flat" || groupIdForTarget === groupId) {
          setDropTarget(boundary);
          setMarkerTop(boundary.top - dragRootY.current - 6);
        } else {
          setDropTarget(null);
          setMarkerTop(null);
        }
      } else {
        setDropTarget(null);
        setMarkerTop(null);
      }
      // Auto-scroll near the list's edges.
      if (listRect.current !== null) {
        if (move.clientY < listRect.current.top + DRAG_EDGE) {
          scrollList(-DRAG_SCROLL_STEP);
        } else if (move.clientY > listRect.current.bottom - DRAG_EDGE) {
          scrollList(DRAG_SCROLL_STEP);
        }
      }
    };

    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      if (dragging) {
        commitDrag(thread.id, target, scope);
      }
      setDragId(null);
      setDropTarget(null);
      setMarkerTop(null);
      dragRects.current = [];
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  function scrollList(delta: number) {
    const next = Math.max(0, scrollOffset.current + delta);
    if (next === scrollOffset.current) return;
    scrollOffset.current = next;
    listRef.current?.scrollTo({ animated: false, y: next });
  }

  function findGroupOf(threadId: string): string | null {
    return (
      orderedGroups.find((group) =>
        group.threads.some((thread) => thread.id === threadId),
      )?.id ?? null
    );
  }

  function commitDrag(
    draggedId: string,
    target: { id: string; half: "before" | "after" } | null,
    scope: string[],
  ) {
    if (target === null) return;
    const base = manualOrder.length > 0 ? [...manualOrder] : currentSessionOrder();
    const without = base.filter((id) => id !== draggedId);
    const targetIndex = without.indexOf(target.id);
    const insertAt =
      targetIndex === -1
        ? without.length
        : targetIndex + (target.half === "after" ? 1 : 0);
    without.splice(insertAt, 0, draggedId);
    setManualOrder(without);
    setSort("manual");
    // Referenced so the drag scope is part of the commit contract.
    void scope;
  }

  /** Every conversation in paint order across groups, for the manual base. */
  function currentSessionOrder(): string[] {
    if (grouping === "flat") return visibleFlat.map((thread) => thread.id);
    return orderedGroups.flatMap((group) =>
      group.threads.map((thread) => thread.id),
    );
  }

  async function copyTitle(title: string) {
    if (typeof navigator === "undefined" || navigator.clipboard === undefined) {
      return;
    }
    try {
      await navigator.clipboard.writeText(title);
      setCopied(true);
    } catch {
      // A denied clipboard permission leaves the title where it was.
    }
  }

  function renderThreadRow(thread: ThreadRow, groupLabel: string, groupId: string) {
    const key = sessionKey(thread.id);
    const active = thread.id === activeThreadId;
    const hovered = hoveredKey === key;
    const focused = focusedKey === key;
    const menuOpen = menu?.kind === "thread" && menu.thread.id === thread.id;
    const dragging = dragId === thread.id;
    const time = compactTime(thread.updatedAt);
    const showRing = focused && ringVisible;
    const showActions = hovered || menuOpen || showRing;

    return (
      <Pressable
        accessibilityLabel={thread.title || "Untitled conversation"}
        accessibilityRole="button"
        accessibilityState={{ selected: active }}
        key={key}
        onFocus={() => {
          setFocusedKey(key);
          if (keyboardModality) setRingVisible(true);
        }}
        onHoverIn={() => enterRow(thread, groupLabel)}
        onHoverOut={leaveRow}
        onPointerDown={(event) => onRowPointerDown(thread, groupId, event)}
        onPress={() => {
          setFocusedKey(key);
          if (suppressClick.current) {
            suppressClick.current = false;
            return;
          }
          onSelectThread(thread.id);
        }}
        ref={(node) => {
          rowRefs.current[key] = node;
          (node as unknown as HTMLElement | null)?.setAttribute?.(
            "data-goble-row",
            "true",
          );
        }}
        style={({ pressed }) => [
          styles.row,
          hovered && !active && !menuOpen && styles.rowHover,
          menuOpen && !active && styles.rowSelected,
          active && styles.rowSelected,
          dragging && styles.rowLifted,
          pressed && styles.pressed,
        ]}
        tabIndex={tabStop === key ? 0 : -1}
      >
        <View style={styles.leadingSlot}>
          <StatusMark status={thread.status} />
        </View>
        <MarqueeTitle
          active={(hovered || menuOpen) && !dragging}
          style={styles.sessionTitle}
          text={thread.title || "Untitled conversation"}
        />
        {dragging ? null : showActions ? (
          <Pressable
            accessibilityLabel={`Open menu for ${thread.title || "untitled conversation"}`}
            accessibilityRole="button"
            onHoverIn={() => setHoveredKey(key)}
            onPress={(event) => {
              event.stopPropagation();
              openThreadMenu(thread, groupLabel);
            }}
            style={({ pressed }) => [
              styles.rowAction,
              pressed && styles.actionPressed,
            ]}
          >
            <MenuEllipsis />
          </Pressable>
        ) : time ? (
          <Text style={styles.time}>{time}</Text>
        ) : null}
        {showRing ? <View pointerEvents="none" style={styles.focusRing} /> : null}
      </Pressable>
    );
  }

  function renderGroupRow(group: SidebarGroup, active: boolean) {
    const key = groupKey(group.id);
    const isCollapsed = collapsed[group.id] === true;
    const hovered = hoveredKey === key;
    const focused = focusedKey === key;
    const showRing = focused && ringVisible;

    return (
      <Pressable
        accessibilityLabel={`${group.label}${isCollapsed ? ", collapsed" : ""}`}
        accessibilityRole="button"
        accessibilityState={{ expanded: !isCollapsed }}
        onFocus={() => {
          setFocusedKey(key);
          if (keyboardModality) setRingVisible(true);
        }}
        onHoverIn={() => setHoveredKey(key)}
        onHoverOut={() => setHoveredKey(null)}
        onPress={() => {
          setFocusedKey(key);
          setCollapsed((current) => ({
            ...current,
            [group.id]: !isCollapsed,
          }));
        }}
        ref={(node) => {
          rowRefs.current[key] = node;
          (node as unknown as HTMLElement | null)?.setAttribute?.(
            "data-goble-row",
            "true",
          );
        }}
        style={({ pressed }) => [
          styles.row,
          hovered && styles.rowHover,
          pressed && styles.pressed,
          focused && styles.rowFocused,
        ]}
        tabIndex={tabStop === key ? 0 : -1}
      >
        <View style={styles.leadingSlot}>
          {hovered ? (
            <SidebarIcon
              color={palette.muted}
              name={isCollapsed ? "triangle-right-fill" : "triangle-down-fill"}
              size={14}
            />
          ) : (
            <SidebarIcon
              color={active ? palette.accent : palette.subtle}
              name={isCollapsed ? "folder-close" : "folder-open"}
              size={16}
            />
          )}
        </View>
        <Text numberOfLines={1} style={styles.groupTitle}>
          {group.label}
        </Text>
        {showRing ? <View pointerEvents="none" style={styles.focusRing} /> : null}
      </Pressable>
    );
  }

  const hasConversations = totalSessions > 0;
  const showOverflow = totalSessions > DIGEST_COUNT;

  return (
    <View ref={rootRef} style={styles.root}>
      <View style={styles.brandRow}>
        <BrandMark size={30} />
        <View style={styles.brandText}>
          <Text style={styles.brand}>Goble</Text>
          <Text style={styles.brandMeta}>Desktop</Text>
        </View>
      </View>

      <View style={styles.nav}>
        <Pressable
          accessibilityRole="button"
          accessibilityState={{ selected: overview }}
          onBlur={() => setNavFocused(false)}
          onFocus={() => setNavFocused(keyboardModality)}
          onPress={onShowSessions}
          style={({ pressed }) => [
            styles.navRow,
            overview && styles.navRowActive,
            pressed && styles.pressed,
          ]}
        >
          <View style={styles.navDot} />
          <Text style={styles.navLabel}>Sessions</Text>
          {navFocused ? (
            <View pointerEvents="none" style={styles.focusRing} />
          ) : null}
        </Pressable>
        {shellOpen ? (
          <View style={[styles.navRow, styles.navRowActive]}>
            <View style={styles.navDot} />
            <Text style={styles.navLabel}>Shell</Text>
          </View>
        ) : null}
      </View>

      {/*
       * The owner's "New conversation" row, matching the desktop shell: a
       * borderless 32px row, radius 12, full column width (8..272 inside the
       * 280px column), leading 16px new-chat glyph, 14px Text label, Hover
       * fill. It is the same create action as the header's square-plus.
       */}
      <Pressable
        accessibilityLabel="New conversation"
        accessibilityRole="button"
        accessibilityState={{ disabled: !canShell }}
        disabled={!canShell}
        onBlur={() => setNewRowFocused(false)}
        onFocus={() => setNewRowFocused(keyboardModality)}
        onHoverIn={() => setNewRowHovered(true)}
        onHoverOut={() => setNewRowHovered(false)}
        onPress={onNewShell}
        style={({ pressed }) => [
          styles.newConversation,
          newRowHovered && canShell && styles.newConversationHover,
          !canShell && styles.newConversationDisabled,
          pressed && canShell && styles.pressed,
        ]}
      >
        <SidebarIcon
          color={canShell ? palette.muted : palette.subtle}
          name="new-chat"
          size={16}
        />
        <Text
          style={[
            styles.newConversationLabel,
            !canShell && styles.newConversationLabelDisabled,
          ]}
        >
          New conversation
        </Text>
        {newRowFocused ? (
          <View pointerEvents="none" style={styles.focusRing} />
        ) : null}
      </Pressable>

      <View style={styles.section}>
        <View style={styles.header}>
          <Pressable
            accessibilityLabel="Show all sessions"
            accessibilityRole="button"
            onBlur={() => setHeaderFocused(false)}
            onFocus={() => setHeaderFocused(keyboardModality)}
            onPress={onShowSessions}
            style={styles.headerTitlePress}
          >
            <Text numberOfLines={1} style={styles.headerTitle}>
              Workspaces
            </Text>
            {headerFocused ? (
              <View pointerEvents="none" style={styles.focusRing} />
            ) : null}
          </Pressable>
          <View style={styles.headerActions}>
            <ToolbarButton
              label="Sidebar view options"
              name="sliders-two"
              onPress={openViewsMenu}
              ref={viewsButtonRef}
            />
            <ToolbarButton
              disabled={!canShell}
              label="New shell"
              name="project-add"
              onPress={onNewShell}
            />
          </View>
        </View>

        <View ref={listWrapRef} style={styles.scrollWrap}>
          <ScrollView
            contentContainerStyle={styles.list}
            onScroll={(event) => {
              scrollOffset.current = event.nativeEvent.contentOffset.y;
            }}
            ref={listRef}
            scrollEventThrottle={16}
            showsVerticalScrollIndicator={false}
            style={styles.scroll}
          >
          {hasConversations ? (
            grouping === "flat" ? (
              visibleFlat.map((thread) =>
                renderThreadRow(thread, findGroupLabel(thread.id), findGroupOf(thread.id) ?? ""),
              )
            ) : (
              orderedGroups.map((group) => {
                const rows = visibleByGroup.get(group.id) ?? [];
                if (rows.length === 0) return null;
                const active = group.threads.some(
                  (thread) => thread.id === activeThreadId,
                );
                return (
                  <View key={group.id} style={styles.groupSection}>
                    {renderGroupRow(group, active)}
                    {collapsed[group.id] === true
                      ? null
                      : rows.map((thread) =>
                          renderThreadRow(thread, group.label, group.id),
                        )}
                  </View>
                );
              })
            )
          ) : (
            <Text style={styles.empty}>
              {canShell
                ? "No conversations yet."
                : "No worker connected. Pair a gateway to list its conversations."}
            </Text>
          )}

          {showOverflow ? (
            <Pressable
              accessibilityLabel={expanded ? "Show less" : `View all ${totalSessions}`}
              accessibilityRole="button"
              onBlur={() => setOverflowFocused(false)}
              onFocus={() => setOverflowFocused(keyboardModality)}
              onPress={() => setExpanded((current) => !current)}
              style={({ pressed }) => [
                styles.overflow,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.overflowLabel}>
                {expanded ? "Show less" : `View all (${totalSessions})`}
              </Text>
              {overflowFocused ? (
                <View pointerEvents="none" style={styles.focusRing} />
              ) : null}
            </Pressable>
          ) : null}
          </ScrollView>
        </View>
      </View>

      {markerTop !== null && dropTarget !== null ? (
        <View pointerEvents="none" style={[styles.dropMarker, { top: markerTop }]}>
          <View style={styles.dropLine} />
          <View style={styles.dropArrow}>
            <SidebarIcon color={palette.accent} name="triangle-right-fill" size={14} />
          </View>
        </View>
      ) : null}

      {hoverCard ? (
        <View pointerEvents="none" style={[styles.hoverCard, { top: hoverCard.top }]}>
          <Text numberOfLines={2} style={styles.hoverTitle}>
            {hoverCard.thread.title || "Untitled conversation"}
          </Text>
          <Text numberOfLines={1} style={styles.hoverBody}>
            {hoverCard.groupLabel}
          </Text>
          {hoverCard.thread.updatedAt ? (
            <Text style={styles.hoverMeta}>
              {relativeTime(hoverCard.thread.updatedAt)}
            </Text>
          ) : null}
          <View style={styles.hoverStatus}>
            <StatusDot color={statusColor(hoverCard.thread.status)} />
            <Text style={styles.hoverStatusLabel}>
              {STATUS_LABEL[hoverCard.thread.status ?? "done"]}
            </Text>
          </View>
        </View>
      ) : null}

      <View style={styles.footer}>
        <Text style={styles.footerLabel}>Gateway</Text>
        <StatusPill label={label} state={state} />
        <Text numberOfLines={1} style={styles.server}>
          {server ?? "No gateway paired"}
        </Text>
      </View>

      <MenuPanel
        left={menu?.left ?? 0}
        onClose={() => setMenu(null)}
        top={menu?.top ?? 0}
        visible={menu !== null}
        width={menu?.kind === "views" ? VIEW_MENU_WIDTH : MENU_WIDTH}
      >
        {menu?.kind === "thread" ? (
          <>
            <MenuAction
              label="Open conversation"
              onPress={() => {
                const thread = menu.thread;
                setMenu(null);
                onSelectThread(thread.id);
              }}
            />
            <MenuAction
              label={copied ? "Copied" : "Copy title"}
              onPress={() => {
                void copyTitle(menu.thread.title);
              }}
            />
          </>
        ) : menu?.kind === "views" ? (
          <>
            <ViewOptionRow
              description="Group by folder"
              icon="folder-close"
              label="Folder"
              onPress={() => {
                setGrouping("folder");
                setMenu(null);
              }}
              selected={grouping === "folder"}
            />
            <ViewOptionRow
              description="One list, no folder headers"
              icon="flat-list"
              label="Flat list"
              onPress={() => {
                setGrouping("flat");
                setMenu(null);
              }}
              selected={grouping === "flat"}
            />
            <ViewOptionRow
              description="Newest first"
              icon="clock"
              label="Recent first"
              onPress={() => {
                setSort("recent");
                setMenu(null);
              }}
              selected={sort === "recent"}
            />
            <ViewOptionRow
              description="Drag rows to reorder"
              icon="chevrons-up-down"
              label="Manual order"
              onPress={() => {
                setSort("manual");
                setMenu(null);
              }}
              selected={sort === "manual"}
            />
          </>
        ) : null}
      </MenuPanel>
    </View>
  );
}

/** The ellipsis glyph with a muted→text ink swap on its own hover. */
function MenuEllipsis() {
  const [hovered, setHovered] = useState(false);
  return (
    <View
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <SidebarIcon
        color={hovered ? palette.text : palette.muted}
        name="ellipsis"
        size={16}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  root: {
    backgroundColor: palette.surface,
    borderRightColor: palette.border,
    borderRightWidth: StyleSheet.hairlineWidth,
    flexDirection: "column",
    paddingHorizontal: space.sm,
    paddingTop: 6,
    paddingBottom: 6,
    position: "relative",
    width: SIDEBAR_WIDTH,
    zIndex: 10,
  },
  brandRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    height: 40,
    paddingHorizontal: space.md,
  },
  brandText: {
    gap: 1,
  },
  brand: {
    ...typography.title,
    color: palette.text,
    fontSize: 21,
    letterSpacing: -0.5,
  },
  brandMeta: {
    ...typography.eyebrow,
    color: palette.subtle,
  },
  nav: {
    gap: 2,
    marginTop: space.xs,
  },
  navRow: {
    alignItems: "center",
    borderRadius: ROW_RADIUS,
    flexDirection: "row",
    gap: LABEL_GAP,
    height: ROW_HEIGHT,
    marginHorizontal: PAINT_INSET,
    paddingHorizontal: ROW_PADDING_START,
    paddingRight: ROW_PADDING_END,
  },
  navRowActive: {
    backgroundColor: palette.raised,
  },
  navDot: {
    backgroundColor: palette.accent,
    borderRadius: 3,
    height: 6,
    width: 6,
  },
  navLabel: {
    color: palette.text,
    fontSize: 14,
    fontWeight: "400",
    lineHeight: 20,
  },
  newConversation: {
    alignItems: "center",
    borderRadius: ROW_RADIUS,
    flexDirection: "row",
    gap: LABEL_GAP,
    height: ROW_HEIGHT,
    marginTop: SECTION_TOP_INSET,
    paddingHorizontal: space.md,
  },
  newConversationHover: {
    backgroundColor: palette.raised,
  },
  newConversationDisabled: {
    opacity: 0.45,
  },
  newConversationLabel: {
    color: palette.text,
    fontSize: 14,
    lineHeight: 20,
  },
  newConversationLabelDisabled: {
    color: palette.subtle,
  },
  section: {
    flex: 1,
    marginTop: SECTION_TOP_INSET,
    minHeight: 0,
  },
  header: {
    alignItems: "center",
    flexDirection: "row",
    height: HEADER_HEIGHT,
    paddingLeft: space.md,
  },
  headerTitlePress: {
    flex: 1,
    justifyContent: "center",
    minWidth: 0,
  },
  headerTitle: {
    color: palette.subtle,
    fontSize: 14,
    lineHeight: 20,
    maxWidth: "45%",
  },
  headerActions: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.xs,
  },
  toolbarButton: {
    alignItems: "center",
    borderRadius: ROW_RADIUS,
    height: HEADER_BUTTON,
    justifyContent: "center",
    width: HEADER_BUTTON,
  },
  toolbarButtonHover: {
    backgroundColor: palette.raised,
  },
  toolbarButtonDisabled: {
    opacity: 0.45,
  },
  pressed: {
    opacity: 0.7,
  },
  scrollWrap: {
    flex: 1,
    minHeight: 0,
  },
  scroll: {
    flex: 1,
  },
  list: {
    gap: GROUP_GAP,
    paddingBottom: LIST_BOTTOM_PAD,
    paddingHorizontal: PAINT_INSET,
  },
  groupSection: {
    gap: ROW_GAP,
  },
  row: {
    alignItems: "center",
    borderRadius: ROW_RADIUS,
    flexDirection: "row",
    height: ROW_HEIGHT,
    paddingLeft: ROW_PADDING_START,
    paddingRight: ROW_PADDING_END,
  },
  rowHover: {
    backgroundColor: palette.raised,
  },
  rowSelected: {
    backgroundColor: ROW_SELECTED_FILL,
  },
  rowFocused: {
    backgroundColor: "transparent",
  },
  rowLifted: {
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderWidth: 1,
  },
  focusRing: {
    borderColor: palette.accent,
    borderRadius: ROW_RADIUS,
    borderWidth: FOCUS_BORDER,
    bottom: 1,
    left: 1,
    position: "absolute",
    right: 1,
    top: 1,
  },
  leadingSlot: {
    alignItems: "center",
    height: LEADING_SLOT_H,
    justifyContent: "center",
    width: LEADING_SLOT_W,
  },
  groupTitle: {
    color: palette.text,
    flex: 1,
    fontSize: 14,
    lineHeight: 20,
    marginLeft: LABEL_GAP,
    minWidth: 0,
  },
  titleCell: {
    flex: 1,
    height: LEADING_SLOT_H,
    justifyContent: "center",
    marginLeft: LABEL_GAP,
    marginRight: LABEL_GAP,
    minWidth: 0,
    overflow: "hidden",
  },
  sessionTitle: {
    color: palette.text,
    fontSize: 14,
    lineHeight: 20,
    width: "100%",
  },
  time: {
    color: palette.subtle,
    flexShrink: 0,
    fontSize: 10,
    lineHeight: 16,
    marginRight: -1,
  },
  rowAction: {
    alignItems: "center",
    borderRadius: 4,
    flexShrink: 0,
    height: ACTION_BUTTON,
    justifyContent: "center",
    width: ACTION_BUTTON,
  },
  actionPressed: {
    backgroundColor: palette.overlay,
  },
  empty: {
    color: palette.subtle,
    fontSize: 13,
    lineHeight: 20,
    paddingHorizontal: space.md,
    paddingVertical: space.lg,
  },
  overflow: {
    borderRadius: ROW_RADIUS,
    height: OVERFLOW_HEIGHT,
    justifyContent: "center",
    paddingLeft: OVERFLOW_INDENT,
  },
  overflowLabel: {
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 16,
  },
  dropMarker: {
    height: 12,
    left: space.sm + PAINT_INSET,
    position: "absolute",
    right: space.sm + PAINT_INSET,
    zIndex: 50,
  },
  dropLine: {
    backgroundColor: palette.accent,
    height: 2,
    left: 0,
    position: "absolute",
    right: 0,
    top: 5,
  },
  dropArrow: {
    left: 2,
    position: "absolute",
    top: -1,
  },
  hoverCard: {
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: ROW_RADIUS,
    borderWidth: 1,
    gap: space.sm,
    left: HOVER_CARD_LEFT,
    paddingHorizontal: space.lg,
    paddingVertical: space.md,
    position: "absolute",
    width: HOVER_CARD_WIDTH,
    zIndex: 40,
  },
  hoverTitle: {
    color: palette.text,
    fontSize: 14,
    lineHeight: 20,
  },
  hoverBody: {
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 16,
  },
  hoverMeta: {
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 16,
  },
  hoverStatus: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.sm,
  },
  hoverStatusLabel: {
    color: palette.subtle,
    fontSize: 12,
    lineHeight: 20,
  },
  menuCard: {
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: ROW_RADIUS,
    borderWidth: 1,
    padding: MENU_PADDING,
    position: "absolute",
  },
  menuRow: {
    alignItems: "center",
    borderRadius: 6,
    flexDirection: "row",
    minHeight: MENU_ROW_HEIGHT,
    paddingHorizontal: space.sm,
  },
  menuRowHover: {
    backgroundColor: palette.overlay,
  },
  menuLabel: {
    color: palette.text,
    fontSize: 12,
    lineHeight: 17,
  },
  menuLabelHover: {
    color: palette.text,
  },
  viewRow: {
    alignItems: "center",
    borderRadius: 6,
    flexDirection: "row",
    gap: space.sm,
    minHeight: VIEW_ROW_MIN_HEIGHT,
    paddingHorizontal: space.sm,
    paddingVertical: 4,
  },
  viewRowText: {
    flex: 1,
    minWidth: 0,
  },
  viewLabel: {
    color: palette.text,
    fontSize: 13,
    lineHeight: 18,
  },
  viewLabelSelected: {
    color: palette.accent,
  },
  viewDescription: {
    color: palette.subtle,
    fontSize: 13,
    lineHeight: 18,
  },
  footer: {
    borderTopColor: palette.border,
    borderTopWidth: StyleSheet.hairlineWidth,
    gap: space.sm,
    marginTop: space.md,
    paddingHorizontal: space.sm,
    paddingTop: space.md,
  },
  footerLabel: {
    ...typography.eyebrow,
    color: palette.muted,
  },
  server: {
    ...typography.mono,
    color: palette.subtle,
    fontSize: 10,
  },
});
