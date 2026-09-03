import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Dialog from "./Dialog";
import SyncSettings from "./SyncSettings";
import IconButton from "./IconButton";
import {
  ClockToDeviceIcon,
  LockIcon,
  LockOpenIcon,
  LockWeakIcon,
  PencilIcon,
  PlusIcon,
  SearchIcon,
  TrashIcon,
} from "./icons";
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
  // Incremented by the "+" in the tab row; SyncSettings opens its editor on change.
  const [addPairRequest, setAddPairRequest] = useState(0);

  return (
    <Dialog title={t("settings.title")} onClose={onClose} width="md">
      <div className="mb-4 flex items-center border-b border-border" role="tablist">
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
        <span className="min-w-0 flex-1" />
        {tab === "sync" && (
          <IconButton
            title={t("sync.addPair")}
            onClick={() => setAddPairRequest((n) => n + 1)}
          >
            <PlusIcon width={14} height={14} />
          </IconButton>
        )}
      </div>
      {tab === "application" ? (
        <ApplicationTab />
      ) : tab === "device" ? (
        <DeviceTab />
      ) : (
        <SyncSettings addPairRequest={addPairRequest} />
      )}
    </Dialog>
  );
}

// ---- shared inline-edit building blocks -------------------------------------------

/**
 * Free-text setting: shown as a static value with a pencil icon; clicking the
 * pencil turns it into an input that saves when it loses focus (Enter commits,
 * Escape cancels).
 */
function EditableText({
  label,
  value,
  saving = false,
  mono = false,
  onSave,
}: {
  label: string;
  value: string;
  saving?: boolean;
  mono?: boolean;
  onSave: (value: string) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const cancelled = useRef(false);

  return (
    <div className="flex items-center gap-2">
      <span className="w-44 shrink-0 text-text-secondary">{label}</span>
      {editing ? (
        <input
          autoFocus
          value={draft}
          aria-label={label}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape") {
              cancelled.current = true;
              e.currentTarget.blur();
            }
          }}
          onBlur={() => {
            setEditing(false);
            if (!cancelled.current && draft !== value) onSave(draft);
            cancelled.current = false;
          }}
          className="min-w-0 flex-1 border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none"
        />
      ) : (
        <>
          <span className={`min-w-0 flex-1 truncate ${mono ? "font-mono" : ""}`}>
            {value || "—"}
          </span>
          <IconButton
            title={t("common.edit")}
            disabled={saving}
            onClick={() => {
              setDraft(value);
              setEditing(true);
            }}
          >
            <PencilIcon
              width={14}
              height={14}
              className={saving ? "animate-pulse" : ""}
            />
          </IconButton>
        </>
      )}
    </div>
  );
}

/**
 * Enumerated setting: shown as a static value with a pencil icon; clicking the
 * pencil turns it into a select that saves on change and closes on blur. The
 * current device value stays selectable even when it is not a known option.
 */
function EditableSelect({
  label,
  value,
  options,
  saving = false,
  onSave,
}: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  saving?: boolean;
  onSave: (value: string) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const known = options.find((o) => o.value === value);

  return (
    <div className="flex items-center gap-2">
      <span className="w-44 shrink-0 text-text-secondary">{label}</span>
      {editing ? (
        <select
          autoFocus
          value={value}
          aria-label={label}
          onChange={(e) => {
            setEditing(false);
            if (e.target.value !== value) onSave(e.target.value);
          }}
          onBlur={() => setEditing(false)}
          onKeyDown={(e) => e.key === "Escape" && e.currentTarget.blur()}
          className="min-w-0 flex-1 border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none"
        >
          {!known && <option value={value}>{value || "—"}</option>}
          {options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      ) : (
        <>
          <span className="min-w-0 flex-1 truncate">
            {known?.label ?? (value || "—")}
          </span>
          <IconButton
            title={t("common.edit")}
            disabled={saving}
            onClick={() => setEditing(true)}
          >
            <PencilIcon
              width={14}
              height={14}
              className={saving ? "animate-pulse" : ""}
            />
          </IconButton>
        </>
      )}
    </div>
  );
}

/**
 * iOS-style on/off switch in the app's monochrome, sharp-cornered design
 * language: a rectangular track with a sliding rectangular knob.
 */
function RectSwitch({
  checked,
  disabled = false,
  label,
  onChange,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative h-4.5 w-9 shrink-0 border transition-colors disabled:opacity-40 ${
        checked ? "border-text bg-text" : "border-border bg-bg"
      }`}
    >
      <span
        className={`absolute top-0.5 h-3 w-4 transition-all ${
          checked ? "start-4 bg-bg" : "start-0.5 bg-text-secondary"
        }`}
      />
    </button>
  );
}

// ---- application tab --------------------------------------------------------------

function ApplicationTab() {
  const t = useT();
  const settings = useApp((s) => s.settings);
  const setTheme = useApp((s) => s.setTheme);
  const setLanguage = useApp((s) => s.setLanguage);
  const setUpdateCheck = useApp((s) => s.setUpdateCheck);
  const checkForUpdate = useApp((s) => s.checkForUpdate);
  const update = useApp((s) => s.update);
  const updateChecking = useApp((s) => s.updateChecking);

  return (
    <div className="flex flex-col gap-3 text-[13px]">
      <EditableSelect
        label={t("settings.theme")}
        value={settings?.theme ?? "system"}
        options={[
          { value: "system", label: t("settings.themeSystem") },
          { value: "light", label: t("settings.themeLight") },
          { value: "dark", label: t("settings.themeDark") },
        ]}
        onSave={(v) => void setTheme(v)}
      />
      <EditableSelect
        label={t("settings.language")}
        value={settings?.language ?? "system"}
        options={[
          { value: "system", label: t("settings.languageSystem") },
          ...LOCALES.map((loc) => ({ value: loc, label: LOCALE_LABEL[loc] })),
        ]}
        onSave={(v) => void setLanguage(v)}
      />

      {/* Update check (FR-APP-5): notify-only, user-disableable (NFR-SEC-4). */}
      <div className="mt-1 border-t border-border pt-3">
        <span className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
          {t("settings.updates")}
        </span>
        <label className="mt-2 flex items-center gap-2">
          <input
            type="checkbox"
            checked={settings?.update_check ?? true}
            onChange={(e) => void setUpdateCheck(e.target.checked)}
          />
          {t("settings.updateCheckAuto")}
        </label>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <button
            onClick={() => void checkForUpdate()}
            disabled={updateChecking}
            className="border border-border px-2.5 py-1 hover:border-text disabled:opacity-50"
          >
            {updateChecking
              ? t("settings.updateChecking")
              : t("settings.updateCheckNow")}
          </button>
          {update &&
            (update.update_available ? (
              <span>
                {t("settings.updateAvailable", {
                  version: update.latest ?? "?",
                  current: update.current,
                })}
              </span>
            ) : (
              <span className="text-text-secondary">
                {t("settings.updateUpToDate", { version: update.current })}
              </span>
            ))}
        </div>
        {update?.update_available && (
          <button
            onClick={() => void openUrl(update.url)}
            className="mt-2 border border-accent bg-accent px-2.5 py-1 text-accent-foreground"
          >
            {t("settings.updateOpenPage")}
          </button>
        )}
      </div>
    </div>
  );
}

// ---- device tab (FR-SET-1/2/3) ---------------------------------------------------

/** Shared look of the collapsible section headers in the device tab. */
const SECTION_SUMMARY =
  "cursor-pointer text-xs font-semibold uppercase tracking-wide text-text-secondary hover:text-text";

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

/** Sensible standby timeouts in minutes; anything beyond 120 has no use case. */
const STANDBY_TIMEOUT_OPTIONS = ["5", "10", "20", "30", "60", "120"];

function DeviceTab() {
  const t = useT();
  const connection = useApp((s) => s.connection);
  const toast = useApp((s) => s.toast);
  const [status, setStatus] = useState<DeviceStatus | null>(null);
  const [configs, setConfigs] = useState<ConfigEntry[] | null>(null);
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const [settingClock, setSettingClock] = useState(false);
  // The host's clock, ticking (the sync button copies it to the device).
  const [now, setNow] = useState(() => new Date());

  const connected = connection.state === "connected";

  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(id);
  }, []);

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
  const advanced = (configs ?? []).filter((c) => !DEDICATED_KEYS.has(c.key));

  const saveValue = async (key: string, value: string) => {
    setSavingKey(key);
    try {
      await ipc.setDeviceConfig(key, value);
      setConfigs(
        (prev) => prev?.map((c) => (c.key === key ? { ...c, value } : c)) ?? prev,
      );
      toast(t("device.configSaved", { key }));
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSavingKey(null);
    }
  };

  const setClock = async () => {
    setSettingClock(true);
    try {
      await ipc.setDeviceClock();
      toast(t("device.clockSet"));
      refreshConfigs(); // the device time changed
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSettingClock(false);
    }
  };

  const configText = (key: string, label: string) => (
    <EditableText
      key={key}
      label={label}
      value={byKey.get(key) ?? ""}
      saving={savingKey === key}
      onSave={(v) => void saveValue(key, v)}
    />
  );

  const configSelect = (
    key: string,
    label: string,
    options: { value: string; label: string }[],
  ) => (
    <EditableSelect
      key={key}
      label={label}
      value={byKey.get(key) ?? ""}
      options={options}
      saving={savingKey === key}
      onSave={(v) => void saveValue(key, v)}
    />
  );

  return (
    <div className="flex flex-col gap-4 text-[13px]">
      {(status === null || configs === null) && (
        <p className="text-text-secondary">{t("device.loadingStatus")}</p>
      )}

      {/* Basic: owner + hardware facts (FR-SET-1/2). One container with a
          uniform row gap; the dl uses the same 11rem (= w-44) label column
          and a 22px row rhythm matching the icon-button rows around it. */}
      <div className="flex flex-col gap-1.5">
        {byKey.has("owner") && configText("owner", t("device.owner"))}
        {status && (
          <dl className="grid grid-cols-[11rem_1fr] gap-x-2 gap-y-1.5 leading-[22px]">
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
          configSelect(
            "timeout_to_standby",
            t("device.standbyTimeout"),
            STANDBY_TIMEOUT_OPTIONS.map((m) => ({ value: m, label: m })),
          )}
      </div>

      {/* Date & time (FR-SET-3), collapsed by default — not needed day to day */}
      {configs !== null && (
        <section className="border-t border-border pt-3">
          <details>
            <summary className={SECTION_SUMMARY}>{t("device.dateTime")}</summary>
            <div className="mt-2 flex flex-col gap-1.5">
              <div className="flex items-center gap-2">
                <span className="w-44 shrink-0 text-text-secondary">
                  {t("device.currentTime")}
                </span>
                <span className="min-w-0 flex-1 tabular-nums">
                  {now.toLocaleString(currentLocale())}
                </span>
                <IconButton
                  title={t("device.setClock")}
                  disabled={settingClock}
                  onClick={() => void setClock()}
                >
                  <ClockToDeviceIcon
                    width={16}
                    height={16}
                    className={settingClock ? "animate-pulse" : ""}
                  />
                </IconButton>
              </div>
              {timezones.length > 0
                ? configSelect(
                    "timezone",
                    t("device.timezone"),
                    timezones.map((z) => ({ value: z, label: z })),
                  )
                : byKey.has("timezone") && configText("timezone", t("device.timezone"))}
              {configSelect(
                "date_format",
                t("device.dateFormat"),
                DATE_FORMAT_OPTIONS.map((f) => ({ value: f, label: f })),
              )}
              {configSelect("time_format", t("device.timeFormat"), [
                { value: "12hour", label: t("device.timeFormat12") },
                { value: "24hour", label: t("device.timeFormat24") },
              ])}
            </div>
          </details>
        </section>
      )}

      <WifiSection />

      {/* Advanced: every remaining key, last (FR-SET-2) */}
      {advanced.length > 0 && (
        <section className="border-t border-border pt-3">
          <details>
            <summary className={SECTION_SUMMARY}>
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
                  saving={savingKey === c.key}
                  onSave={(v) => void saveValue(c.key, v)}
                />
              ))}
            </div>
          </details>
        </section>
      )}
    </div>
  );
}

/** One advanced key/value row with the same pencil → edit → save-on-blur flow. */
function AdvancedRow({
  entry,
  saving,
  onSave,
}: {
  entry: ConfigEntry;
  saving: boolean;
  onSave: (value: string) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const cancelled = useRef(false);

  return (
    <>
      <span className="truncate font-mono" title={entry.key}>
        {entry.key}
      </span>
      {editing ? (
        <>
          <input
            autoFocus
            value={draft}
            aria-label={entry.key}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                cancelled.current = true;
                e.currentTarget.blur();
              }
            }}
            onBlur={() => {
              setEditing(false);
              if (!cancelled.current && draft !== entry.value) onSave(draft);
              cancelled.current = false;
            }}
            className="min-w-0 border border-border bg-bg px-1.5 py-0.5 font-mono focus:border-text focus:outline-none"
          />
          <span />
        </>
      ) : (
        <>
          <span className="truncate font-mono">{entry.value || "—"}</span>
          <IconButton
            title={t("common.edit")}
            disabled={saving}
            onClick={() => {
              setDraft(entry.value);
              setEditing(true);
            }}
          >
            <PencilIcon
              width={12}
              height={12}
              className={saving ? "animate-pulse" : ""}
            />
          </IconButton>
        </>
      )}
    </>
  );
}

// ---- Wi-Fi (FR-SET-4) -----------------------------------------------------------

/** Rough security classification of a network for the lock icon. */
function securityLevel(security: string): "secured" | "weak" | "open" {
  const s = security.toLowerCase();
  if (!s || /none|open|nonsec/.test(s)) return "open";
  if (/wep|wpa[^23]|wpa$/.test(s)) return "weak";
  return "secured"; // WPA2/WPA3/PSK/EAP…
}

/** Small lock icon indicating how a network is secured, with a tooltip. */
function SecurityBadge({ security }: { security: string }) {
  const t = useT();
  const level = securityLevel(security);
  const [Icon, title] =
    level === "open"
      ? ([LockOpenIcon, t("wifi.unsecured")] as const)
      : level === "weak"
        ? ([LockWeakIcon, t("wifi.weaklySecured", { security })] as const)
        : ([LockIcon, t("wifi.secured", { security })] as const);
  return (
    <span title={title} className="shrink-0 text-text-secondary">
      <Icon width={13} height={13} aria-label={title} />
    </span>
  );
}

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

  // One combined list: stored networks first, then scan results that are
  // not stored yet (visible-only rows are dimmed and get an add action).
  const networks: { ap: AccessPoint; isStored: boolean }[] = (stored ?? []).map((ap) => ({
    ap,
    isStored: true,
  }));
  {
    const knownSsids = new Set(networks.map((n) => n.ap.ssid));
    for (const ap of visible ?? []) {
      if (!knownSsids.has(ap.ssid)) {
        networks.push({ ap, isStored: false });
        knownSsids.add(ap.ssid);
      }
    }
  }

  return (
    <section className="border-t border-border pt-3 text-[13px]">
      <details>
        <summary className={SECTION_SUMMARY}>{t("wifi.title")}</summary>

        {/* pe-1 lines the switch up with the pencil icons (their p-1 inset). */}
        <div className="mt-2 flex items-center gap-2 pe-1">
          <span className="w-44 shrink-0 text-text-secondary">{t("wifi.radio")}</span>
          <span className="min-w-0 flex-1" />
          <RectSwitch
            checked={enabled === true}
            disabled={enabled == null}
            label={t("wifi.radio")}
            onChange={(on) => void toggleRadio(on)}
          />
        </div>

        <div className="mt-3 flex items-center gap-1">
          <span className="text-text-secondary">{t("wifi.networks")}</span>
          <span className="min-w-0 flex-1" />
          <IconButton
            title={scanning ? t("wifi.scanning") : t("wifi.scan")}
            disabled={scanning}
            onClick={() => void scan()}
          >
            <SearchIcon
              width={14}
              height={14}
              className={scanning ? "animate-pulse" : ""}
            />
          </IconButton>
          <IconButton title={t("wifi.joinTitle")} onClick={() => setJoining("manual")}>
            <PlusIcon width={14} height={14} />
          </IconButton>
        </div>

        {networks.length === 0 ? (
          <p className="mt-1 text-xs text-text-secondary">
            {visible !== null ? t("wifi.noneVisible") : t("wifi.noStored")}
          </p>
        ) : (
          <ul className="mt-1">
            {networks.map(({ ap, isStored }, i) => (
              <li
                key={`${ap.ssid}/${ap.security}/${i}`}
                className={`flex items-center gap-2 py-0.5 ${
                  isStored ? "" : "text-text-secondary"
                }`}
              >
                <SecurityBadge security={ap.security} />
                <span className="min-w-0 flex-1 truncate font-mono" title={ap.ssid}>
                  {ap.ssid}
                </span>
                {isStored ? (
                  <IconButton title={t("common.remove")} onClick={() => void remove(ap)}>
                    <TrashIcon width={14} height={14} />
                  </IconButton>
                ) : (
                  <IconButton title={t("wifi.join")} onClick={() => setJoining(ap)}>
                    <PlusIcon width={14} height={14} />
                  </IconButton>
                )}
              </li>
            ))}
          </ul>
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
      </details>
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
