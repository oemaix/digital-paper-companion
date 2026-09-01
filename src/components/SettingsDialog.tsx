import { useCallback, useEffect, useMemo, useState } from "react";
import Dialog from "./Dialog";
import SyncSettings from "./SyncSettings";
import { ClockIcon } from "./icons";
import { useApp } from "../lib/store";
import {
  ipc,
  errorMessage,
  type AccessPoint,
  type ConfigEntry,
  type DeviceStatus,
  type WifiNetworkConfig,
} from "../lib/ipc";
import { formatBytes } from "../lib/format";
import { useT, currentLocale, LOCALES, LOCALE_LABEL } from "../lib/i18n";

export type SettingsTab = "application" | "device" | "sync";

/**
 * Settings dialog with Application, Device and Sync tabs
 * (docs/05 §3.3/§3.6; FR-SET-1…5, FR-SYN-1…8).
 */
export default function SettingsDialog({
  onClose,
  initialTab = "application",
}: {
  onClose: () => void;
  initialTab?: SettingsTab;
}) {
  const t = useT();
  const [tab, setTab] = useState<SettingsTab>(initialTab);

  return (
    <Dialog title={t("settings.title")} onClose={onClose} wide={tab !== "application"}>
      <div className="mb-4 flex border-b border-border" role="tablist">
        {(
          [
            ["application", t("settings.tabApplication")],
            ["device", t("settings.tabDevice")],
            ["sync", t("settings.tabSync")],
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
        <DeviceTab />
      ) : (
        <SyncSettings />
      )}
    </Dialog>
  );
}

function ApplicationTab() {
  const t = useT();
  const settings = useApp((s) => s.settings);
  const setTheme = useApp((s) => s.setTheme);
  const setLanguage = useApp((s) => s.setLanguage);

  const select =
    "border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none";

  return (
    <div className="flex flex-col gap-4">
      <label className="flex items-center justify-between gap-4">
        <span>{t("settings.theme")}</span>
        <select
          value={settings?.theme ?? "system"}
          onChange={(e) => void setTheme(e.target.value)}
          className={select}
        >
          <option value="system">{t("settings.themeSystem")}</option>
          <option value="light">{t("settings.themeLight")}</option>
          <option value="dark">{t("settings.themeDark")}</option>
        </select>
      </label>
      <label className="flex items-center justify-between gap-4">
        <span>{t("settings.language")}</span>
        <select
          value={settings?.language ?? "system"}
          onChange={(e) => void setLanguage(e.target.value)}
          className={select}
        >
          <option value="system">{t("settings.languageSystem")}</option>
          {LOCALES.map((loc) => (
            <option key={loc} value={loc}>
              {LOCALE_LABEL[loc]}
            </option>
          ))}
        </select>
      </label>
      <p className="text-xs text-text-secondary">{t("settings.syncHint")}</p>
    </div>
  );
}

// ---- device tab (FR-SET-1/2/3) ---------------------------------------------------

/** Config keys with a dedicated control; everything else lands in Advanced. */
const DEDICATED_KEYS = new Set([
  "owner",
  "timezone",
  "date_format",
  "time_format",
  "timeout_to_standby",
  "datetime",
]);

/** Display formats the firmware is known to accept (protocol §7.5). */
const DATE_FORMAT_OPTIONS = ["yyyy/mm/dd", "dd/mm/yyyy", "mm/dd/yyyy"];

function formatDeviceTime(iso: string | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString(currentLocale());
}

function DeviceTab() {
  const t = useT();
  const connection = useApp((s) => s.connection);
  const toast = useApp((s) => s.toast);
  const [status, setStatus] = useState<DeviceStatus | null>(null);
  const [configs, setConfigs] = useState<ConfigEntry[] | null>(null);
  const [edited, setEdited] = useState<Record<string, string>>({});
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [settingClock, setSettingClock] = useState(false);

  const connected = connection.state === "connected";

  useEffect(() => {
    if (!connected) return;
    ipc
      .deviceStatus()
      .then(setStatus)
      .catch((err) => toast(errorMessage(err), "error"));
  }, [connected, toast]);

  const refreshConfigs = useCallback(() => {
    if (!connected) return;
    ipc
      .deviceConfigs()
      .then(setConfigs)
      .catch((err) => toast(errorMessage(err), "error"));
  }, [connected, toast]);

  useEffect(() => {
    refreshConfigs();
  }, [refreshConfigs]);

  // IANA zone list from the runtime; empty → plain text input fallback.
  const timezones = useMemo<string[]>(() => {
    const intl = Intl as { supportedValuesOf?: (key: string) => string[] };
    try {
      return intl.supportedValuesOf?.("timeZone") ?? [];
    } catch {
      return [];
    }
  }, []);

  if (!connected) {
    return <p className="text-text-secondary">{t("device.notConnected")}</p>;
  }

  const byKey = new Map((configs ?? []).map((c) => [c.key, c.value]));
  const valueOf = (key: string) => edited[key] ?? byKey.get(key) ?? "";
  const isDirty = (key: string) =>
    edited[key] !== undefined && edited[key] !== byKey.get(key);
  const advanced = (configs ?? []).filter((c) => !DEDICATED_KEYS.has(c.key));

  const saveValue = async (key: string, value: string) => {
    setSavingKey(key);
    try {
      await ipc.setDeviceConfig(key, value);
      setConfigs(
        (prev) => prev?.map((c) => (c.key === key ? { ...c, value } : c)) ?? prev,
      );
      setEdited((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
      toast(t("device.configSaved", { key }));
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSavingKey(null);
    }
  };
  const saveKey = (key: string) => saveValue(key, valueOf(key));

  const setClock = async () => {
    setSettingClock(true);
    try {
      await ipc.setDeviceClock();
      toast(t("device.clockSet"));
      refreshConfigs(); // the shown device time changed
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSettingClock(false);
    }
  };

  /** Free-text config value with an explicit save button. */
  const editableRow = (key: string, label: string) => (
    <div key={key} className="flex items-center gap-2">
      <label htmlFor={`cfg-${key}`} className="w-44 shrink-0 text-text-secondary">
        {label}
      </label>
      <input
        id={`cfg-${key}`}
        value={valueOf(key)}
        onChange={(e) => setEdited((prev) => ({ ...prev, [key]: e.target.value }))}
        onKeyDown={(e) => e.key === "Enter" && isDirty(key) && void saveKey(key)}
        className="min-w-0 flex-1 border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none"
      />
      <button
        onClick={() => void saveKey(key)}
        disabled={!isDirty(key) || savingKey === key}
        className="border border-border px-2 py-1 text-xs hover:border-text disabled:opacity-40"
      >
        {savingKey === key ? t("common.saving") : t("common.save")}
      </button>
    </div>
  );

  /** Enumerated config value; saves immediately on change. The current
   *  device value is kept selectable even when it is not a known option. */
  const selectRow = (
    key: string,
    label: string,
    options: { value: string; label: string }[],
  ) => {
    const current = byKey.get(key) ?? "";
    const known = options.some((o) => o.value === current);
    return (
      <div key={key} className="flex items-center gap-2">
        <label htmlFor={`cfg-${key}`} className="w-44 shrink-0 text-text-secondary">
          {label}
        </label>
        <select
          id={`cfg-${key}`}
          value={current}
          disabled={savingKey === key || configs === null}
          onChange={(e) => void saveValue(key, e.target.value)}
          className="min-w-0 flex-1 border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none disabled:opacity-50"
        >
          {!known && <option value={current}>{current || "—"}</option>}
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </div>
    );
  };

  return (
    <div className="flex flex-col gap-4 text-[13px]">
      {(status === null || configs === null) && (
        <p className="text-text-secondary">{t("device.loadingStatus")}</p>
      )}

      {/* Basic: owner + hardware facts (FR-SET-1/2) */}
      {byKey.has("owner") && editableRow("owner", t("device.owner"))}
      {status && (
        <dl className="grid grid-cols-[11rem_1fr] gap-x-2 gap-y-1.5">
          <dt className="text-text-secondary">{t("device.device")}</dt>
          <dd>{status.model ?? connection.name ?? "Digital Paper"}</dd>
          <dt className="text-text-secondary">{t("device.serial")}</dt>
          <dd className="tabular-nums">{status.serial}</dd>
          <dt className="text-text-secondary">{t("device.firmware")}</dt>
          <dd className="tabular-nums">{status.firmware ?? "—"}</dd>
          <dt className="text-text-secondary">{t("device.mac")}</dt>
          <dd className="tabular-nums">{status.mac_address ?? "—"}</dd>
          <dt className="text-text-secondary">{t("device.battery")}</dt>
          <dd className="tabular-nums">
            {status.battery.level != null ? `${status.battery.level} %` : "—"}
            {status.battery.plugged === "connected" ? t("device.charging") : ""}
          </dd>
          <dt className="text-text-secondary">{t("device.storage")}</dt>
          <dd className="tabular-nums">
            {t("device.storageUsed", {
              used: formatBytes(
                (status.storage.capacity ?? 0) - (status.storage.available ?? 0),
              ),
              total: formatBytes(status.storage.capacity),
            })}
          </dd>
        </dl>
      )}
      {byKey.has("timeout_to_standby") &&
        editableRow("timeout_to_standby", t("device.standbyTimeout"))}

      {/* Date & time (FR-SET-3) */}
      {configs !== null && (
        <section className="border-t border-border pt-3">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("device.dateTime")}
          </h3>
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center gap-2">
              <span className="w-44 shrink-0 text-text-secondary">
                {t("device.currentTime")}
              </span>
              <span className="min-w-0 flex-1 tabular-nums">
                {formatDeviceTime(byKey.get("datetime"))}
              </span>
              <button
                title={t("device.setClock")}
                aria-label={t("device.setClock")}
                disabled={settingClock}
                onClick={() => void setClock()}
                className="border border-border p-1 text-text-secondary hover:border-text hover:text-text disabled:opacity-40"
              >
                <ClockIcon
                  width={14}
                  height={14}
                  className={settingClock ? "animate-pulse" : ""}
                />
              </button>
            </div>
            {timezones.length > 0
              ? selectRow(
                  "timezone",
                  t("device.timezone"),
                  timezones.map((z) => ({ value: z, label: z })),
                )
              : byKey.has("timezone") && editableRow("timezone", t("device.timezone"))}
            {selectRow(
              "date_format",
              t("device.dateFormat"),
              DATE_FORMAT_OPTIONS.map((f) => ({ value: f, label: f })),
            )}
            {selectRow("time_format", t("device.timeFormat"), [
              { value: "12hour", label: t("device.timeFormat12") },
              { value: "24hour", label: t("device.timeFormat24") },
            ])}
          </div>
        </section>
      )}

      <WifiSection />

      {/* Advanced: every remaining key, last (FR-SET-2) */}
      {advanced.length > 0 && (
        <section className="border-t border-border pt-3">
          <details>
            <summary className="cursor-pointer text-[13px] text-text-secondary hover:text-text">
              {t("device.advanced")} ({advanced.length})
            </summary>
            <p className="mb-2 mt-1 text-xs text-text-secondary">
              {t("device.advancedHint")}
            </p>
            <div
              className="grid grid-cols-[minmax(10rem,auto)_1fr_auto] items-center gap-x-2 gap-y-1 text-xs"
              role="table"
              aria-label={t("device.advanced")}
            >
              <span className="font-semibold uppercase tracking-wide text-text-secondary">
                {t("device.advancedKey")}
              </span>
              <span className="font-semibold uppercase tracking-wide text-text-secondary">
                {t("device.advancedValue")}
              </span>
              <span />
              {advanced.map((c) => (
                <AdvancedRow
                  key={c.key}
                  entry={c}
                  value={valueOf(c.key)}
                  dirty={isDirty(c.key)}
                  saving={savingKey === c.key}
                  onChange={(v) => setEdited((prev) => ({ ...prev, [c.key]: v }))}
                  onSave={() => void saveKey(c.key)}
                />
              ))}
            </div>
          </details>
        </section>
      )}
    </div>
  );
}

function AdvancedRow({
  entry,
  value,
  dirty,
  saving,
  onChange,
  onSave,
}: {
  entry: ConfigEntry;
  value: string;
  dirty: boolean;
  saving: boolean;
  onChange: (v: string) => void;
  onSave: () => void;
}) {
  const t = useT();
  return (
    <>
      <span className="truncate font-mono" title={entry.key}>
        {entry.key}
      </span>
      <input
        value={value}
        aria-label={entry.key}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && dirty && onSave()}
        className="min-w-0 border border-border bg-bg px-1.5 py-0.5 font-mono focus:border-text focus:outline-none"
      />
      <button
        onClick={onSave}
        disabled={!dirty || saving}
        className="border border-border px-1.5 py-0.5 hover:border-text disabled:opacity-40"
      >
        {saving ? t("common.saving") : t("common.save")}
      </button>
    </>
  );
}

// ---- Wi-Fi (FR-SET-4) -----------------------------------------------------------

function WifiSection() {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [stored, setStored] = useState<AccessPoint[] | null>(null);
  const [visible, setVisible] = useState<AccessPoint[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [joining, setJoining] = useState<AccessPoint | "manual" | null>(null);

  const refresh = useCallback(() => {
    ipc
      .wifiEnabled()
      .then(setEnabled)
      .catch(() => setEnabled(null));
    ipc
      .wifiStoredNetworks()
      .then(setStored)
      .catch(() => setStored(null));
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const toggleRadio = async (on: boolean) => {
    if (!on && !window.confirm(t("wifi.offWarning"))) return;
    try {
      await ipc.setWifiEnabled(on);
      setEnabled(on);
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const scan = async () => {
    setScanning(true);
    try {
      setVisible(await ipc.wifiScan());
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setScanning(false);
    }
  };

  const remove = async (ap: AccessPoint) => {
    if (!window.confirm(t("wifi.removeConfirm", { ssid: ap.ssid }))) return;
    try {
      await ipc.wifiRemoveNetwork(ap.ssid, ap.security);
      toast(t("wifi.removed", { ssid: ap.ssid }));
      refresh();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const btn =
    "border border-border px-2 py-1 text-xs hover:border-text disabled:opacity-40";

  return (
    <section className="border-t border-border pt-3">
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-text-secondary">
        {t("wifi.title")}
      </h3>

      <div className="flex items-center justify-between gap-4 text-[13px]">
        <span>{t("wifi.radio")}</span>
        <select
          value={enabled == null ? "" : enabled ? "on" : "off"}
          disabled={enabled == null}
          onChange={(e) => void toggleRadio(e.target.value === "on")}
          aria-label={t("wifi.radio")}
          className="border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none"
        >
          {enabled == null && <option value="">—</option>}
          <option value="on">{t("wifi.on")}</option>
          <option value="off">{t("wifi.off")}</option>
        </select>
      </div>

      <div className="mt-3 text-[13px]">
        <h4 className="mb-1 text-xs text-text-secondary">{t("wifi.storedNetworks")}</h4>
        {stored == null || stored.length === 0 ? (
          <p className="text-xs text-text-secondary">{t("wifi.noStored")}</p>
        ) : (
          <ul className="border border-border">
            {stored.map((ap) => (
              <li
                key={`${ap.ssid}/${ap.security}`}
                className="flex items-center gap-2 border-b border-border px-2 py-1.5 last:border-b-0"
              >
                <span className="min-w-0 flex-1 truncate" title={ap.ssid}>
                  {ap.ssid}
                </span>
                <span className="text-xs text-text-secondary">{ap.security}</span>
                <button onClick={() => void remove(ap)} className={btn}>
                  {t("common.remove")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="mt-3 flex items-center gap-2">
        <button onClick={() => void scan()} disabled={scanning} className={btn}>
          {scanning ? t("wifi.scanning") : t("wifi.scan")}
        </button>
        <button onClick={() => setJoining("manual")} className={btn}>
          {t("wifi.joinTitle")}…
        </button>
      </div>

      {visible && (
        <div className="mt-2 text-[13px]">
          <h4 className="mb-1 text-xs text-text-secondary">{t("wifi.scanResults")}</h4>
          {visible.length === 0 ? (
            <p className="text-xs text-text-secondary">{t("wifi.noneVisible")}</p>
          ) : (
            <ul className="border border-border">
              {visible.map((ap, i) => (
                <li
                  key={`${ap.ssid}/${ap.security}/${i}`}
                  className="flex items-center gap-2 border-b border-border px-2 py-1.5 last:border-b-0"
                >
                  <span className="min-w-0 flex-1 truncate" title={ap.ssid}>
                    {ap.ssid}
                  </span>
                  <span className="text-xs text-text-secondary">{ap.security}</span>
                  <button onClick={() => setJoining(ap)} className={btn}>
                    {t("wifi.join")}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {joining && (
        <JoinNetworkDialog
          preset={joining === "manual" ? null : joining}
          onClose={() => setJoining(null)}
          onAdded={() => {
            setJoining(null);
            refresh();
          }}
        />
      )}
    </section>
  );
}

function JoinNetworkDialog({
  preset,
  onClose,
  onAdded,
}: {
  preset: AccessPoint | null;
  onClose: () => void;
  onAdded: () => void;
}) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const [cfg, setCfg] = useState<WifiNetworkConfig>({
    ssid: preset?.ssid ?? "",
    // The scan reports things like "WPA2-PSK"; the register call wants
    // "psk" or "nonsec" (protocol §7.6).
    security: preset == null || /psk|wpa/i.test(preset.security) ? "psk" : "nonsec",
    passwd: "",
    dhcp: true,
    static_address: "",
    gateway: "",
    network_mask: "",
    dns1: "",
    dns2: "",
    proxy: false,
  });

  const add = async () => {
    setBusy(true);
    try {
      await ipc.wifiAddNetwork(cfg);
      toast(t("wifi.added", { ssid: cfg.ssid }));
      onAdded();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  const field =
    "border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none";
  const valid =
    cfg.ssid.trim() !== "" && (cfg.security === "nonsec" || cfg.passwd !== "");

  return (
    <Dialog title={t("wifi.joinTitle")} onClose={onClose}>
      <div className="flex flex-col gap-3 text-[13px]">
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("wifi.ssid")}</span>
          <input
            value={cfg.ssid}
            disabled={preset != null}
            onChange={(e) => setCfg({ ...cfg, ssid: e.target.value })}
            className={`${field} disabled:opacity-60`}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("wifi.security")}</span>
          <select
            value={cfg.security}
            onChange={(e) => setCfg({ ...cfg, security: e.target.value })}
            className={field}
          >
            <option value="psk">{t("wifi.securityPsk")}</option>
            <option value="nonsec">{t("wifi.securityOpen")}</option>
          </select>
        </label>
        {cfg.security === "psk" && (
          <label className="flex flex-col gap-1">
            <span className="text-text-secondary">{t("wifi.password")}</span>
            <input
              type="password"
              value={cfg.passwd}
              onChange={(e) => setCfg({ ...cfg, passwd: e.target.value })}
              className={field}
            />
            <span className="text-xs text-text-secondary">{t("wifi.passwordHint")}</span>
          </label>
        )}

        <details>
          <summary className="cursor-pointer text-text-secondary hover:text-text">
            {t("wifi.staticConfig")}
          </summary>
          <div className="mt-2 flex flex-col gap-2">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={cfg.dhcp}
                onChange={(e) => setCfg({ ...cfg, dhcp: e.target.checked })}
              />
              {t("wifi.dhcp")}
            </label>
            {!cfg.dhcp && (
              <div className="grid grid-cols-2 gap-2">
                {(
                  [
                    ["static_address", t("wifi.staticAddress")],
                    ["gateway", t("wifi.gateway")],
                    ["network_mask", t("wifi.networkMask")],
                    ["dns1", t("wifi.dns1")],
                    ["dns2", t("wifi.dns2")],
                  ] as const
                ).map(([key, label]) => (
                  <label key={key} className="flex flex-col gap-1">
                    <span className="text-xs text-text-secondary">{label}</span>
                    <input
                      value={cfg[key]}
                      onChange={(e) => setCfg({ ...cfg, [key]: e.target.value })}
                      className={field}
                    />
                  </label>
                ))}
              </div>
            )}
          </div>
        </details>

        <div className="flex justify-end gap-2 border-t border-border pt-3">
          <button
            onClick={onClose}
            className="border border-border px-3 py-1.5 hover:border-text"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => void add()}
            disabled={busy || !valid}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
          >
            {busy ? t("wifi.adding") : t("wifi.add")}
          </button>
        </div>
      </div>
    </Dialog>
  );
}
