import { Ionicons } from "@expo/vector-icons";
import { useRouter } from "expo-router";
import type { ElementRef } from "react";
import { useEffect, useRef, useState } from "react";
import {
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
  useWindowDimensions,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { MenuButton } from "../../components/menu-button";
import { MenuDivider, MenuRow, Popover } from "../../components/popover";
import { Screen } from "../../components/screen";
import { tapFeedback } from "../../lib/haptics";
import { useOpenDrawer } from "../../lib/navigation";
import {
  loadPreferences,
  saveDefaultMode,
  saveInstructions,
} from "../../lib/preferences";
import { palette, radius, space, typography } from "../../lib/theme";
import type { SessionMode } from "../../lib/types";

const tabs = [
  { id: "discover", label: "Discover" },
  { id: "connectors", label: "Connectors" },
  { id: "skills", label: "Skills" },
] as const;

type Tab = (typeof tabs)[number]["id"];

interface SkillItem {
  id: string;
  name: string;
  summary: string;
  installed: boolean;
  active: boolean;
}

interface ConnectorItem {
  id: string;
  name: string;
  summary: string;
  active: boolean;
}

interface EditorState {
  kind: "skill" | "connector";
  id?: string;
  name: string;
  summary: string;
}

interface MenuState {
  kind: "skill" | "connector";
  id: string;
  top: number;
}

const initialSkills: SkillItem[] = [
  {
    active: false,
    id: "focus-my-energy",
    installed: true,
    name: "focus-my-energy",
    summary: "Use when the user asks what to work on next.",
  },
  {
    active: false,
    id: "writing-style",
    installed: false,
    name: "Match your writing style",
    summary: "Learns your voice from your real conversations.",
  },
  {
    active: false,
    id: "perspectives",
    installed: false,
    name: "Get more perspectives",
    summary: "Get 3-5 distinct viewpoints before deciding.",
  },
  {
    active: false,
    id: "fresh-ideas",
    installed: false,
    name: "Generate fresh ideas",
    summary: "Turn existing content into 5 entirely new angles.",
  },
  {
    active: false,
    id: "prep-meetings",
    installed: false,
    name: "Prep for meetings",
    summary: "Prep for any meeting with a brief and open questions.",
  },
];

const initialConnectors: ConnectorItem[] = [
  {
    active: true,
    id: "github",
    name: "GitHub",
    summary: "Read repositories, diffs and check runs.",
  },
  {
    active: true,
    id: "filesystem",
    name: "Filesystem",
    summary: "Read and write files inside the workspace.",
  },
  {
    active: true,
    id: "sandbox",
    name: "Sandbox",
    summary: "Run commands in the worker container.",
  },
  {
    active: true,
    id: "sentry",
    name: "Sentry",
    summary: "Triage errors and releases from your projects.",
  },
  {
    active: false,
    id: "linear",
    name: "Linear",
    summary: "Read issues and move them through the cycle.",
  },
  {
    active: false,
    id: "notion",
    name: "Notion",
    summary: "Search pages and keep docs in sync.",
  },
  {
    active: false,
    id: "slack",
    name: "Slack",
    summary: "Send updates and read the channels you pick.",
  },
  {
    active: false,
    id: "vercel",
    name: "Vercel",
    summary: "Watch deployments and read build logs.",
  },
];

const modes: { mode: SessionMode; label: string }[] = [
  { mode: "agent", label: "Agent" },
  { mode: "terminal", label: "Terminal" },
];

export default function CustomizeScreen() {
  const router = useRouter();
  const insets = useSafeAreaInsets();
  const { height } = useWindowDimensions();
  const openDrawer = useOpenDrawer();

  const [tab, setTab] = useState<Tab>("skills");
  const [skills, setSkills] = useState<SkillItem[]>(initialSkills);
  const [connectors, setConnectors] =
    useState<ConnectorItem[]>(initialConnectors);
  const [inactiveOpen, setInactiveOpen] = useState(true);
  const [query, setQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [instructions, setInstructions] = useState("");
  const [mode, setMode] = useState<SessionMode>("agent");

  useEffect(() => {
    void loadPreferences().then((preferences) => {
      setInstructions(preferences.instructions);
      setMode(preferences.mode);
    });
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => {
      void saveInstructions(instructions);
    }, 400);
    return () => clearTimeout(timer);
  }, [instructions]);

  function pickMode(next: SessionMode) {
    tapFeedback();
    setMode(next);
    void saveDefaultMode(next);
  }

  function pickTab(next: Tab) {
    tapFeedback();
    setTab(next);
    setQuery("");
    setEditor(null);
  }

  function toggleSearch() {
    tapFeedback();
    if (searchOpen) setQuery("");
    setSearchOpen(!searchOpen);
  }

  const search = query.trim().toLowerCase();
  const matches = (name: string, summary: string) =>
    !search || `${name} ${summary}`.toLowerCase().includes(search);

  const shownSkills = skills.filter((skill) =>
    matches(skill.name, skill.summary),
  );
  const activeSkills = shownSkills.filter(
    (skill) => skill.installed && skill.active,
  );
  const inactiveSkills = shownSkills.filter(
    (skill) => skill.installed && !skill.active,
  );
  const recommendedSkills = shownSkills.filter((skill) => !skill.installed);

  const shownConnectors = connectors.filter((connector) =>
    matches(connector.name, connector.summary),
  );
  const activeConnectors = shownConnectors.filter(
    (connector) => connector.active,
  );
  const inactiveConnectors = shownConnectors.filter(
    (connector) => !connector.active,
  );

  const menuSkill =
    menu?.kind === "skill"
      ? skills.find((skill) => skill.id === menu.id)
      : undefined;
  const menuConnector =
    menu?.kind === "connector"
      ? connectors.find((connector) => connector.id === menu.id)
      : undefined;

  function installSkill(id: string) {
    tapFeedback();
    setSkills((current) =>
      current.map((skill) =>
        skill.id === id ? { ...skill, installed: true } : skill,
      ),
    );
  }

  function useSkill(id: string) {
    tapFeedback();
    setMenu(null);
    setSkills((current) =>
      current.map((skill) =>
        skill.id === id
          ? { ...skill, active: true, installed: true }
          : skill,
      ),
    );
  }

  function turnOffSkill(id: string) {
    tapFeedback();
    setMenu(null);
    setSkills((current) =>
      current.map((skill) =>
        skill.id === id ? { ...skill, active: false } : skill,
      ),
    );
  }

  function deleteSkill(id: string) {
    tapFeedback();
    setMenu(null);
    setSkills((current) => current.filter((skill) => skill.id !== id));
  }

  function editWithGemini(skill: SkillItem) {
    tapFeedback();
    setMenu(null);
    const prompt = `Help me rewrite the "${skill.name}" skill. Current instruction: ${skill.summary}`;
    router.push(
      `/session/skill-${skill.id}-${Date.now()}?prompt=${encodeURIComponent(
        prompt,
      )}&mode=agent`,
    );
  }

  function enableConnector(id: string) {
    tapFeedback();
    setConnectors((current) =>
      current.map((connector) =>
        connector.id === id ? { ...connector, active: true } : connector,
      ),
    );
  }

  function disableConnector(id: string) {
    tapFeedback();
    setMenu(null);
    setConnectors((current) =>
      current.map((connector) =>
        connector.id === id ? { ...connector, active: false } : connector,
      ),
    );
  }

  function startEditor(kind: "skill" | "connector", id?: string) {
    tapFeedback();
    setMenu(null);
    setCreateOpen(false);
    if (kind === "skill") {
      const skill = skills.find((entry) => entry.id === id);
      setEditor({
        id,
        kind,
        name: skill?.name ?? "",
        summary: skill?.summary ?? "",
      });
      setTab("skills");
    } else {
      const connector = connectors.find((entry) => entry.id === id);
      setEditor({
        id,
        kind,
        name: connector?.name ?? "",
        summary: connector?.summary ?? "",
      });
      setTab("connectors");
    }
  }

  function saveEditor() {
    if (!editor) return;
    const name = editor.name.trim();
    if (!name) return;
    tapFeedback();
    const summary =
      editor.summary.trim() ||
      (editor.kind === "skill"
        ? "Custom instruction for Goble."
        : "Custom connector for this workspace.");
    if (editor.kind === "skill") {
      setSkills((current) =>
        editor.id
          ? current.map((skill) =>
              skill.id === editor.id ? { ...skill, name, summary } : skill,
            )
          : [
              ...current,
              {
                active: false,
                id: `skill-${Date.now()}`,
                installed: false,
                name,
                summary,
              },
            ],
      );
    } else {
      setConnectors((current) =>
        editor.id
          ? current.map((connector) =>
              connector.id === editor.id
                ? { ...connector, name, summary }
                : connector,
            )
          : [
              ...current,
              {
                active: false,
                id: `connector-${Date.now()}`,
                name,
                summary,
              },
            ],
      );
    }
    setEditor(null);
  }

  return (
    <Screen>
      <View style={styles.header}>
        <MenuButton onPress={openDrawer} />
        <View style={styles.headerPill}>
          <Pressable
            accessibilityLabel={
              searchOpen
                ? "Close search"
                : "Search skills and connectors"
            }
            accessibilityRole="button"
            onPress={toggleSearch}
            style={({ pressed }) => [
              styles.headerButton,
              pressed && styles.pressed,
            ]}
          >
            <Ionicons
              color={palette.text}
              name={searchOpen ? "close" : "search"}
              size={20}
            />
          </Pressable>
          <Pressable
            accessibilityLabel="Create skill or connector"
            accessibilityRole="button"
            onPress={() => {
              tapFeedback();
              setCreateOpen(true);
            }}
            style={({ pressed }) => [
              styles.headerButton,
              pressed && styles.pressed,
            ]}
          >
            <Ionicons color={palette.text} name="add" size={23} />
          </Pressable>
        </View>
      </View>

      <View style={styles.tabs}>
        {tabs.map((entry) => {
          const selected = entry.id === tab;
          return (
            <Pressable
              accessibilityRole="button"
              accessibilityState={{ selected }}
              key={entry.id}
              onPress={() => pickTab(entry.id)}
              style={({ pressed }) => [
                styles.tab,
                selected && styles.tabActive,
                pressed && styles.pressed,
              ]}
            >
              <Text
                style={[styles.tabLabel, selected && styles.tabLabelActive]}
              >
                {entry.label}
              </Text>
            </Pressable>
          );
        })}
      </View>

      {searchOpen ? (
        <View style={styles.search}>
          <Ionicons color={palette.subtle} name="search" size={17} />
          <TextInput
            accessibilityLabel="Search skills and connectors"
            autoFocus
            onChangeText={setQuery}
            placeholder="Search skills and connectors"
            placeholderTextColor={palette.subtle}
            selectionColor={palette.accent}
            style={styles.searchInput}
            value={query}
          />
        </View>
      ) : null}

      {editor ? (
        <View style={styles.editor}>
          <Text style={styles.editorLabel}>
            {(editor.id ? "EDIT " : "NEW ") + editor.kind.toUpperCase()}
          </Text>
          <TextInput
            accessibilityLabel={
              editor.kind === "skill" ? "Skill name" : "Connector name"
            }
            onChangeText={(name) =>
              setEditor((current) => (current ? { ...current, name } : current))
            }
            placeholder={
              editor.kind === "skill" ? "Skill name" : "Connector name"
            }
            placeholderTextColor={palette.subtle}
            selectionColor={palette.accent}
            style={styles.editorInput}
            value={editor.name}
          />
          <TextInput
            accessibilityLabel="Description"
            multiline
            onChangeText={(summary) =>
              setEditor((current) =>
                current ? { ...current, summary } : current,
              )
            }
            placeholder="What should this do?"
            placeholderTextColor={palette.subtle}
            selectionColor={palette.accent}
            style={[styles.editorInput, styles.editorBody]}
            textAlignVertical="top"
            value={editor.summary}
          />
          <View style={styles.editorActions}>
            <Pressable
              accessibilityLabel="Cancel editing"
              accessibilityRole="button"
              onPress={() => {
                tapFeedback();
                setEditor(null);
              }}
              style={({ pressed }) => [
                styles.secondaryButton,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.secondaryLabel}>Cancel</Text>
            </Pressable>
            <Pressable
              accessibilityLabel="Save"
              accessibilityRole="button"
              accessibilityState={{ disabled: !editor.name.trim() }}
              disabled={!editor.name.trim()}
              onPress={saveEditor}
              style={({ pressed }) => [
                styles.primaryButton,
                !editor.name.trim() && styles.disabled,
                pressed && styles.pressed,
              ]}
            >
              <Text style={styles.primaryLabel}>Save</Text>
            </Pressable>
          </View>
        </View>
      ) : null}

      {tab === "discover" ? (
        <View>
          <Text style={styles.pageTitle}>Discover</Text>
          <Text style={styles.pageBody}>
            Make Goble work the way you do. Instructions travel with every new
            session.
          </Text>

          <Text style={styles.groupLabel}>INSTRUCTIONS</Text>
          <View style={styles.card}>
            <TextInput
              accessibilityLabel="Custom instructions"
              multiline
              onChangeText={setInstructions}
              placeholder="Tell Goble how you like to work: tone, stack, review habits."
              placeholderTextColor={palette.subtle}
              selectionColor={palette.accent}
              style={styles.input}
              textAlignVertical="top"
              value={instructions}
            />
          </View>
          <Text style={styles.hint}>
            Saved on this device and sent with every new session.
          </Text>

          <Text style={styles.groupLabel}>DEFAULT MODE</Text>
          <View style={styles.group}>
            <View style={styles.modeRow}>
              <View style={styles.rowText}>
                <Text style={styles.rowLabel}>New sessions start in</Text>
                <Text style={styles.rowDetail}>
                  The composer still switches from the plus button.
                </Text>
              </View>
              <View style={styles.segments}>
                {modes.map((option) => {
                  const active = option.mode === mode;
                  return (
                    <Pressable
                      accessibilityRole="button"
                      accessibilityState={{ selected: active }}
                      key={option.mode}
                      onPress={() => pickMode(option.mode)}
                      style={({ pressed }) => [
                        styles.segment,
                        active && styles.segmentActive,
                        pressed && styles.pressed,
                      ]}
                    >
                      <Text
                        style={[
                          styles.segmentLabel,
                          active && styles.segmentLabelActive,
                        ]}
                      >
                        {option.label}
                      </Text>
                    </Pressable>
                  );
                })}
              </View>
            </View>
          </View>

          <Text style={styles.groupLabel}>APPEARANCE</Text>
          <View style={styles.group}>
            <View style={styles.modeRow}>
              <View style={styles.rowText}>
                <Text style={styles.rowLabel}>Theme</Text>
                <Text style={styles.rowDetail}>
                  The desktop and the phone share one dark palette.
                </Text>
              </View>
              <Text style={styles.rowValue}>Dark</Text>
            </View>
          </View>
        </View>
      ) : null}

      {tab === "skills" ? (
        <View>
          <Text style={styles.pageTitle}>Skills</Text>
          <Text style={styles.pageBody}>
            Tell Goble how you like things done.{" "}
            <Text style={styles.learnMore}>Learn more</Text>
          </Text>

          {activeSkills.length > 0 ? (
            <View>
              <Text style={styles.groupLabel}>
                Active ({activeSkills.length})
              </Text>
              {activeSkills.map((skill) => (
                <SkillRow
                  key={skill.id}
                  onInstall={() => installSkill(skill.id)}
                  onMore={(top) =>
                    setMenu({ id: skill.id, kind: "skill", top })
                  }
                  skill={skill}
                />
              ))}
            </View>
          ) : null}

          {inactiveSkills.length > 0 ? (
            <View>
              <Text style={styles.groupLabel}>
                Inactive ({inactiveSkills.length})
              </Text>
              {inactiveSkills.map((skill) => (
                <SkillRow
                  key={skill.id}
                  onInstall={() => installSkill(skill.id)}
                  onMore={(top) =>
                    setMenu({ id: skill.id, kind: "skill", top })
                  }
                  skill={skill}
                />
              ))}
            </View>
          ) : null}

          {recommendedSkills.length > 0 ? (
            <View>
              <Text style={styles.groupLabel}>
                Recommended ({recommendedSkills.length})
              </Text>
              {recommendedSkills.map((skill) => (
                <SkillRow
                  key={skill.id}
                  onInstall={() => installSkill(skill.id)}
                  onMore={(top) =>
                    setMenu({ id: skill.id, kind: "skill", top })
                  }
                  skill={skill}
                />
              ))}
            </View>
          ) : null}

          {shownSkills.length === 0 ? (
            <Text style={styles.empty}>No skills match “{query.trim()}”.</Text>
          ) : null}
        </View>
      ) : null}

      {tab === "connectors" ? (
        <View>
          <Text style={styles.pageTitle}>Connected Apps</Text>
          <Text style={styles.pageBody}>
            Connect your tools to get more done.
          </Text>

          {activeConnectors.length > 0 ? (
            <View>
              <Text style={styles.groupLabel}>
                Active ({activeConnectors.length})
              </Text>
              {activeConnectors.map((connector) => (
                <ConnectorRow
                  connector={connector}
                  key={connector.id}
                  onMore={(top) =>
                    setMenu({ id: connector.id, kind: "connector", top })
                  }
                  onPlus={() => enableConnector(connector.id)}
                />
              ))}
            </View>
          ) : null}

          {inactiveConnectors.length > 0 ? (
            <View>
              <Pressable
                accessibilityLabel={
                  inactiveOpen
                    ? "Collapse inactive connectors"
                    : "Expand inactive connectors"
                }
                accessibilityRole="button"
                onPress={() => {
                  tapFeedback();
                  setInactiveOpen(!inactiveOpen);
                }}
                style={({ pressed }) => [
                  styles.inactiveHeader,
                  pressed && styles.pressed,
                ]}
              >
                <Text style={styles.groupLabel}>
                  Inactive ({inactiveConnectors.length})
                </Text>
                <Ionicons
                  color={palette.muted}
                  name={inactiveOpen ? "chevron-down" : "chevron-forward"}
                  size={16}
                />
              </Pressable>
              {inactiveOpen
                ? inactiveConnectors.map((connector) => (
                    <ConnectorRow
                      connector={connector}
                      key={connector.id}
                      onMore={(top) =>
                        setMenu({ id: connector.id, kind: "connector", top })
                      }
                      onPlus={() => enableConnector(connector.id)}
                    />
                  ))
                : null}
            </View>
          ) : null}

          {shownConnectors.length === 0 ? (
            <Text style={styles.empty}>
              No connectors match “{query.trim()}”.
            </Text>
          ) : null}
        </View>
      ) : null}

      <Popover
        onClose={() => setCreateOpen(false)}
        style={{ right: 16, top: insets.top + 60 }}
        visible={createOpen}
        width={224}
      >
        <MenuRow
          icon="sparkles-outline"
          label="New skill"
          onPress={() => startEditor("skill")}
        />
        <MenuRow
          icon="link-outline"
          label="New connector"
          onPress={() => startEditor("connector")}
        />
      </Popover>

      <Popover
        onClose={() => setMenu(null)}
        style={{ right: 12, top: Math.min(menu?.top ?? 120, height - 300) }}
        visible={menu !== null}
        width={248}
      >
        {menuSkill ? (
          <View>
            <MenuRow
              icon={menuSkill.active ? "pause-outline" : "play-outline"}
              label={menuSkill.active ? "Turn off" : "Use now"}
              onPress={() =>
                menuSkill.active
                  ? turnOffSkill(menuSkill.id)
                  : useSkill(menuSkill.id)
              }
            />
            <MenuRow
              icon="sparkles-outline"
              label="Edit with Gemini"
              onPress={() => editWithGemini(menuSkill)}
            />
            <MenuRow
              icon="create-outline"
              label="Edit manually"
              onPress={() => startEditor("skill", menuSkill.id)}
            />
            <MenuDivider />
            <MenuRow
              destructive
              icon="trash-outline"
              label="Delete"
              onPress={() => deleteSkill(menuSkill.id)}
            />
          </View>
        ) : null}
        {menuConnector ? (
          <View>
            <MenuRow
              icon="settings-outline"
              label="Configure"
              onPress={() => startEditor("connector", menuConnector.id)}
            />
            <MenuRow
              destructive
              icon="power-outline"
              label="Disable"
              onPress={() => disableConnector(menuConnector.id)}
            />
          </View>
        ) : null}
      </Popover>
    </Screen>
  );
}

function SkillRow({
  skill,
  onMore,
  onInstall,
}: {
  skill: SkillItem;
  onMore: (top: number) => void;
  onInstall: () => void;
}) {
  return (
    <View style={styles.item}>
      <View style={styles.itemText}>
        <Text numberOfLines={1} style={styles.itemTitle}>
          {skill.name}
        </Text>
        <Text numberOfLines={1} style={styles.itemBody}>
          {skill.summary}
        </Text>
      </View>
      {skill.installed ? (
        <MoreButton label={`Actions for ${skill.name}`} onOpen={onMore} />
      ) : (
        <Pressable
          accessibilityLabel={`Install ${skill.name}`}
          accessibilityRole="button"
          hitSlop={8}
          onPress={onInstall}
          style={({ pressed }) => [
            styles.addButton,
            pressed && styles.pressed,
          ]}
        >
          <Ionicons color={palette.text} name="add" size={22} />
        </Pressable>
      )}
    </View>
  );
}

function ConnectorRow({
  connector,
  onMore,
  onPlus,
}: {
  connector: ConnectorItem;
  onMore: (top: number) => void;
  onPlus: () => void;
}) {
  return (
    <View style={styles.item}>
      <View style={styles.itemText}>
        <Text numberOfLines={1} style={styles.itemTitle}>
          {connector.name}
        </Text>
        <Text numberOfLines={1} style={styles.itemBody}>
          {connector.summary}
        </Text>
      </View>
      {connector.active ? (
        <MoreButton
          label={`Actions for ${connector.name}`}
          onOpen={onMore}
        />
      ) : (
        <Pressable
          accessibilityLabel={`Connect ${connector.name}`}
          accessibilityRole="button"
          hitSlop={8}
          onPress={onPlus}
          style={({ pressed }) => [
            styles.addButton,
            pressed && styles.pressed,
          ]}
        >
          <Ionicons color={palette.text} name="add" size={22} />
        </Pressable>
      )}
    </View>
  );
}

function MoreButton({
  label,
  onOpen,
}: {
  label: string;
  onOpen: (top: number) => void;
}) {
  const ref = useRef<ElementRef<typeof View> | null>(null);
  return (
    <Pressable
      accessibilityLabel={label}
      accessibilityRole="button"
      hitSlop={8}
      onPress={() => {
        tapFeedback();
        ref.current?.measureInWindow((_x, y, _width, heightValue) =>
          onOpen(y + heightValue + 4),
        );
      }}
      ref={ref}
      style={({ pressed }) => [styles.moreButton, pressed && styles.pressed]}
    >
      <Ionicons
        color={palette.muted}
        name="ellipsis-horizontal"
        size={20}
      />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  header: {
    alignItems: "center",
    flexDirection: "row",
    justifyContent: "space-between",
    paddingBottom: space.md,
    paddingTop: space.sm,
  },
  headerPill: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    padding: 3,
  },
  headerButton: {
    alignItems: "center",
    height: 38,
    justifyContent: "center",
    width: 42,
  },
  tabs: {
    flexDirection: "row",
    gap: 6,
    justifyContent: "center",
    marginBottom: space.lg,
  },
  tab: {
    borderRadius: radius.pill,
    paddingHorizontal: 18,
    paddingVertical: 9,
  },
  tabActive: {
    backgroundColor: palette.overlay,
  },
  tabLabel: {
    ...typography.callout,
    color: palette.muted,
    fontSize: 15,
  },
  tabLabelActive: {
    color: palette.text,
    fontWeight: "500",
  },
  search: {
    alignItems: "center",
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: "row",
    gap: space.sm,
    marginBottom: space.lg,
    paddingHorizontal: space.md,
  },
  searchInput: {
    ...typography.body,
    color: palette.text,
    flex: 1,
    fontSize: 14,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingVertical: 11,
  },
  pageTitle: {
    color: palette.text,
    fontSize: 30,
    fontWeight: "500",
    letterSpacing: -0.7,
    lineHeight: 36,
  },
  pageBody: {
    ...typography.callout,
    color: palette.muted,
    fontSize: 15,
    lineHeight: 21,
    marginTop: 4,
  },
  learnMore: {
    color: palette.text,
    textDecorationLine: "underline",
  },
  groupLabel: {
    color: palette.muted,
    fontSize: 14,
    lineHeight: 20,
    marginBottom: space.sm,
    marginTop: space.xl,
  },
  item: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    minHeight: 60,
    paddingVertical: space.sm,
  },
  itemText: {
    flex: 1,
    gap: 2,
  },
  itemTitle: {
    ...typography.callout,
    color: palette.text,
    fontSize: 16,
    fontWeight: "400",
  },
  itemBody: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 13,
    lineHeight: 18,
  },
  moreButton: {
    alignItems: "center",
    height: 44,
    justifyContent: "center",
    width: 40,
  },
  addButton: {
    alignItems: "center",
    height: 40,
    justifyContent: "center",
    width: 40,
  },
  inactiveHeader: {
    alignItems: "center",
    alignSelf: "flex-start",
    flexDirection: "row",
    gap: space.xs,
  },
  empty: {
    ...typography.caption,
    color: palette.muted,
    marginTop: space.xl,
  },
  editor: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    gap: space.sm,
    marginBottom: space.lg,
    padding: space.md,
  },
  editorLabel: {
    ...typography.eyebrow,
    color: palette.muted,
    fontSize: 9,
  },
  editorInput: {
    ...typography.body,
    backgroundColor: palette.raised,
    borderColor: palette.border,
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    color: palette.text,
    fontSize: 14,
    outlineColor: "transparent",
    outlineStyle: "solid",
    paddingHorizontal: 10,
    paddingVertical: 9,
  },
  editorBody: {
    minHeight: 64,
  },
  editorActions: {
    flexDirection: "row",
    gap: space.sm,
    justifyContent: "flex-end",
    marginTop: 2,
  },
  secondaryButton: {
    borderColor: palette.borderStrong,
    borderRadius: radius.pill,
    borderWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 16,
    paddingVertical: 8,
  },
  secondaryLabel: {
    ...typography.callout,
    color: palette.text,
    fontSize: 13,
  },
  primaryButton: {
    backgroundColor: palette.primary,
    borderRadius: radius.pill,
    paddingHorizontal: 18,
    paddingVertical: 8,
  },
  primaryLabel: {
    ...typography.callout,
    color: palette.black,
    fontSize: 13,
    fontWeight: "600",
  },
  disabled: {
    opacity: 0.4,
  },
  card: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    padding: space.md,
  },
  input: {
    ...typography.body,
    color: palette.text,
    fontSize: 14,
    lineHeight: 21,
    minHeight: 112,
    outlineColor: "transparent",
    outlineStyle: "solid",
    padding: 0,
  },
  hint: {
    ...typography.caption,
    color: palette.subtle,
    marginTop: space.sm,
    paddingLeft: space.xs,
  },
  group: {
    backgroundColor: palette.surface,
    borderColor: palette.border,
    borderRadius: radius.lg,
    borderWidth: StyleSheet.hairlineWidth,
    overflow: "hidden",
  },
  modeRow: {
    alignItems: "center",
    flexDirection: "row",
    gap: space.md,
    minHeight: 58,
    paddingHorizontal: space.md,
    paddingVertical: space.sm,
  },
  rowText: {
    flex: 1,
    gap: 2,
    minWidth: 0,
  },
  rowLabel: {
    ...typography.callout,
    color: palette.text,
    fontSize: 14,
  },
  rowDetail: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
    lineHeight: 17,
  },
  rowValue: {
    ...typography.mono,
    color: palette.muted,
    fontSize: 11,
  },
  segments: {
    backgroundColor: palette.bg,
    borderRadius: radius.md,
    flexDirection: "row",
    padding: 3,
  },
  segment: {
    alignItems: "center",
    borderRadius: radius.sm,
    justifyContent: "center",
    paddingHorizontal: 12,
    paddingVertical: 7,
  },
  segmentActive: {
    backgroundColor: palette.accentSoft,
  },
  segmentLabel: {
    ...typography.caption,
    color: palette.muted,
    fontSize: 12,
  },
  segmentLabelActive: {
    color: palette.text,
  },
  pressed: {
    opacity: 0.7,
  },
});
