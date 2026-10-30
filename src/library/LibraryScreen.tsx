import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { moveSelection } from "../menu";
import { groupIssues, type ScanResult } from "./types";

type Props = {
  onBack: () => void;
};

function LibraryScreen({ onBack }: Props) {
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const selectedRow = useRef<HTMLLIElement>(null);
  const panelBody = useRef<HTMLDivElement>(null);
  const album = scan?.albums[selected];

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

    function onKeyDown(e: KeyboardEvent) {
      if (scanning) return;
      if (e.key === "Escape") onBack();
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
  }, [scan, scanning, onBack]);

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
          <h2 className="panel-title">{album ? album.name : "Fix library"}</h2>
          <div ref={panelBody} className="panel-body scroll">
            {scanning && <p>Scanning…</p>}
            {!scanning && error && <p>{error}</p>}
            {!scanning && !error && !scan && <p>Press ↵ to choose a music folder. Nothing in it will be changed.</p>}
            {!scanning && scan && !album && <p>No audio files in {scan.root}.</p>}
            {!scanning && album && (
              <>
                <p className="path">{album.path}</p>
                <p>
                  {album.trackCount} {album.trackCount === 1 ? "track" : "tracks"}, checked for {scan?.profile}
                </p>
                {album.issues.length === 0 && <p className="diagnosis">No problems found.</p>}
                <ul className={album.issues.length > 0 ? "diagnosis" : undefined}>
                  {groupIssues(album.issues).map((group) => (
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
              </>
            )}
          </div>
        </section>
      </main>

      <footer className="footer">
        <span>↑↓ MOVE</span>
        <span>PGUP PGDN SCROLL</span>
        <span>↵ CHOOSE FOLDER</span>
        <span>ESC BACK</span>
      </footer>
    </>
  );
}

export default LibraryScreen;
