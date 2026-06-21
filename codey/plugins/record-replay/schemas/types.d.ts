/**
 * Record & Replay — Data Contract Definitions
 *
 * These types define the multi-layer evidence model for recording,
 * goal summarization, tool graph compilation, and skill generation.
 */

// ─── Layer 1: Software & Path Context ────────────────────────────────────────

export interface SessionContext {
  /** Name of the application being recorded (e.g. "Browser", "VS Code") */
  softwareName: string;
  /** Title of the active window or tab */
  windowTitle: string;
  /** OS process name if applicable */
  processName?: string;
  /** Current working URL or file system path */
  workingPath: string;
  /** Full URL if in a browser context */
  url?: string;
  /** ISO timestamp of when the session started */
  startedAt: string;
}

export interface RecordingSession {
  /** Unique session identifier */
  id: string;
  /** Human-readable session name */
  name: string;
  /** Software and path context captured at session start */
  context: SessionContext;
  /** Current session lifecycle state */
  status: "created" | "recording" | "recorded" | "compiled" | "error";
  /** All captured action events */
  events: ActionEvent[];
  /** All screenshot evidence records */
  screenshots: ScreenshotEvidence[];
}

// ─── Layer 2: Action & Element Evidence ──────────────────────────────────────

export type ActionType =
  | "click"
  | "type"
  | "navigate"
  | "select"
  | "submit"
  | "scroll"
  | "upload";

export interface ActionEvent {
  /** Unique event identifier */
  eventId: string;
  /** Type of user action */
  actionType: ActionType;
  /** Unix timestamp (ms) when the action occurred */
  timestamp: number;
  /** Element evidence for the action target */
  target: ElementEvidence;
  /** Input value if applicable (typed text, selected option, file name) */
  inputValue?: string | null;
  /** Page URL at time of action */
  url?: string;
  /** Page title at time of action */
  pageTitle?: string;
  /** Reference to pre-action state snapshot */
  preStateId: string;
  /** Reference to post-action state snapshot */
  postStateId: string;
  /** Associated screenshot evidence */
  screenshotEvidence: ScreenshotEvidence;
}

export interface ElementEvidence {
  /** Ordered list of locator strategies (most reliable first) */
  locatorCandidates: string[];
  /** ARIA role or HTML tag name */
  role: string;
  /** Accessible name (aria-label, name attr, alt text) */
  name: string;
  /** Visible text content (truncated to 100 chars) */
  text: string;
  /** Path to cropped screenshot of just this element */
  elementScreenshotPath?: string;
}

// ─── Layer 3: Screenshot Evidence ────────────────────────────────────────────

export interface ScreenshotEvidence {
  /** Full-page screenshot taken before the action */
  beforeShotPath: string | null;
  /** Cropped screenshot of the action target element */
  actionShotPath: string | null;
  /** Full-page screenshot taken after the action */
  afterShotPath: string | null;
  /** Viewport dimensions at capture time */
  viewportMeta: ViewportMeta;
}

export interface ViewportMeta {
  width: number;
  height: number;
}

// ─── Goal Summarization ──────────────────────────────────────────────────────

export interface GoalSummary {
  /** Short title describing the operational goal (e.g. "Login to CRM") */
  goalTitle: string;
  /** Detailed description of what the workflow accomplishes */
  goalDescription: string;
  /** Observable signals that indicate successful completion */
  successSignals: string[];
  /** Confidence score (0-1) that the goal summary is correct */
  confidence: number;
  /** If true, the goal needs user confirmation before publishing as a skill */
  needsConfirmation: boolean;
}

// ─── Tool Graph (Compiled Workflow) ──────────────────────────────────────────

export interface ToolGraphStep {
  /** Unique step identifier within the workflow */
  stepId: string;
  /** Name of the cn-codex tool to invoke (e.g. "browser_run") */
  toolName: string;
  /** Parameterized arguments with {{variable}} placeholders */
  argsTemplate: Record<string, unknown>;
  /** Step IDs that must complete before this step */
  dependsOn: string[];
  /** Description of how to verify step success */
  verification: string;
  /** Fallback strategy if step fails */
  fallback: string;
}

export interface WorkflowVariable {
  /** Variable type (date, email, url, filepath, string, number) */
  type: string;
  /** Human-readable description of what this variable represents */
  description: string;
  /** Default value captured during recording */
  default?: string;
}

export interface RecoveryPolicy {
  /** Maximum retry attempts per step */
  maxRetries: number;
  /** Strategy for handling failures */
  fallbackStrategy: "retry_with_alternative_locator" | "screenshot_and_relocate" | "ask_user";
}

// ─── Agent Skill Specification ───────────────────────────────────────────────

export interface AgentSkillSpec {
  /** URL-safe slug used as skill identifier */
  name: string;
  /** High-level objective description */
  objective: string;
  /** Phrases that should trigger this skill */
  triggerPhrases: string[];
  /** Variables that must be resolved before execution */
  variables: Record<string, WorkflowVariable>;
  /** Ordered tool graph for execution */
  toolGraph: ToolGraphStep[];
  /** Error recovery configuration */
  recoveryPolicy: RecoveryPolicy;
  /** Session ID of the source recording */
  sourceTrace: string;
  /** ISO timestamp of compilation */
  compiledAt: string;
}

// ─── Trace File Format ───────────────────────────────────────────────────────

export interface TraceFile {
  /** Session identifier */
  sessionId: string;
  /** Human-readable session name */
  sessionName: string;
  /** Context captured at recording start */
  context: SessionContext;
  /** All captured action events with evidence */
  events: ActionEvent[];
  /** ISO timestamp of when recording completed */
  recordedAt: string;
  /** Total number of events captured */
  eventCount: number;
}
