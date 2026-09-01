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
  type KnownDevice,
  type SyncStatus,
  type SyncPairInfo,
  type SyncConfirmationRequest,
  type SyncRunRecord,
} from "./ipc";
import { t, useI18n } from "./i18n";

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
  knownDevices: KnownDevice[];
  toasts: Toast[];
  syncStatus: SyncStatus;
  syncPairs: SyncPairInfo[] | null;
  syncConfirmation: SyncConfirmationRequest | null;
  lastSyncRecord: SyncRunRecord | null;

  init: () => Promise<void>;
  refreshEntries: (force?: boolean) => Promise<void>;
  refreshSyncPairs: () => Promise<void>;
  refreshKnownDevices: () => Promise<void>;
  setTheme: (theme: string) => Promise<void>;
  setLanguage: (language: string) => Promise<void>;
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
  knownDevices: [],
  toasts: [],
  syncStatus: { running: null, queued: [], pending_confirmation: null },
  syncPairs: null,
  syncConfirmation: null,
  lastSyncRecord: null,

  init: async () => {
    if (initialized) return;
    initialized = true;

    await listen<ConnectionPayload>(EVENTS.connectionChanged, (event) => {
      const prev = get().connection.state;
      set({ connection: event.payload });
      if (event.payload.state === "connected" && prev !== "connected") {
        void get().refreshEntries(true);
        // A pairing/import may have added a device (FR-CONN-7).
        void get().refreshKnownDevices();
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
    await listen<SyncStatus>(EVENTS.syncUpdated, (event) => {
      set({
        syncStatus: event.payload,
        syncConfirmation: event.payload.pending_confirmation ?? null,
      });
    });
    await listen<SyncConfirmationRequest>(EVENTS.syncConfirmationRequired, (event) => {
      set({ syncConfirmation: event.payload });
    });
    await listen<SyncRunRecord>(EVENTS.syncFinished, (event) => {
      const r = event.payload;
      set({ lastSyncRecord: r, syncConfirmation: null });
      const label =
        r.result === "ok"
          ? t("sync.finishedToast", { n: r.done })
          : r.result === "cancelled"
            ? t("sync.cancelledToast")
            : t("sync.failedToast", {
                result: r.result,
                done: r.done,
                failed: r.failed,
              });
      get().toast(label, r.result === "failed" ? "error" : "info");
      void get().refreshSyncPairs();
    });

    const [version, settings, connection, transfers, syncStatus] = await Promise.all([
      ipc.appVersion(),
      ipc.getSettings(),
      ipc.connectionState(),
      ipc.transferList(),
      ipc.syncStatus(),
    ]);
    set({ version, settings, connection, transfers, syncStatus });
    applyTheme(settings.theme);
    useI18n.getState().setLanguageSetting(settings.language ?? "system");
    void get().refreshSyncPairs();
    void get().refreshKnownDevices();
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

  refreshSyncPairs: async () => {
    try {
      const syncPairs = await ipc.syncPairs();
      set({ syncPairs });
    } catch (err) {
      console.error("failed to load sync pairs", err);
    }
  },

  refreshKnownDevices: async () => {
    try {
      const knownDevices = await ipc.knownDevices();
      set({ knownDevices });
    } catch (err) {
      console.error("failed to load known devices", err);
    }
  },

  setTheme: async (theme) => {
    await ipc.setTheme(theme);
    const settings = get().settings;
    set({ settings: settings ? { ...settings, theme } : settings });
    applyTheme(theme);
  },

  setLanguage: async (language) => {
    await ipc.setLanguage(language);
    const settings = get().settings;
    set({ settings: settings ? { ...settings, language } : settings });
    useI18n.getState().setLanguageSetting(language);
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
