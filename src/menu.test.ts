import { describe, expect, it } from "vitest";
import { moveSelection } from "./menu";

describe("moveSelection", () => {
  it("moves down and up", () => {
    expect(moveSelection(0, "ArrowDown", 3)).toBe(1);
    expect(moveSelection(2, "ArrowUp", 3)).toBe(1);
  });

  it("stops at both ends", () => {
    expect(moveSelection(2, "ArrowDown", 3)).toBe(2);
    expect(moveSelection(0, "ArrowUp", 3)).toBe(0);
  });

  it("ignores other keys", () => {
    expect(moveSelection(1, "Enter", 3)).toBe(1);
  });
});
