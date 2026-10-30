export type MenuItem = {
  id: string;
  label: string;
  detail: string;
};

export const MENU_ITEMS: MenuItem[] = [
  {
    id: "library",
    label: "Fix library",
    detail: "Scan a folder, flag Echo Mini problems, write repaired copies.",
  },
  {
    id: "rip",
    label: "Rip disc",
    detail: "Read the disc, look it up, rip with error correction, encode.",
  },
  {
    id: "system",
    label: "System",
    detail: "Build information reported by the Rust backend.",
  },
];

/** Moves the selection one row, stopping at either end of the list. */
export function moveSelection(index: number, key: string, count: number): number {
  if (key === "ArrowDown") return Math.min(index + 1, count - 1);
  if (key === "ArrowUp") return Math.max(index - 1, 0);
  return index;
}
