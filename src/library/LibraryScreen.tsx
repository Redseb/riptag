import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { moveSelection } from "../menu";
import { groupIssues, type IssueGroup, type RepairProgress, type RepairReport, type ScanResult } from "./types";

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
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const selectedRow = useRef<HTMLLIElement>(null);
  const panelBody = useRef<HTMLDivElement>(null);
  const album = scan?.albums[selected];
  const busy = scanning || repairing;

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
      try {
        setReport(await invoke<RepairReport>("repair_library", { source, output, onProgress }));
      } catch (e) {
        setError(String(e));
      } finally {
        setRepairing(false);
      }
    }

    function onKeyDown(e: KeyboardEvent) {
      if (scanning || repairing) return;
      if (e.key === "Escape") {
        if (report || error) {
          setReport(null);
          setError(null);
        } else {
          onBack();
        }
      }
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
  }, [scan, scanning, repairing, report, error, onBack]);

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
          <h2 className="panel-title">{report ? "Repair finished" : album ? album.name : "Fix library"}</h2>
          <div ref={panelBody} className="panel-body scroll">
            {scanning && <p>Scanning…</p>}
            {repairing && <p>Repairing… {progress && `${progress.done} of ${progress.total}`}</p>}
            {busy || !error ? null : <p>{error}</p>}
            {report && !busy && (
              <>
                <p className="path">{report.output}</p>
                <p>
                  {report.written} written, {report.skipped.length} skipped
                </p>
                {report.tagsUntouched > 0 && <p>{report.tagsUntouched} copied without tag cleanup (not FLAC or MP3)</p>}
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
        <span>↑↓ MOVE</span>
        <span>PGUP PGDN SCROLL</span>
        <span>↵ CHOOSE FOLDER</span>
        {scan && scan.albums.length > 0 && <span>R REPAIR</span>}
        <span>ESC BACK</span>
      </footer>
    </>
  );
}

export default LibraryScreen;
