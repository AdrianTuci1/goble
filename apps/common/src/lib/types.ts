export type DataSource = "demo" | "live" | "offline";

export type SessionMode = "agent" | "terminal";

export type SessionStatus = "live" | "waiting" | "done";

export type ThinkingLevel = "low" | "standard" | "high";

export interface Model {
  id: string;
  name: string;
  vendor: string;
  detail: string;
}

export interface Conversation {
  id: string;
  title: string;
  preview: string;
  projectName: string;
  mode: SessionMode;
  /** The harness the conversation runs on: a catalog id, `goble` for the app's own. */
  harness: string;
  status: SessionStatus;
  starred: boolean;
  /** A chat that leaves nothing behind: it never reaches the sidebar. */
  temporary?: boolean;
  updatedAt: string;
}

export interface Agent {
  id: string;
  name: string;
  model: string;
  tools: string;
  summary: string;
  status: "ready" | "paused";
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "tool" | "terminal";
  text: string;
  at: string;
  command?: string;
  output?: string;
  /** `skipped` marks a proposed command the user refused to run. */
  status?: "done" | "running" | "waiting_approval" | "skipped" | "error";
  /** A local image attached to the turn. */
  image?: string;
  /** Terminal block context, drawn above the command. */
  cwd?: string;
  branch?: string;
  duration?: string;
}

export interface ScheduledTask {
  id: string;
  name: string;
  schedule: string;
  detail: string;
  nextRun: string;
  lastRun?: string;
  status: "active" | "paused";
}

export interface ConversationDetail {
  conversation: Conversation;
  messages: ChatMessage[];
}

export interface ServerInfo {
  /** Configured gateway URL, without a trailing slash. */
  url: string | null;
  name: string;
  online: boolean;
  version?: string;
}

export interface HomeSnapshot {
  server: ServerInfo;
  updatedAt: string;
  source: DataSource;
}

/** An image the composer holds, with its bytes ready for the worker. */
export interface PickedAttachment {
  /** Local URI, used for the preview in the transcript and the composer. */
  uri: string;
  name: string;
  mime: string;
  /** Base64 of the file's bytes. Empty when the platform gave no bytes. */
  base64: string;
}
