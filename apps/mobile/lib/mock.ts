import type {
  Agent,
  ChatMessage,
  Conversation,
  ConversationDetail,
  HomeSnapshot,
  Model,
  ScheduledTask,
} from "./types";

function minutesAgo(minutes: number): string {
  return new Date(Date.now() - minutes * 60_000).toISOString();
}

function minutesFromNow(minutes: number): string {
  return new Date(Date.now() + minutes * 60_000).toISOString();
}

const models: Model[] = [
  {
    id: "gpt-4o",
    name: "GPT-4o",
    vendor: "OpenAI",
    detail: "Default · vision and tools",
  },
  {
    id: "gpt-4o-mini",
    name: "GPT-4o mini",
    vendor: "OpenAI",
    detail: "Fast and cheap",
  },
  {
    id: "gpt-4.1",
    name: "GPT-4.1",
    vendor: "OpenAI",
    detail: "Long context",
  },
  {
    id: "claude-3-5",
    name: "Claude 3.5",
    vendor: "Anthropic",
    detail: "Balanced reasoning",
  },
];

const agents: Agent[] = [
  {
    id: "internal",
    name: "Internal harness",
    model: "GPT-4o",
    tools: "read/write",
    summary: "Plans the change, edits files and runs tools in the project.",
    status: "ready",
  },
  {
    id: "reviewer",
    name: "Reviewer",
    model: "Claude 3.5",
    tools: "read-only",
    summary: "Reads diffs, checks risks and writes a report.",
    status: "ready",
  },
  {
    id: "sandbox",
    name: "Sandbox runner",
    model: "GPT-4o mini",
    tools: "sandbox",
    summary: "Runs commands in an isolated container.",
    status: "paused",
  },
];

const schedules: ScheduledTask[] = [
  {
    id: "nightly",
    name: "Nightly regression",
    schedule: "0 2 * * *",
    detail: "Runs the workspace checks on terminal-first.",
    nextRun: minutesFromNow(6 * 60),
    lastRun: minutesAgo(18 * 60),
    status: "active",
  },
  {
    id: "inbox",
    name: "Inbox triage",
    schedule: "Every 30 min",
    detail: "Summarizes new approvals and unread sessions.",
    nextRun: minutesFromNow(12),
    lastRun: minutesAgo(18),
    status: "active",
  },
  {
    id: "release",
    name: "Release notarize",
    schedule: "0 9 * * 1-5",
    detail: "Builds and notarizes the macOS app.",
    nextRun: minutesFromNow(15 * 60),
    status: "paused",
  },
];

const conversations: Conversation[] = [
  {
    id: "gateway-access",
    title: "Gateway access control",
    preview: "Add device pairing and per-sandbox tokens…",
    projectName: "goble",
    mode: "agent",
    status: "live",
    starred: true,
    updatedAt: minutesAgo(4),
  },
  {
    id: "mobile-shell",
    title: "Mobile shell",
    preview: "The empty session, sidebar and composer are in…",
    projectName: "goble",
    mode: "agent",
    status: "waiting",
    starred: true,
    updatedAt: minutesAgo(28),
  },
  {
    id: "pty-resize",
    title: "PTY resize on reconnect",
    preview: "The worker replays the last window size after…",
    projectName: "goble",
    mode: "terminal",
    status: "done",
    starred: false,
    updatedAt: minutesAgo(61),
  },
  {
    id: "landing-hero",
    title: "Landing hero",
    preview: "Shorter headline and one call to action…",
    projectName: "octomus",
    mode: "agent",
    status: "done",
    starred: false,
    updatedAt: minutesAgo(180),
  },
  {
    id: "notarize",
    title: "Notarization output",
    preview: "The release job could not find the signing identity…",
    projectName: "rakazo",
    mode: "terminal",
    status: "done",
    starred: false,
    updatedAt: minutesAgo(232),
  },
];

const messages: Record<string, ChatMessage[]> = {
  "gateway-access": [
    {
      id: "m1",
      role: "user",
      text: "Wire the gateway access control. Start with device pairing and per-sandbox tokens.",
      at: minutesAgo(30),
    },
    {
      id: "m2",
      role: "assistant",
      text: "I mapped the current gateway routes and the desktop pairing flow. The first cut will add a device code exchange, a token table, and a per-sandbox bearer token in the worker environment.",
      at: minutesAgo(28),
    },
    {
      id: "m3",
      role: "tool",
      text: "cargo check -p goble-gateway",
      at: minutesAgo(24),
      output: "Finished `dev` profile [unoptimized] target(s) in 12.4s",
      status: "done",
    },
    {
      id: "m4",
      role: "assistant",
      text: "The gateway compiles. **The next step is the migration** that creates users, devices and sandboxes. {color:#f59e0b}It is paused for your approval.{/color}",
      at: minutesAgo(4),
    },
  ],
  "mobile-shell": [
    {
      id: "m1",
      role: "user",
      text: "Build the mobile shell with an empty session and a sidebar.",
      at: minutesAgo(50),
    },
    {
      id: "m2",
      role: "assistant",
      text: "The Expo shell is in place. Home opens on an empty session, the drawer holds conversations, and the plus button switches between agent and terminal.",
      at: minutesAgo(28),
    },
  ],
  "pty-resize": [
    {
      id: "m1",
      role: "user",
      text: "Fix PTY resize after a reconnect.",
      at: minutesAgo(74),
    },
    {
      id: "m2",
      role: "terminal",
      text: "cargo test -p goblin-worker resize",
      at: minutesAgo(63),
      command: "cargo test -p goblin-worker resize",
      output:
        "running 8 tests\ntest pty::tests::resize_after_reconnect ... ok\ntest result: ok. 8 passed",
      cwd: "workspace/goble",
      branch: "terminal-first",
      duration: "1.2s",
      status: "done",
    },
    {
      id: "m3",
      role: "assistant",
      text: "The worker keeps the pane alive, replays the latest size, and only then resumes output. The reconnect test is green.",
      at: minutesAgo(61),
    },
  ],
  "landing-hero": [
    {
      id: "m1",
      role: "user",
      text: "Rewrite the landing hero. Make it shorter.",
      at: minutesAgo(190),
    },
    {
      id: "m2",
      role: "assistant",
      text: "The new hero is three lines: the promise, one proof point, and one call to action. The terminal preview moved above the fold.",
      at: minutesAgo(180),
    },
  ],
  notarize: [
    {
      id: "m1",
      role: "terminal",
      text: "pnpm release:mac",
      at: minutesAgo(240),
      command: "pnpm release:mac",
      output: "codesign: no identity found\nexit code 1",
      cwd: "workspace/rakazo",
      branch: "main",
      duration: "4.8s",
      status: "error",
    },
    {
      id: "m2",
      role: "assistant",
      text: "The signing identity is missing from the release job. I stopped before touching the build.",
      at: minutesAgo(232),
    },
  ],
};

export function demoModels(): Model[] {
  return models;
}

export function demoAgents(): Agent[] {
  return agents;
}

export function demoSchedules(): ScheduledTask[] {
  return schedules;
}

interface ConversationPatch {
  title?: string;
  preview?: string;
  starred?: boolean;
  deleted?: boolean;
}

const conversationPatches = new Map<string, ConversationPatch>();
const conversationListeners = new Set<() => void>();
/** Sessions started on this phone, newest first. */
const extraConversations: Conversation[] = [];

function allConversations(): Conversation[] {
  return [...extraConversations, ...conversations];
}

function patched(conversation: Conversation): Conversation {
  const patch = conversationPatches.get(conversation.id);
  if (!patch) return conversation;
  return {
    ...conversation,
    title: patch.title ?? conversation.title,
    preview: patch.preview ?? conversation.preview,
    starred: patch.starred ?? conversation.starred,
  };
}

function notifyConversations() {
  for (const listener of conversationListeners) listener();
}

export function subscribeConversations(listener: () => void): () => void {
  conversationListeners.add(listener);
  return () => {
    conversationListeners.delete(listener);
  };
}

export function renameConversation(id: string, title: string): void {
  const current = conversationPatches.get(id) ?? {};
  conversationPatches.set(id, { ...current, title });
  notifyConversations();
}

export function setConversationPreview(id: string, preview: string): void {
  const current = conversationPatches.get(id) ?? {};
  conversationPatches.set(id, { ...current, preview });
  notifyConversations();
}

export function setConversationStarred(id: string, starred: boolean): void {
  const current = conversationPatches.get(id) ?? {};
  conversationPatches.set(id, { ...current, starred });
  notifyConversations();
}

export function removeConversation(id: string): void {
  const current = conversationPatches.get(id) ?? {};
  conversationPatches.set(id, { ...current, deleted: true });
  notifyConversations();
}

/** Add a session started on this phone so the sidebar can reopen it. */
export function rememberConversation(
  conversation: Conversation,
  initialMessages?: ChatMessage[],
): void {
  if (initialMessages && !messages[conversation.id]) {
    messages[conversation.id] = initialMessages;
  }
  if (allConversations().some((item) => item.id === conversation.id)) return;
  extraConversations.unshift(conversation);
  notifyConversations();
}

export function storeConversationMessages(
  id: string,
  transcript: ChatMessage[],
): void {
  messages[id] = transcript;
}

export function demoConversations(): Conversation[] {
  return allConversations()
    .filter((conversation) => !conversationPatches.get(conversation.id)?.deleted)
    .map(patched);
}

export function demoConversation(
  id: string,
  prompt?: string,
  mode: "agent" | "terminal" = "agent",
  image?: string,
): ConversationDetail {
  const existing = allConversations().find((item) => item.id === id);
  if (existing) {
    return { conversation: patched(existing), messages: messages[id] ?? [] };
  }

  const now = new Date().toISOString();
  const conversation: Conversation = {
    id,
    title:
      prompt?.slice(0, 54) ||
      (image ? "Photo" : mode === "terminal" ? "New terminal" : "New session"),
    preview: prompt || (image ? "Attached image" : "Empty session"),
    projectName: "goble",
    mode,
    status: "live",
    starred: false,
    updatedAt: now,
  };
  if (!prompt && !image) {
    return { conversation: patched(conversation), messages: [] };
  }

  const userMessage: ChatMessage = {
    id: "m1",
    role: mode === "terminal" ? "terminal" : "user",
    text: prompt || "Attached image",
    command: mode === "terminal" ? prompt || "open image" : undefined,
    cwd: mode === "terminal" ? "workspace/goble" : undefined,
    image,
    at: now,
    status: mode === "terminal" ? "done" : undefined,
  };
  const reply: ChatMessage = {
    id: "m2",
    role: "assistant",
    text:
      mode === "terminal"
        ? "The terminal bridge is not connected to the gateway yet. The command is queued in this demo session; the next slice sends it to the worker PTY."
        : image
          ? "I have the image and the prompt. The gateway run API is the next slice; until it lands this session stays local and shows the shape of the turn."
          : "I have the prompt. The gateway run API is the next slice; until it lands this session stays local and shows the shape of the transcript.",
    at: new Date(Date.now() + 1000).toISOString(),
  };
  return {
    conversation: patched(conversation),
    messages: [userMessage, reply],
  };
}

export function demoSnapshot(): HomeSnapshot {
  return {
    server: {
      url: null,
      name: "Demo workspace",
      online: false,
      version: "0.1.0",
    },
    updatedAt: new Date().toISOString(),
    source: "demo",
  };
}
