import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MENU_ITEMS, moveSelection } from "./menu";
import Footer from "./Footer";
import LibraryScreen from "./library/LibraryScreen";
import "./App.css";

type AppInfo = {
  version: string;
  os: string;
  arch: string;
};

function App() {
  const [screen, setScreen] = useState<"menu" | "library">("menu");
  const [selected, setSelected] = useState(0);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const item = MENU_ITEMS[selected];

  useEffect(() => {
    invoke<AppInfo>("get_app_info").then(setInfo).catch(console.error);
  }, []);

  useEffect(() => {
    if (screen !== "menu") return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") e.preventDefault();
      if (e.key === "Enter" && MENU_ITEMS[selected].id === "library") setScreen("library");
      setSelected((i) => moveSelection(i, e.key, MENU_ITEMS.length));
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [screen, selected]);

  return (
    <div className="app">
      <header className="header">
        <h1>Riptag</h1>
        <div className="divider">
          <span className="diamond" />
          <span className="divider-end" />
        </div>
      </header>

      {screen === "library" ? (
        <LibraryScreen onBack={() => setScreen("menu")} />
      ) : (
        <>
          <main className="layout">
            <ul className="menu">
              {MENU_ITEMS.map((menuItem, i) => (
                <li
                  key={menuItem.id}
                  className={i === selected ? "row selected" : "row"}
                  onClick={() => setSelected(i)}
                >
                  <span className="marker" />
                  {menuItem.label}
                </li>
              ))}
            </ul>

            <section className="panel">
              <h2 className="panel-title">
                <span>{item.label}</span>
              </h2>
              <div className="panel-body">
                <p>{item.detail}</p>
                {item.id === "system" && (
                  <dl className="data">
                    <dt>Version</dt>
                    <dd>{info?.version ?? "—"}</dd>
                    <dt>OS</dt>
                    <dd>{info?.os ?? "—"}</dd>
                    <dt>Arch</dt>
                    <dd>{info?.arch ?? "—"}</dd>
                  </dl>
                )}
              </div>
            </section>
          </main>

          <Footer hints={item.id === "library" ? [["↑↓", "MOVE"], ["↵", "SELECT"]] : [["↑↓", "MOVE"]]} />
        </>
      )}
    </div>
  );
}

export default App;
