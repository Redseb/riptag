import { describe, expect, it } from "vitest";
import { albumLabel, formatBytes, groupIssues, groupSkipped, issueTag, type Issue } from "./types";

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

describe("albumLabel", () => {
  it("uses the last folder", () => {
    expect(albumLabel("The Real... Bossa Nova CD1")).toBe("The Real... Bossa Nova CD1");
    expect(albumLabel("Artist/2014 - Album")).toBe("2014 - Album");
  });

  it("keeps the album beside a disc folder", () => {
    expect(albumLabel("Artist/2014 - Album/CD3")).toBe("CD3 · 2014 - Album");
    expect(albumLabel("CD3")).toBe("CD3");
  });
});

describe("issueTag", () => {
  it("names each family of issue", () => {
    expect(issueTag("folder_cover_missing")).toBe("ART");
    expect(issueTag("cover_progressive")).toBe("ART");
    expect(issueTag("lyrics_missing")).toBe("LRC");
    expect(issueTag("id3_v24")).toBe("TAG");
    expect(issueTag("layout_mismatch")).toBe("FS");
    expect(issueTag("unreadable")).toBe("ERR");
  });
});

describe("groupSkipped", () => {
  it("groups by reason without its detail", () => {
    const groups = groupSkipped([
      { file: "a", reason: "Already in the output: a" },
      { file: "b", reason: "Already in the output: b" },
      { file: "c", reason: "Couldn't write: full" },
    ]);
    expect(groups).toEqual([
      { reason: "Already in the output", files: ["a", "b"] },
      { reason: "Couldn't write", files: ["c"] },
    ]);
  });
});
