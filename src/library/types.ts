export type Issue = {
  code: string;
  problem: string;
  fix: string;
  file: string | null;
  detail: string | null;
};

export type Album = {
  path: string;
  name: string;
  trackCount: number;
  issues: Issue[];
};

export type ScanResult = {
  root: string;
  profile: string;
  trackCount: number;
  albums: Album[];
};

export type RepairReport = {
  output: string;
  written: number;
  converted: number;
  tagsUntouched: number;
  skipped: { file: string; reason: string }[];
  remaining: Issue[];
};

export type LyricsReport = {
  saved: number;
  alreadyHad: number;
  plainOnly: number;
  instrumental: number;
  notFound: number;
  stopped: boolean;
  failed: { file: string; reason: string }[];
  remaining: Issue[];
};

export type PlayerVolume = {
  name: string;
  path: string;
  freeBytes: number;
};

export type SyncReport = {
  device: string;
  copied: number;
  alreadyThere: string[];
  failed: { file: string; reason: string }[];
  stopped: boolean;
};

export type RepairProgress = {
  done: number;
  total: number;
};

export type IssueGroup = {
  code: string;
  problem: string;
  fix: string;
  count: number;
  /** Differing details across the group, such as tag names. */
  details: string[];
};

export function formatBytes(bytes: number): string {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

/** Short enough for a menu row. The disc comes first so truncating can't hide it. */
export function albumLabel(name: string): string {
  const parts = name.split("/");
  const last = parts[parts.length - 1];
  const isDisc = /^(cd|disc|disk)\s*\d+$/i.test(last);
  return isDisc && parts.length > 1 ? `${last} · ${parts[parts.length - 2]}` : last;
}

/** The chip shown beside an issue. */
export function issueTag(code: string): string {
  if (code.startsWith("cover") || code.startsWith("folder_cover")) return "ART";
  if (code.startsWith("lyrics")) return "LRC";
  if (code.startsWith("tags") || code.startsWith("id3")) return "TAG";
  if (code.startsWith("path") || code.startsWith("layout")) return "FS";
  if (code.startsWith("format")) return "FMT";
  return "ERR";
}

export type SkippedGroup = {
  reason: string;
  files: string[];
};

/** Groups skipped files by reason, ignoring any detail after a colon. */
export function groupSkipped(skipped: { file: string; reason: string }[]): SkippedGroup[] {
  const groups = new Map<string, SkippedGroup>();
  for (const { file, reason } of skipped) {
    const short = reason.split(":")[0];
    const group = groups.get(short) ?? { reason: short, files: [] };
    group.files.push(file);
    groups.set(short, group);
  }
  return [...groups.values()];
}

/** Collapses per-file issues so an album shows each problem once. */
export function groupIssues(issues: Issue[]): IssueGroup[] {
  const groups = new Map<string, IssueGroup>();
  for (const issue of issues) {
    let group = groups.get(issue.code);
    if (!group) {
      group = { code: issue.code, problem: issue.problem, fix: issue.fix, count: 0, details: [] };
      groups.set(issue.code, group);
    }
    group.count += 1;
    if (issue.detail && !group.details.includes(issue.detail)) {
      group.details.push(issue.detail);
    }
  }
  return [...groups.values()];
}
