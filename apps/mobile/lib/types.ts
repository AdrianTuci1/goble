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
  status: SessionStatus;
  starred: boolean;
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
  status?: "done" | "running" | "waiting_approval" | "error";
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
