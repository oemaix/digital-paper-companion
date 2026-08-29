import { useEffect, useState } from "react";
import Dialog from "./Dialog";
import SyncSettings from "./SyncSettings";
import { useApp } from "../lib/store";
import { ipc, errorMessage, type DeviceStatus } from "../lib/ipc";
import { formatBytes } from "../lib/format";

export type SettingsTab = "application" | "device" | "sync";

/**
 * Settings dialog with Application, Device and Sync tabs
 * (docs/05 §3.3/§3.6; FR-SET-1/3, FR-SYN-1…8).
 */
export default function SettingsDialog({
  onClose,
  initialTab = "application",
}: {
  onClose: () => void;
  initialTab?: SettingsTab;
}) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);

  return (
    <Dialog title="Settings" onClose={onClose} wide={tab === "sync"}>
      <div className="mb-4 flex border-b border-border" role="tablist">
        {(
          [
            ["application", "Application"],
            ["device", "Device"],
            ["sync", "Sync"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={`-mb-px border-b-2 px-3 py-1.5 text-[13px] ${
              tab === id
                ? "border-text font-medium text-text"
                : "border-transparent text-text-secondary hover:text-text"
            }`}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === "application" ? (
        <ApplicationTab />
      ) : tab === "device" ? (
        <DeviceTab onClose={onClose} />
      ) : (
        <SyncSettings />
      )}
    </Dialog>
  );
}

function ApplicationTab() {
  const settings = useApp((s) => s.settings);
  const setTheme = useApp((s) => s.setTheme);

  return (
    <div className="flex flex-col gap-4">
      <label className="flex items-center justify-between gap-4">
        <span>Theme</span>
        <select
          value={settings?.theme ?? "system"}
          onChange={(e) => void setTheme(e.target.value)}
          className="border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none"
        >
          <option value="system">Follow system</option>
          <option value="light">Light</option>
          <option value="dark">Dark</option>
        </select>
      </label>
      <p className="text-xs text-text-secondary">
        Folder sync is configured in the Sync tab.
      </p>
    </div>
  );
}

function DeviceTab({ onClose }: { onClose: () => void }) {
  const connection = useApp((s) => s.connection);
  const toast = useApp((s) => s.toast);
  const [status, setStatus] = useState<DeviceStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [settingClock, setSettingClock] = useState(false);

  const connected = connection.state === "connected";

  useEffect(() => {
    if (!connected) return;
    setLoading(true);
    ipc
      .deviceStatus()
      .then(setStatus)
      .catch((err) => toast(errorMessage(err), "error"))
      .finally(() => setLoading(false));
  }, [connected, toast]);

  if (!connected) {
    return (
      <p className="text-text-secondary">
        No device connected. Connect a device to see its status.
      </p>
    );
  }

  const setClock = async () => {
    setSettingClock(true);
    try {
      await ipc.setDeviceClock();
      toast("Device clock set to this computer's time");
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSettingClock(false);
    }
  };

  const disconnect = async () => {
    await ipc.disconnectDevice();
    onClose();
  };

  const storagePct =
    status?.storage.capacity && status.storage.available != null
      ? 1 - status.storage.available / status.storage.capacity
      : null;

  return (
    <div className="flex flex-col gap-4">
      {loading && <p className="text-text-secondary">Loading device status…</p>}
      {status && (
        <>
          <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-[13px]">
            <dt className="text-text-secondary">Device</dt>
            <dd>{status.model ?? connection.name ?? "Digital Paper"}</dd>
            <dt className="text-text-secondary">Serial</dt>
            <dd className="tabular-nums">{status.serial}</dd>
            <dt className="text-text-secondary">Firmware</dt>
            <dd className="tabular-nums">{status.firmware ?? "—"}</dd>
            <dt className="text-text-secondary">MAC address</dt>
            <dd className="tabular-nums">{status.mac_address ?? "—"}</dd>
            <dt className="text-text-secondary">Battery</dt>
            <dd className="tabular-nums">
              {status.battery.level != null ? `${status.battery.level} %` : "—"}
              {status.battery.plugged === "connected" ? " · charging" : ""}
            </dd>
            <dt className="text-text-secondary">Storage</dt>
            <dd className="tabular-nums">
              {formatBytes(
                (status.storage.capacity ?? 0) - (status.storage.available ?? 0),
              )}{" "}
              of {formatBytes(status.storage.capacity)} used
            </dd>
          </dl>
          {storagePct != null && (
            <div
              className="h-1.5 w-full border border-border"
              role="progressbar"
              aria-valuenow={Math.round(storagePct * 100)}
            >
              <div
                className="h-full bg-text"
                style={{ width: `${Math.round(storagePct * 100)}%` }}
              />
            </div>
          )}
        </>
      )}

      <div className="flex gap-2 border-t border-border pt-4">
        <button
          onClick={setClock}
          disabled={settingClock}
          className="border border-border px-3 py-1.5 hover:border-text disabled:opacity-50"
        >
          {settingClock ? "Setting clock…" : "Set clock to this computer"}
        </button>
        <button
          onClick={disconnect}
          className="ml-auto border border-border px-3 py-1.5 text-text-secondary hover:border-text hover:text-text"
        >
          Disconnect
        </button>
      </div>
    </div>
  );
}
