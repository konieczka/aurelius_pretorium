// Agents view: what `agent_configs` returns, the settings the page offers per agent, and how a setting's value is
// worked out across an agent's files.

import type { Provider } from "./usageStats";

export type Scope = "managed" | "local" | "project" | "global";
export type Format = "json" | "jsonc" | "toml";
export type ConfigFile = {
  path: string;
  scope: Scope;
  label: string;
  format: Format;
  exists: boolean;
  readOnly: boolean;
  text: string; // "" when missing
  values: Record<string, unknown> | null; // null when it doesn't parse
  error: string | null;
};
export type Model = { id: string; name: string; efforts: string[] };
/** `files` come highest precedence first: the first one that sets a key decides it. */
export type AgentConfig = { agent: Provider; installed: boolean; files: ConfigFile[]; models: Model[] };
export type Scan = { agents: AgentConfig[]; home: string };

export type Option = { value: string; label?: string };
export type Field = {
  key: string[];
  label: string;
  help: string;
  kind: "text" | "select" | "bool" | "number";
  options?: Option[];
  /** Typed in freely, with these offered. */
  suggest?: string[];
  /** Options are the reasoning efforts of the model that `effortOf` sets (when the agent lists its models). */
  effortOf?: string[];
  placeholder?: string;
};

const opts = (...values: string[]): Option[] => values.map((value) => ({ value }));
const k = (key: string) => key.split(".");

// ponytail: a few settings per agent, checked against each CLI's current release (Oct 2026); everything else is
// edited in the file. Codex's and Grok's models come from their own model caches.
export const FIELDS: Record<Provider, Field[]> = {
  claude: [
    { key: k("model"), label: "Model", kind: "text", help: "Model for new sessions: an alias or a full model ID.", suggest: ["opus", "sonnet", "haiku", "fable", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-fable-5-1"] },
    { key: k("effortLevel"), label: "Effort", kind: "select", help: "How much the model thinks before answering.", options: opts("low", "medium", "high", "xhigh", "max") },
    {
      key: k("permissions.defaultMode"),
      label: "Permission mode",
      kind: "select",
      help: "What Claude may do without asking, at the start of a session.",
      options: [
        { value: "default", label: "Ask (default)" },
        { value: "acceptEdits", label: "Accept edits" },
        { value: "plan", label: "Plan only" },
        { value: "auto", label: "Auto" },
        { value: "dontAsk", label: "Don't ask (deny unlisted)" },
        { value: "bypassPermissions", label: "Bypass permissions" },
      ],
    },
    { key: k("alwaysThinkingEnabled"), label: "Extended thinking", kind: "bool", help: "Think before every answer by default." },
    { key: k("outputStyle"), label: "Output style", kind: "text", help: "How Claude writes its replies.", suggest: ["default", "Explanatory", "Learning"] },
    { key: k("cleanupPeriodDays"), label: "Keep transcripts", kind: "number", help: "Days to keep chat transcripts (30 when not set).", placeholder: "30" },
  ],
  codex: [
    { key: k("model"), label: "Model", kind: "text", help: "Model for new sessions." },
    { key: k("model_reasoning_effort"), label: "Reasoning effort", kind: "select", help: "How much the model reasons.", options: opts("low", "medium", "high", "xhigh"), effortOf: k("model") },
    { key: k("plan_mode_reasoning_effort"), label: "Plan mode effort", kind: "select", help: "Reasoning effort while planning.", options: opts("low", "medium", "high", "xhigh"), effortOf: k("model") },
    {
      key: k("approval_policy"),
      label: "Approvals",
      kind: "select",
      help: "When Codex stops to ask before running a command.",
      options: [
        { value: "untrusted", label: "Untrusted (ask for most commands)" },
        { value: "on-request", label: "On request (model decides)" },
        { value: "never", label: "Never ask" },
      ],
    },
    {
      key: k("sandbox_mode"),
      label: "Sandbox",
      kind: "select",
      help: "What commands may touch.",
      options: [
        { value: "read-only", label: "Read only" },
        { value: "workspace-write", label: "Write in the workspace" },
        { value: "danger-full-access", label: "Full access (no sandbox)" },
      ],
    },
    { key: k("web_search"), label: "Web search", kind: "select", help: "Whether the model can search the web, and how fresh results are.", options: opts("disabled", "cached", "indexed", "live") },
  ],
  grok: [
    { key: k("models.default"), label: "Model", kind: "text", help: "Model for new sessions." },
    { key: k("models.default_reasoning_effort"), label: "Reasoning effort", kind: "select", help: "Default effort for models that support it.", options: opts("low", "medium", "high", "xhigh"), effortOf: k("models.default") },
    {
      key: k("ui.permission_mode"),
      label: "Permission mode",
      kind: "select",
      help: "Default for tool permissions.",
      options: [{ value: "default" }, { value: "ask" }, { value: "auto" }, { value: "always-approve", label: "always approve" }],
    },
    { key: k("ui.screen_mode"), label: "Screen mode", kind: "select", help: "How plain `grok` draws itself. Takes effect after a restart.", options: opts("fullscreen", "minimal") },
    { key: k("ui.theme"), label: "Theme", kind: "text", help: "Color theme name, or auto to follow the system.", suggest: ["auto"] },
    { key: k("session.auto_compact_threshold_percent"), label: "Auto-compact at", kind: "number", help: "Percent of the context window that triggers compaction (85 when not set).", placeholder: "85" },
    { key: k("cli.auto_update"), label: "Auto-update", kind: "bool", help: "Check for updates on launch." },
  ],
  antigravity: [],
  opencode: [
    { key: k("model"), label: "Model", kind: "text", help: "Model for new sessions, as provider/model.", placeholder: "anthropic/claude-sonnet-5-5" },
    { key: k("small_model"), label: "Small model", kind: "text", help: "Model for light work like titles.", placeholder: "anthropic/claude-haiku-5-5" },
    { key: k("theme"), label: "Theme", kind: "text", help: "Color theme name.", suggest: ["opencode", "system"] },
    { key: k("autoupdate"), label: "Auto-update", kind: "bool", help: "Download updates on launch." },
    { key: k("share"), label: "Sharing", kind: "select", help: "Whether sessions can be shared, or are shared automatically.", options: opts("manual", "auto", "disabled") },
    { key: k("permission.edit"), label: "File edits", kind: "select", help: "Edits without asking, after asking, or never.", options: opts("ask", "allow", "deny") },
    { key: k("permission.bash"), label: "Shell commands", kind: "select", help: "Commands without asking, after asking, or never.", options: opts("ask", "allow", "deny") },
  ],
  // Keys checked with `muse exec -c key=value`, which refuses unknown ones. Approvals and permission profiles are set
  // per run (--approval-mode) or in the file.
  muse: [
    { key: k("model"), label: "Model", kind: "text", help: "Model for new sessions.", suggest: ["muse-spark-1.3", "muse-spark-1.2"] },
    {
      key: k("reasoning_effort"),
      label: "Reasoning effort",
      kind: "select",
      help: "How much the model reasons (high when not set). Ultra only where your plan offers it.",
      options: opts("none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"),
    },
    { key: k("tui.terminal_background"), label: "Theme", kind: "select", help: "Colors for a dark or light terminal, or detected.", options: opts("auto", "dark", "light") },
    { key: k("session_retention_days"), label: "Keep sessions", kind: "number", help: "Days to keep session logs (forever when not set).", placeholder: "forever" },
    { key: k("telemetry.enabled"), label: "Telemetry", kind: "bool", help: "Send usage telemetry to Meta." },
  ],
};

export const scopeOrder: Scope[] = ["global", "project", "local"];

/** The value at `key` in parsed file contents, if set. */
export function getPath(values: unknown, key: string[]): unknown {
  let v = values;
  for (const part of key) {
    if (v === null || typeof v !== "object" || Array.isArray(v)) return undefined;
    v = (v as Record<string, unknown>)[part];
  }
  return v;
}

/** Which file decides `key`, and its value there: the first (highest-precedence) file that sets it. */
export function effective(files: ConfigFile[], key: string[]) {
  for (const file of files) {
    const value = getPath(file.values, key);
    if (value !== undefined) return { file, value };
  }
  return null;
}

/** Whether the form can show `value` in `field`'s control; anything else is edited in the file. */
export function fits(field: Field, value: unknown) {
  if (value === undefined) return true;
  if (field.kind === "bool") return typeof value === "boolean";
  if (field.kind === "number") return typeof value === "number";
  return typeof value === "string";
}

/** A select's options: the field's own, or the efforts the chosen model takes, plus a value set outside them. */
export function optionsFor(field: Field, files: ConfigFile[], models: Model[], current: unknown): Option[] {
  let list = field.options ?? [];
  if (field.effortOf) {
    const model = effective(files, field.effortOf)?.value;
    const efforts = models.find((m) => m.id === model)?.efforts;
    if (efforts?.length) list = efforts.map((value) => list.find((o) => o.value === value) ?? { value });
  }
  if (typeof current === "string" && current && !list.some((o) => o.value === current)) list = [...list, { value: current }];
  return list;
}

/** Turn a control's input into the value to store; undefined unsets the key. */
export function parseInput(field: Field, input: string): unknown {
  const s = input.trim();
  if (!s) return undefined;
  if (field.kind === "bool") return s === "true";
  if (field.kind === "number") {
    const n = Number(s);
    return Number.isFinite(n) ? n : undefined;
  }
  return s;
}

/** Keys whose values are credentials: blurred in the file editor until hovered. */
export const SECRET_KEY = /(api[_-]?key|token|secret|password|passwd|credential|authorization|bearer|private[_-]?key)/i;

/** [from, to) ranges of quoted string values given to secret-looking keys in `line`, JSON or TOML. */
export function secretRanges(line: string): [number, number][] {
  const out: [number, number][] = [];
  const re = /(["']?)([\w.$-]+)\1\s*[:=]\s*("(?:[^"\\]|\\.)*"|'[^']*')/g;
  for (let m; (m = re.exec(line)); ) {
    if (!SECRET_KEY.test(m[2])) continue;
    const to = m.index + m[0].length;
    const from = to - m[3].length;
    if (m[3].length > 2) out.push([from + 1, to - 1]); // the text inside the quotes
  }
  return out;
}

/** New-file text for an editor: an empty object for JSON, nothing for TOML. */
export const blank = (f: ConfigFile) => (f.format === "toml" ? "" : "{\n}\n");
