/**
 * Global frontend state (zustand): connection, entry cache, transfer queue,
 * settings and toasts. Subscribes to backend events once at startup.
 */
import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import {
  ipc,
  EVENTS,
  errorMessage,
  type ConnectionPayload,
  type Entry,
  type JobSnapshot,
  type AppSettings,
} from "./ipc";

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
}

interface AppStore {
  version: string;
  connection: ConnectionPayload;
  entries: Entry[] | null;
  entriesLoading: boolean;
  transfers: JobSnapshot[];
  settings: AppSettings | null;
  toasts: Toast[];

  init: () => Promise<void>;
  refreshEntries: (force?: boolean) => Promise<void>;
  setTheme: (theme: string) => Promise<void>;
  toast: (text: string, kind?: Toast["kind"]) => void;
  dismissToast: (id: number) => void;
}

let nextToastId = 1;
let initialized = false;

export const useApp = create<AppStore>((set, get) => ({
  version: "",
  connection: { state: "disconnected", serial: null, name: null },
  entries: null,
  entriesLoading: false,
  transfers: [],
  settings: null,
  toasts: [],

  init: async () => {
    if (initialized) return;
    initialized = true;

    await listen<ConnectionPayload>(EVENTS.connectionChanged, (event) => {
      const prev = get().connection.state;
      set({ connection: event.payload });
      if (event.payload.state === "connected" && prev !== "connected") {
        void get().refreshEntries(true);
      }
      if (event.payload.state === "disconnected") {
        set({ entries: null });
      }
    });
    await listen(EVENTS.entriesInvalidated, () => {
      void get().refreshEntries(true);
    });
    await listen<JobSnapshot[]>(EVENTS.transferUpdated, (event) => {
      set({ transfers: event.payload });
    });

    const [version, settings, connection, transfers] = await Promise.all([
      ipc.appVersion(),
      ipc.getSettings(),
      ipc.connectionState(),
      ipc.transferList(),
    ]);
    set({ version, settings, connection, transfers });
    applyTheme(settings.theme);
    if (connection.state === "connected") {
      void get().refreshEntries();
    }
  },

  refreshEntries: async (force = false) => {
    if (get().connection.state !== "connected") return;
    if (get().entriesLoading) return;
    set({ entriesLoading: true });
    try {
      const entries = await ipc.listEntries(force);
      set({ entries });
    } catch (err) {
      get().toast(errorMessage(err), "error");
    } finally {
      set({ entriesLoading: false });
    }
  },

  setTheme: async (theme) => {
    await ipc.setTheme(theme);
    const settings = get().settings;
    set({ settings: settings ? { ...settings, theme } : settings });
    applyTheme(theme);
  },

  toast: (text, kind = "info") => {
    const id = nextToastId++;
    set({ toasts: [...get().toasts, { id, text, kind }] });
    window.setTimeout(() => get().dismissToast(id), kind === "error" ? 8000 : 4000);
  },

  dismissToast: (id) => {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },
}));

/** Applies the theme choice: `light` | `dark` | `system` (docs/05 §5.1). */
export function applyTheme(theme: string) {
  const root = document.documentElement;
  root.dataset.theme = theme === "light" || theme === "dark" ? theme : "system";
}
