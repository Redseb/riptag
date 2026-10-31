import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { moveSelection } from "../menu";
import {
  formatBytes,
  groupIssues,
  type IssueGroup,
  type LyricsReport,
  type PlayerVolume,
  type RepairProgress,
  type RepairReport,
  type ScanResult,
  type SyncReport,
} from "./types";

type Props = {
  onBack: () => void;
};

function IssueList({ groups }: { groups: IssueGroup[] }) {
  if (groups.length === 0) return null;
  return (
    <ul className="diagnosis">
      {groups.map((group) => (
        <li key={group.code} className="issue">
          <span className="marker" />
          <div>
            <p>
              {group.problem}
              {group.count > 1 && ` ×${group.count}`}
            </p>
            <p className="fix">→ {group.fix}</p>
            {group.details.length > 0 && <p className="fix">{group.details.slice(0, 3).join(" · ")}</p>}
          </div>
        </li>
      ))}
    </ul>
  );
}

function LibraryScreen({ onBack }: Props) {
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<RepairProgress | null>(null);
  const [repairing, setRepairing] = useState(false);
  const [report, setReport] = useState<RepairReport | null>(null);
  const [fetching, setFetching] = useState(false);
  const [lyrics, setLyrics] = useState<LyricsReport | null>(null);
  const [players, setPlayers] = useState<PlayerVolume[] | null>(null);
  const [player, setPlayer] = useState(0);
  const [syncing, setSyncing] = useState(false);
  const [synced, setSynced] = useState<SyncReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const selectedRow = useRef<HTMLLIElement>(null);
  const panelBody = useRef<HTMLDivElement>(null);
  const album = scan?.albums[selected];
  const busy = scanning || repairing || fetching || syncing;

  useEffect(() => {
    selectedRow.current?.scrollIntoView({ block: "nearest" });
    panelBody.current?.scrollTo({ top: 0 });
  }, [selected]);

  useEffect(() => {
    async function chooseFolder() {
      const path = await open({ directory: true, title: "Choose a music folder" });
      if (!path) return;
      setScanning(true);
      setError(null);
      setReport(null);
      setLyrics(null);
      setSynced(null);
      try {
        setScan(await invoke<ScanResult>("scan_library", { path }));
        setSelected(0);
      } catch (e) {
        setScan(null);
        setError(String(e));
      } finally {
        setScanning(false);
      }
    }

    async function repair(source: string) {
      const output = await open({
        directory: true,
        canCreateDirectories: true,
        title: "Choose where to write repaired copies",
      });
      if (!output) return;
      const onProgress = new Channel<RepairProgress>();
      onProgress.onmessage = setProgress;
      setRepairing(true);
      setProgress(null);
      setError(null);
      setLyrics(null);
      setSynced(null);
      try {
        setReport(await invoke<RepairReport>("repair_library", { source, output, onProgress }));
      } catch (e) {
        setError(String(e));
      } finally {
        setRepairing(false);
      }
    }

    async function fetchLyrics(source: string, finished: RepairReport) {
      const onProgress = new Channel<RepairProgress>();
      onProgress.onmessage = setProgress;
      setFetching(true);
      setProgress(null);
      setError(null);
      setSynced(null);
      try {
        const result = await invoke<LyricsReport>("fetch_lyrics", {
          source,
          output: finished.output,
          onProgress,
        });
        setLyrics(result);
        setReport({ ...finished, remaining: result.remaining });
      } catch (e) {
        setError(String(e));
      } finally {
        setFetching(false);
      }
    }

    async function choosePlayer() {
      setError(null);
      setSynced(null);
      try {
        const found = await invoke<PlayerVolume[]>("find_player_volumes");
        if (found.length === 0) {
          setError("No player found. Connect it and press S again.");
        } else {
          setPlayer(0);
          setPlayers(found);
        }
      } catch (e) {
        setError(String(e));
      }
    }

    async function syncTo(volume: PlayerVolume, finished: RepairReport) {
      const onProgress = new Channel<RepairProgress>();
      onProgress.onmessage = setProgress;
      setPlayers(null);
      setSyncing(true);
      setProgress(null);
      setError(null);
      try {
        setSynced(await invoke<SyncReport>("sync_to_device", { output: finished.output, volume: volume.path, onProgress }));
      } catch (e) {
        setError(String(e));
      } finally {
        setSyncing(false);
      }
    }

    function onKeyDown(e: KeyboardEvent) {
      if (busy) return;
      // Nothing is written until the player is confirmed here.
      if (players) {
        if (e.key === "Escape") setPlayers(null);
        if (e.key === "Enter" && report) syncTo(players[player], report);
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          setPlayer((i) => moveSelection(i, e.key, players.length));
        }
        return;
      }
      if (e.key === "Escape") {
        if (report || error) {
          setReport(null);
          setLyrics(null);
          setSynced(null);
          setError(null);
        } else {
          onBack();
        }
      }
      if (e.key.toLowerCase() === "l" && scan && report) fetchLyrics(scan.root, report);
      if (e.key.toLowerCase() === "s" && report) choosePlayer();
      if (e.key.toLowerCase() === "r" && scan && scan.albums.length > 0) repair(scan.root);
      if (e.key === "Enter") chooseFolder();
      if (e.key === "PageDown" || e.key === "PageUp") {
        e.preventDefault();
        const body = panelBody.current;
        if (body) body.scrollBy({ top: (e.key === "PageDown" ? 1 : -1) * body.clientHeight * 0.8 });
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setSelected((i) => moveSelection(i, e.key, scan?.albums.length ?? 0));
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [scan, busy, report, error, players, player, onBack]);

  const syncRows = synced
    ? [...synced.failed, ...synced.alreadyThere.map((file) => ({ file, reason: "Already on the player, left as it is" }))]
    : [];

  return (
    <>
      <main className="layout">
        <ul className="menu scroll">
          {scan?.albums.map((a, i) => (
            <li
              key={a.path}
              ref={i === selected ? selectedRow : null}
              className={i === selected ? "row selected" : "row"}
              onClick={() => setSelected(i)}
            >
              <span className="marker" />
              <span className="row-label">{a.name}</span>
              <span className="row-count">{a.issues.length || "OK"}</span>
            </li>
          ))}
        </ul>

        <section className="panel">
          <h2 className="panel-title">{players ? "Copy to player" : syncing ? "Syncing" : synced ? "Sync finished" : fetching ? "Lyrics" : report ? "Repair finished" : album ? album.name : "Fix library"}</h2>
          <div ref={panelBody} className="panel-body scroll">
            {scanning && <p>Scanning…</p>}
            {repairing && <p>Repairing… {progress && `${progress.done} of ${progress.total}`}</p>}
            {fetching && <p>Fetching lyrics… {progress && `${progress.done} of ${progress.total}`}</p>}
            {syncing && <p>Copying… {progress && `${progress.done} of ${progress.total}`}</p>}
            {busy || !error ? null : <p>{error}</p>}
            {players && report && (
              <>
                <p>Copy this folder to the player? Nothing on it gets replaced or removed.</p>
                <p className="path">{report.output}</p>
                <ul className="menu">
                  {players.map((volume, i) => (
                    <li key={volume.path} className={i === player ? "row selected" : "row"}>
                      <span className="marker" />
                      <span className="row-label">{volume.name}</span>
                      <span className="row-count">{formatBytes(volume.freeBytes)} free</span>
                    </li>
                  ))}
                </ul>
                <p className="path">{players[player].path}</p>
              </>
            )}
            {synced && !busy && !players && (
              <>
                <p>
                  {synced.copied} copied to {synced.device}, {synced.alreadyThere.length} already there, {synced.failed.length} failed
                </p>
                {synced.stopped && <p>The player was disconnected. Reconnect it and press S to copy the rest.</p>}
                {syncRows.length > 0 && (
                  <ul className="diagnosis">
                    {syncRows.map((row, i) => (
                      <li key={i} className="issue">
                        <span className="marker" />
                        <div>
                          <p className="path">{row.file}</p>
                          <p className="fix">{row.reason}</p>
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
            {report && !busy && !players && !synced && (
              <>
                <p className="path">{report.output}</p>
                <p>
                  {report.written} written, {report.skipped.length} skipped
                </p>
                {report.converted > 0 && <p>{report.converted} converted to FLAC</p>}
                {report.tagsUntouched > 0 &&<p>{report.tagsUntouched} copied without tag cleanup (not FLAC or MP3)</p>}
                {lyrics && (
                  <p>
                    Lyrics: {lyrics.saved} saved, {lyrics.notFound} not found
                    {lyrics.plainOnly > 0 && `, ${lyrics.plainOnly} unsynced only`}
                    {lyrics.instrumental > 0 && `, ${lyrics.instrumental} instrumental`}
                    {lyrics.alreadyHad > 0 && `, ${lyrics.alreadyHad} already had`}
                  </p>
                )}
                {lyrics?.stopped && <p>Stopped after repeated errors. Press L to try the rest again.</p>}
                {lyrics && lyrics.failed.length > 0 && (
                  <ul className="diagnosis">
                    {lyrics.failed.map((failed, i) => (
                      <li key={i} className="issue">
                        <span className="marker" />
                        <div>
                          <p className="path">{failed.file}</p>
                          <p className="fix">{failed.reason}</p>
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
                {report.skipped.length > 0 && (
                  <ul className="diagnosis">
                    {report.skipped.map((skipped, i) => (
                      <li key={i} className="issue">
                        <span className="marker" />
                        <div>
                          <p className="path">{skipped.file}</p>
                          <p className="fix">{skipped.reason}</p>
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
                {report.remaining.length === 0 && <p className="diagnosis">The output has no remaining problems.</p>}
                <IssueList groups={groupIssues(report.remaining)} />
              </>
            )}
            {!busy && !error && !report && !scan && <p>Press ↵ to choose a music folder. Your originals are never changed.</p>}
            {!busy && !error && !report && scan && !album && <p>No audio files in {scan.root}.</p>}
            {!busy && !error && !report && album && (
              <>
                <p className="path">{album.path}</p>
                <p>
                  {album.trackCount} {album.trackCount === 1 ? "track" : "tracks"}, checked for {scan?.profile}
                </p>
                {album.issues.length === 0 && <p className="diagnosis">No problems found.</p>}
                <IssueList groups={groupIssues(album.issues)} />
              </>
            )}
          </div>
        </section>
      </main>

      <footer className="footer">
        {players ? (
          <>
            <span>↑↓ MOVE</span>
            <span>↵ COPY</span>
            <span>ESC CANCEL</span>
          </>
        ) : (
          <>
            <span>↑↓ MOVE</span>
            <span>PGUP PGDN SCROLL</span>
            <span>↵ CHOOSE FOLDER</span>
            {scan && scan.albums.length > 0 && <span>R REPAIR</span>}
            {report && <span>L LYRICS</span>}
            {report && <span>S SYNC</span>}
            <span>ESC BACK</span>
          </>
        )}
      </footer>
    </>
  );
}

export default LibraryScreen;
