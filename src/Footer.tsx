import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatBytes, type PlayerVolume } from "./library/types";

const POLL_MS = 3000;

/** Keys and what they do, such as ["↵", "SELECT"]. */
export type Hint = [string, string];

function Footer({ hints }: { hints: Hint[] }) {
  const [player, setPlayer] = useState<PlayerVolume | null>(null);

  useEffect(() => {
    function check() {
      invoke<PlayerVolume[]>("find_player_volumes")
        .then((found) => setPlayer(found[0] ?? null))
        .catch(console.error);
    }
    check();
    const timer = setInterval(check, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  return (
    <footer className="footer">
      {hints.map(([key, label]) => (
        <span key={key}>
          <kbd className="key">{key}</kbd> {label}
        </span>
      ))}
      <span className="status">
        {player ? `${player.name} ■ ${formatBytes(player.freeBytes)} FREE` : "NO PLAYER"}
      </span>
    </footer>
  );
}

export default Footer;
