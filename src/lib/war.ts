// War Table math: every project (its agents, its local servers and containers, its stats) and where each thing sits
// on the table in its two views: Castra (a walled camp per project) and Front line (bands by urgency, a column per
// project).
import { STATUSES, type Row, type Status } from "./fleet";
import type { Container, Listener } from "./Infra.svelte";
import type { Row as UsageRow } from "./usageStats";

export type WarView = "castra" | "front";

/** From `project_stats` (stats.rs). */
export type Stats = { lines: number; files: number; langs: { name: string; lines: number }[]; commits7d: number };
export type Tokens = { month: number; today: number };

/** A dev server (a process listening in the project's folder) or one of its containers. `agent`: the agent whose
 * terminal started it; `tab`: the terminal it runs in, whether an agent's or not. */
export type Service = {
  id: string;
  name: string;
  type: "process" | "container";
  detail: string;
  ports: number[];
  up: boolean;
  agent: string | null;
  tab: string | null;
  listener?: Listener;
  container?: Container;
};

export type Project = { key: string; name: string; agents: Row[]; services: Service[] };

const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
const rank = (s: Status) => STATUSES.indexOf(s);

/** Agents, servers and workspace folders, by project. `keyOf` maps a folder to its project (a repo's main checkout,
 * worktrees included), as Fleet groups them. */
export function buildProjects(rows: Row[], listeners: Listener[], containers: Container[], folders: string[], keyOf: (path: string) => string): Project[] {
  const all = new Map<string, Project>();
  const at = (path: string) => {
    const key = keyOf(path);
    return all.get(key) ?? all.set(key, { key, name: base(key), agents: [], services: [] }).get(key)!;
  };
  for (const f of folders) if (f) at(f);
  for (const r of rows) at(r.cwd).agents.push(r);
  const agentTabs = new Set(rows.map((r) => r.id));
  for (const l of listeners) {
    if (l.kind !== "project" || !l.root) continue;
    at(l.root).services.push({
      id: `p${l.pid}`, name: l.name, type: "process", detail: l.args, ports: l.ports, up: true,
      agent: l.tab && agentTabs.has(l.tab) ? l.tab : null, tab: l.tab, listener: l,
    });
  }
  for (const c of containers) {
    if (!c.root) continue;
    at(c.root).services.push({
      id: `c${c.id}`, name: c.service || c.name, type: "container", detail: c.image, ports: c.ports.map((p) => p.host), up: c.state === "running",
      agent: null, tab: null, container: c,
    });
  }
  for (const p of all.values()) p.agents.sort((a, b) => rank(a.status) - rank(b.status) || (a.since ?? Infinity) - (b.since ?? Infinity));
  // Most urgent first: who waits on you, then who's working, then the biggest; then by name, so the table holds still
  const t = new Map([...all.values()].map((p) => [p.key, tally(p.agents)]));
  return [...all.values()].sort((a, b) => {
    const x = t.get(a.key)!, y = t.get(b.key)!;
    return y.attention - x.attention || y.busy - x.busy || b.agents.length - a.agents.length || b.services.length - a.services.length || a.name.localeCompare(b.name);
  });
}

/** Projects in an order that holds still, for Castra's camps and the Front line's columns alike: as arranged by hand,
 * else as their workspaces are, then by name. Agents changing state never move a project. */
export function laneOrder(projects: Project[], arranged: string[], workspaces: string[]): Project[] {
  const at = (key: string) => {
    const i = arranged.indexOf(key);
    if (i >= 0) return i;
    const j = workspaces.indexOf(key);
    return j >= 0 ? arranged.length + j : Infinity;
  };
  return [...projects].sort((a, b) => at(a.key) - at(b.key) || a.name.localeCompare(b.name));
}

/** `keys` with `key` moved to position `to`. */
export function moveKey(keys: string[], key: string, to: number): string[] {
  const rest = keys.filter((k) => k !== key);
  rest.splice(Math.max(0, Math.min(to, rest.length)), 0, key);
  return rest;
}

export function tally(agents: Row[]): Record<Status, number> {
  const t = { attention: 0, busy: 0, idle: 0, asleep: 0 };
  for (const a of agents) t[a.status]++;
  return t;
}

/** Is `cwd` in project `key`: inside it, or one of its worktrees made beside it (`<project>-<branch>`)? */
function within(cwd: string, key: string) {
  return cwd === key || cwd.startsWith(key + "/") || cwd.startsWith(key + "-");
}

/** Tokens of the last 30 days and of today, by project: each session counts where it ran (the deepest project that
 * holds its folder). Sessions whose logs don't say where they ran count nowhere. */
export function tokensByProject(rows: UsageRow[], keys: string[], dayStart: number): Map<string, Tokens> {
  const out = new Map<string, Tokens>(keys.map((k) => [k, { month: 0, today: 0 }]));
  const deepest = [...keys].sort((a, b) => b.length - a.length);
  const owner = new Map<string, string | undefined>();
  for (const r of rows) {
    if (!r.cwd) continue;
    if (!owner.has(r.cwd)) owner.set(r.cwd, deepest.find((k) => within(r.cwd, k)));
    const k = owner.get(r.cwd);
    if (!k) continue;
    const t = out.get(k)!;
    t.month += r.total;
    if (r.hour >= dayStart) t.today += r.total;
  }
  return out;
}

const LANG: Record<string, string> = {
  TypeScript: "blue", TSX: "sky", JavaScript: "yellow", JSX: "sky", Svelte: "orange", Vue: "green", Astro: "orange",
  Rust: "rust", Go: "teal", Python: "violet", Ruby: "red", PHP: "violet", Java: "red", Kotlin: "violet", Swift: "orange",
  Dart: "sky", C: "blue", "C++": "blue", "Objective-C": "blue", "C#": "green", Zig: "amber", Lua: "violet", Elixir: "violet",
  Haskell: "violet", Scala: "red", Clojure: "green", SQL: "pink", Shell: "green", HTML: "red", CSS: "violet", SCSS: "pink",
  Markdown: "gray", MDX: "amber", JSON: "yellow", YAML: "pink", TOML: "rust", XML: "amber", GraphQL: "pink", Protobuf: "teal",
  Dockerfile: "sky", Makefile: "rust",
};
/** A language's colour: the file tree's for its files. */
export const langColor = (name: string) => `var(--fi-${LANG[name] ?? "gray"})`;

/** The top languages and the rest as "Other", as shares of all lines. */
export function langShares(s: Stats, top = 6) {
  if (!s.lines) return [];
  const shares = s.langs.filter((l) => l.name !== "Other").slice(0, top).map((l) => ({ name: l.name, share: l.lines / s.lines }));
  const rest = 1 - shares.reduce((a, l) => a + l.share, 0);
  return rest > 0.005 ? [...shares, { name: "Other", share: rest }] : shares;
}

/** 4.2k, 48.2k, 2.84M */
export function fmtNum(n: number) {
  if (n >= 1e6) return (n / 1e6).toFixed(2).replace(/\.?0+$/, "") + "M";
  if (n >= 1e4) return (n / 1e3).toFixed(1).replace(/\.0$/, "") + "k";
  return n.toLocaleString("en-US");
}

export type Rect = { x: number; y: number; w: number; h: number };
export function union(rs: Rect[]): Rect | null {
  if (!rs.length) return null;
  const x = Math.min(...rs.map((r) => r.x)), y = Math.min(...rs.map((r) => r.y));
  return { x, y, w: Math.max(...rs.map((r) => r.x + r.w)) - x, h: Math.max(...rs.map((r) => r.y + r.h)) - y };
}

/** Orthogonal polyline with softly rounded corners, as an SVG path. */
export function ortho(pts: [number, number][], r = 9) {
  pts = pts.filter((p, i) => i === 0 || p[0] !== pts[i - 1][0] || p[1] !== pts[i - 1][1]);
  let d = `M${pts[0][0]},${pts[0][1]}`;
  for (let i = 1; i < pts.length - 1; i++) {
    const [px, py] = pts[i - 1], [x, y] = pts[i], [nx, ny] = pts[i + 1];
    const l1 = Math.hypot(x - px, y - py), l2 = Math.hypot(nx - x, ny - y);
    const rr = Math.min(r, l1 / 2, l2 / 2);
    d += ` L${x - ((x - px) / l1) * rr},${y - ((y - py) / l1) * rr} Q${x},${y} ${x + ((nx - x) / l2) * rr},${y + ((ny - y) / l2) * rr}`;
  }
  const l = pts[pts.length - 1];
  return d + ` L${l[0]},${l[1]}`;
}

// ---- Castra: each project a walled plate on the survey grid; agents as coins in ranks, servers as towers on the wall

export const C = { T: 32, SLOT_W: 5, SLOT_H: 7, HEAD_H: 6, TOWER_W: 4, COIN: 56 } as const;

export type Camp = {
  key: string;
  x: number;
  y: number;
  w: number;
  h: number;
  plateH: number;
  units: { id: string; x: number; y: number }[];
  towers: { id: string; x: number; y: number }[];
  roads: { agent: string; service: string; d: string }[];
};

function camp(p: Project): Omit<Camp, "x" | "y"> {
  const { T, SLOT_W, SLOT_H, HEAD_H, TOWER_W, COIN } = C;
  const n = p.agents.length;
  const cols = n <= 4 ? Math.max(1, n) : Math.min(4, Math.ceil(n / 2));
  const ranks = Math.ceil(n / cols);
  const col = new Map(p.agents.map((a, i) => [a.id, i % cols]));
  // Towers an agent started first, in its coin's column order, so roads don't cross
  const towers = [...p.services].sort((a, b) => (a.agent ? col.get(a.agent)! : 99) - (b.agent ? col.get(b.agent)! : 99));
  const innerW = Math.max(13, n ? cols * SLOT_W : 0, towers.length * TOWER_W);
  const w = (innerW + 2) * T;
  const agentsLeft = (1 + Math.floor((innerW - cols * SLOT_W) / 2)) * T;
  const towersLeft = (1 + Math.floor((innerW - towers.length * TOWER_W) / 2)) * T;
  const ranksTop = HEAD_H * T;
  const ranksBottom = ranksTop + ranks * SLOT_H * T;
  const plateH = towers.length ? ranksBottom + 2 * T : ranksBottom + (n ? 0 : T);
  const towerTop = plateH - T;
  const units = p.agents.map((a, i) => ({ id: a.id, x: agentsLeft + (i % cols) * SLOT_W * T, y: ranksTop + Math.floor(i / cols) * SLOT_H * T }));
  const unitOf = new Map(units.map((u) => [u.id, u]));
  const placed = towers.map((s, i) => ({ id: s.id, x: towersLeft + i * TOWER_W * T + 4, y: towerTop }));
  const lanes = [...new Set(towers.flatMap((s) => (s.agent && unitOf.has(s.agent) ? [s.agent] : [])))];
  const roads = towers.flatMap((s, i) => {
    const u = s.agent ? unitOf.get(s.agent) : undefined;
    if (!u) return [];
    const cx = u.x + (SLOT_W * T) / 2, cy = u.y + 16 + COIN / 2;
    const lane = ranksBottom + 8 + lanes.indexOf(s.agent!) * 7;
    const tx = towersLeft + i * TOWER_W * T + (TOWER_W * T) / 2;
    return [{ agent: s.agent!, service: s.id, d: ortho([[cx - COIN / 2 - 6, cy], [u.x, cy], [u.x, lane], [tx, lane], [tx, towerTop]]) }];
  });
  return { key: p.key, w, h: towers.length ? plateH + T : plateH, plateH, units, towers: placed, roads };
}

/** Shelf-pack the camps in order (the first top-left), at the row width that fills a `sw`×`sh` stage best. */
export function castraLayout(projects: Project[], sw: number, sh: number): Camp[] {
  const items = projects.map(camp);
  if (!items.length) return [];
  const gx = 3 * C.T, gy = 2 * C.T;
  const tryW = (maxW: number) => {
    let x = 0, y = 0, rowH = 0, w = 0;
    const out: Camp[] = [];
    for (const it of items) {
      if (x > 0 && x + it.w > maxW) (y += rowH + gy), (x = 0), (rowH = 0);
      out.push({ ...it, x, y });
      x += it.w + gx;
      rowH = Math.max(rowH, it.h);
      w = Math.max(w, x - gx);
    }
    return { out, k: Math.min(sw / w, sh / (y + rowH)) };
  };
  let best = tryW(0);
  const sum = items.reduce((s, i) => s + i.w + gx, 0);
  for (let mw = Math.max(...items.map((i) => i.w)); mw <= sum + 1; mw += C.T) {
    const r = tryW(mw);
    if (r.k > best.k + 1e-6) best = r;
  }
  return best.out;
}

// ---- Front line: bands across the table (needs you, working, idle), a column per project, servers at its foot

export const F = { LANE_W: 288, GAP: 32, PADX: 22, HEAD_H: 196, PT: 38, PB: 16, CGAP: 10, CHIP_H: 30, CHIP_GAP: 6, CALM_H: 64 } as const;
export const CARD_H: Record<Status, number> = { attention: 140, busy: 102, idle: 96, asleep: 96 };
export const CARD_W = F.LANE_W - F.PADX * 2;

export type Band = "front" | "work" | "rear" | "supply";
export const BANDS: Band[] = ["front", "work", "rear", "supply"];
const bandOf = (s: Status): Band => (s === "attention" ? "front" : s === "busy" ? "work" : "rear");

export type Front = {
  lanes: { key: string; x: number; calm: boolean }[];
  bands: Record<Band, { y: number; h: number }>;
  cards: Record<string, { x: number; y: number; h: number }>;
  chips: Record<string, { x: number; y: number }>;
  links: { key: string; agent: string; service: string; d: string; dot: [number, number]; end: [number, number] }[];
  w: number;
  h: number;
};

export function frontLayout(projects: Project[]): Front {
  const { LANE_W, GAP, PADX, HEAD_H, PT, PB, CGAP, CHIP_H, CHIP_GAP, CALM_H } = F;
  const stack = (p: Project, b: Band) => p.agents.filter((a) => bandOf(a.status) === b);
  const stackH = (list: Row[]) => (list.length ? list.reduce((s, a) => s + CARD_H[a.status], 0) + CGAP * (list.length - 1) : 0);
  const bands = {} as Front["bands"];
  let y = HEAD_H;
  for (const b of ["front", "work", "rear"] as const) {
    const c = Math.max(b === "front" ? CALM_H : 48, ...projects.map((p) => stackH(stack(p, b))));
    bands[b] = { y, h: PT + c + PB };
    y += bands[b].h;
  }
  const most = Math.max(1, ...projects.map((p) => p.services.length));
  bands.supply = { y, h: PT + most * CHIP_H + (most - 1) * CHIP_GAP + PB + 4 };
  y += bands.supply.h;

  const cards: Front["cards"] = {}, chips: Front["chips"] = {}, links: Front["links"] = [];
  const lanes = projects.map((p, i) => {
    const x = i * (LANE_W + GAP);
    for (const b of ["front", "work", "rear"] as const) {
      let cy = bands[b].y + PT;
      for (const a of stack(p, b)) (cards[a.id] = { x: x + PADX, y: cy, h: CARD_H[a.status] }), (cy += CARD_H[a.status] + CGAP);
    }
    p.services.forEach((s, j) => (chips[s.id] = { x: x + PADX, y: bands.supply.y + PT + j * (CHIP_H + CHIP_GAP) }));
    // Agent -> server: out of the card's right edge, down the column's right rail, into the chip from the right
    const right = x + PADX + CARD_W, R = 7;
    const owners = [...new Set(p.services.flatMap((s) => (s.agent && cards[s.agent] ? [s.agent] : [])))];
    owners.forEach((agent, n) => {
      const c = cards[agent], rx = right + 7 + n * 6, ay = c.y + 31;
      for (const s of p.services.filter((s) => s.agent === agent)) {
        const sy = chips[s.id].y + CHIP_H / 2;
        links.push({ key: p.key, agent, service: s.id, d: `M${right} ${ay} H${rx - R} Q${rx} ${ay} ${rx} ${ay + R} V${sy - R} Q${rx} ${sy} ${rx - R} ${sy} H${right}`, dot: [right, ay], end: [right, sy] });
      }
    });
    return { key: p.key, x, calm: !stack(p, "front").length };
  });
  return { lanes, bands, cards, chips, links, w: Math.max(LANE_W, projects.length * (LANE_W + GAP) - GAP), h: y };
}
