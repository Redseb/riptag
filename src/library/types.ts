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
