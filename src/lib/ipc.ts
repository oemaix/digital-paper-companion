/**
 * Typed bindings for the IPC commands exposed by dpt-app
 * (src-tauri/src/commands.rs; docs/04 §5.2).
 */
import { invoke } from "@tauri-apps/api/core";

// ---- types mirrored from the Rust side -------------------------------------

export type ConnectionState =
  "disconnected" | "connecting" | "connected" | "reauthenticating";

export interface ConnectionPayload {
  state: ConnectionState;
  serial: string | null;
  name: string | null;
}

export interface Entry {
  entry_id: string;
  entry_name: string;
  entry_path: string;
  entry_type: "document" | "folder";
  parent_folder_id?: string | null;
  created_date?: string | null;
  modified_date?: string | null;
  reading_date?: string | null;
  file_size?: number | null;
  file_revision?: string | null;
  mime_type?: string | null;
  title?: string | null;
  total_page?: number | null;
  is_new?: boolean | null;
}

export interface KnownDevice {
  serial: string;
  name: string;
  model?: string | null;
  last_address?: string | null;
}

export interface Discovered {
  address: string;
  name: string;
  port: number;
  serial: string | null;
  model: string | null;
  paired: boolean;
  connected: boolean;
}

export interface DeviceInfo {
  serial_number: string;
  model_name?: string | null;
  firmware_version?: string | null;
}

export interface DeviceStatus {
  serial: string;
  model?: string | null;
  firmware?: string | null;
  mac_address?: string | null;
  battery: { level?: number | null; status?: string | null; plugged?: string | null };
  storage: { capacity?: number | null; available?: number | null };
}

export interface JobSnapshot {
  id: number;
  kind: "upload" | "download";
  name: string;
  status: "queued" | "running" | "done" | "failed" | "cancelled";
  progress?: number | null;
  error?: string | null;
}

export interface AppSettings {
  version: number;
  theme: string;
  last_active_serial?: string | null;
}

export interface AppError {
  code: string;
  message: string;
}

export interface UploadItem {
  local_path: string;
  file_name: string;
  existing_doc_id?: string | null;
}

// ---- sync (FR-SYN-*) ---------------------------------------------------------

export type SyncMode = "two-way" | "mirror-to-local" | "mirror-to-remote";

export interface SyncPair {
  id: string;
  name: string;
  local_root: string;
  remote_root: string;
  mode: SyncMode;
  on_connect: boolean;
  interval_minutes?: number | null;
  deletion_threshold: number;
  filters: string[];
  enabled: boolean;
}

export interface SyncPairInfo extends SyncPair {
  last_run?: SyncRunRecord | null;
}

export type SyncActionKind =
  | "create_local_dir"
  | "create_remote_dir"
  | "upload"
  | "download"
  | "conflict_resolve"
  | "delete_local"
  | "delete_remote"
  | "delete_local_dir"
  | "delete_remote_dir"
  | "adopt"
  | "forget";

export interface SyncAction {
  kind: SyncActionKind;
  relpath: string;
  winner?: "local" | "remote";
  keep_copy?: boolean;
  fetch_copy?: boolean;
}

export interface SyncPlanSummary {
  uploads: number;
  downloads: number;
  conflicts: number;
  delete_local: number;
  delete_remote: number;
  create_local_dirs: number;
  create_remote_dirs: number;
  adopts: number;
}

export interface SyncPlan {
  actions: SyncAction[];
  summary: SyncPlanSummary;
  warnings: string[];
}

export interface SyncRunRecord {
  pair_id: string;
  /** Serial of the device this run talked to (multi-device hub model). */
  device_serial?: string;
  trigger: string;
  started_at: string;
  finished_at: string;
  result: "ok" | "partial" | "cancelled" | "failed";
  summary?: SyncPlanSummary | null;
  done: number;
  failed: number;
  skipped: number;
  conflicts: string[];
  errors: string[];
  warnings: string[];
}

export interface SyncRunningStatus {
  pair_id: string;
  phase: string;
  done: number;
  total: number;
  current?: string | null;
}

export interface SyncConfirmationRequest {
  pair_id: string;
  pair_name: string;
  threshold: number;
  local_deletions: string[];
  remote_deletions: string[];
}

export interface SyncStatus {
  running?: SyncRunningStatus | null;
  queued: string[];
  pending_confirmation?: SyncConfirmationRequest | null;
}

export interface ExcludedSyncAction {
  kind: SyncActionKind;
  relpath: string;
}

/** Event channel names (must match state.rs `events`). */
export const EVENTS = {
  connectionChanged: "connection:changed",
  entriesInvalidated: "entries:invalidated",
  transferUpdated: "transfer:updated",
  syncUpdated: "sync:updated",
  syncConfirmationRequired: "sync:confirmation-required",
  syncFinished: "sync:finished",
} as const;

// ---- commands ---------------------------------------------------------------

export const ipc = {
  appVersion: () => invoke<string>("app_version"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  setTheme: (theme: string) => invoke<void>("set_theme", { theme }),

  connectionState: () => invoke<ConnectionPayload>("connection_state"),
  discoverDevices: (seconds?: number) =>
    invoke<Discovered[]>("discover_devices", { seconds }),
  probeDevice: (address: string) => invoke<DeviceInfo>("probe_device", { address }),
  knownDevices: () => invoke<KnownDevice[]>("known_devices"),
  connectKnownDevice: (serial: string, address?: string) =>
    invoke<ConnectionPayload>("connect_known_device", { serial, address }),
  disconnectDevice: () => invoke<void>("disconnect_device"),
  forgetDevice: (serial: string) => invoke<void>("forget_device", { serial }),

  startPairing: (address: string) => invoke<DeviceInfo>("start_pairing", { address }),
  submitPairingPin: (pin: string) =>
    invoke<ConnectionPayload>("submit_pairing_pin", { pin }),
  cancelPairing: () => invoke<void>("cancel_pairing"),

  listEntries: (refresh?: boolean) => invoke<Entry[]>("list_entries", { refresh }),
  resolveFolder: (path: string) => invoke<Entry>("resolve_folder", { path }),
  createRemoteFolder: (parentFolderId: string, name: string) =>
    invoke<void>("create_remote_folder", { parentFolderId, name }),
  deleteEntries: (ids: string[]) => invoke<void>("delete_entries", { ids }),
  renameEntry: (id: string, newName: string) =>
    invoke<void>("rename_entry", { id, newName }),
  moveEntries: (ids: string[], targetFolderId: string) =>
    invoke<void>("move_entries", { ids, targetFolderId }),
  openEntry: (id: string) => invoke<void>("open_entry", { id }),
  openOnDevice: (id: string, page?: number) =>
    invoke<void>("open_on_device", { id, page }),

  deviceStatus: () => invoke<DeviceStatus>("device_status"),
  setDeviceClock: () => invoke<void>("set_device_clock"),

  uploadFiles: (destFolderId: string, items: UploadItem[]) =>
    invoke<number[]>("upload_files", { destFolderId, items }),
  uploadFolder: (destFolderId: string, destFolderPath: string, localDir: string) =>
    invoke<number[]>("upload_folder", { destFolderId, destFolderPath, localDir }),
  downloadEntries: (ids: string[], targetDir: string) =>
    invoke<number[]>("download_entries", { ids, targetDir }),
  classifyPaths: (paths: string[]) =>
    invoke<{ path: string; file_name: string; is_dir: boolean }[]>("classify_paths", {
      paths,
    }),
  transferList: () => invoke<JobSnapshot[]>("transfer_list"),
  transferCancel: (id: number) => invoke<void>("transfer_cancel", { id }),
  transfersClearFinished: () => invoke<void>("transfers_clear_finished"),

  syncPairs: () => invoke<SyncPairInfo[]>("sync_pairs"),
  syncPairUpsert: (pair: SyncPair) => invoke<SyncPair>("sync_pair_upsert", { pair }),
  syncPairDelete: (id: string) => invoke<void>("sync_pair_delete", { id }),
  syncPreview: (id: string) => invoke<SyncPlan>("sync_preview", { id }),
  syncRun: (id: string, confirmed?: boolean, excluded?: ExcludedSyncAction[]) =>
    invoke<void>("sync_run", { id, confirmed, excluded }),
  syncRunAll: () => invoke<number>("sync_run_all"),
  syncCancel: (id: string) => invoke<void>("sync_cancel", { id }),
  syncConfirm: (id: string, decision: "apply" | "skip-deletions" | "cancel") =>
    invoke<void>("sync_confirm", { id, decision }),
  syncHistory: (id: string) => invoke<SyncRunRecord[]>("sync_history", { id }),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
};

/** Normalizes an unknown thrown value into a user-presentable message. */
export function errorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as AppError).message);
  }
  return String(err);
}
