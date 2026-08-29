import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Skeleton shell: sidebar + status bar layout per docs/05 §2, with a
 * placeholder main area. Real views (Library, Notes, Templates, Sync,
 * Device, Settings) land in Phase 1.
 */
export default function App() {
  const [version, setVersion] = useState<string>("…");
  const [connection, setConnection] = useState<string>("unknown");

  useEffect(() => {
    invoke<string>("app_version").then(setVersion).catch(console.error);
    invoke<string>("connection_state").then(setConnection).catch(console.error);
  }, []);

  const navItems = ["Library", "Notes", "Templates", "Sync", "Device", "Settings"];

  return (
    <div className="flex h-screen flex-col">
      <div className="flex min-h-0 flex-1">
        <aside className="w-44 shrink-0 border-r border-(--color-border) bg-(--color-surface) p-2">
          <div className="px-2 py-3 text-sm font-semibold">Digital Paper</div>
          <nav className="flex flex-col gap-0.5">
            {navItems.map((item, i) => (
              <button
                key={item}
                className={`rounded-md px-2 py-1.5 text-left text-[13px] ${
                  i === 0
                    ? "bg-(--color-accent) text-white"
                    : "text-(--color-text-secondary) hover:bg-(--color-bg)"
                }`}
              >
                {item}
              </button>
            ))}
          </nav>
        </aside>

        <main className="flex min-w-0 flex-1 items-center justify-center">
          <div className="max-w-sm text-center">
            <h1 className="mb-2 text-xl font-semibold">No device connected</h1>
            <p className="mb-4 text-(--color-text-secondary)">
              Connect your Digital Paper to the same network, then pair it to
              get started.
            </p>
            <button className="rounded-lg bg-(--color-accent) px-4 py-2 text-white">
              Find my device
            </button>
          </div>
        </main>
      </div>

      <footer className="flex h-7 shrink-0 items-center gap-3 border-t border-(--color-border) bg-(--color-surface) px-3 text-xs text-(--color-text-secondary)">
        <span className="inline-flex items-center gap-1.5">
          <span className="size-2 rounded-full bg-(--color-danger)" />
          {connection}
        </span>
        <span className="ml-auto">v{version}</span>
      </footer>
    </div>
  );
}
