import { describe, expect, it } from "vitest";
import { formatBytes, groupIssues, type Issue } from "./types";

function issue(code: string, file: string, detail: string | null = null): Issue {
  return { code, problem: `${code} problem`, fix: `${code} fix`, file, detail };
}

describe("groupIssues", () => {
  it("counts each problem once per file and keeps first-seen order", () => {
    const groups = groupIssues([issue("b", "1"), issue("a", "1"), issue("b", "2")]);
    expect(groups.map((g) => [g.code, g.count])).toEqual([
      ["b", 2],
      ["a", 1],
    ]);
  });

  it("collects distinct details", () => {
    const groups = groupIssues([issue("a", "1", "X"), issue("a", "2", "X"), issue("a", "3", "Y")]);
    expect(groups[0].details).toEqual(["X", "Y"]);
  });
});

describe("formatBytes", () => {
  it("uses MB below a gigabyte and GB above", () => {
    expect(formatBytes(240_000_000)).toBe("240 MB");
    expect(formatBytes(6_624_718_848)).toBe("6.6 GB");
  });
});
