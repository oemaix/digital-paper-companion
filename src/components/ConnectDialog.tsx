import { useCallback, useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import Dialog from "./Dialog";
import { RefreshIcon, TabletIcon } from "./icons";
import { useApp } from "../lib/store";
import {
  ipc,
  errorMessage,
  BLUETOOTH_PAN_ADDRESS,
  type Discovered,
  type DeviceInfo,
  type ImportCandidate,
  type KnownDevice,
  type UsbCandidate,
} from "../lib/ipc";
import { useT, t as tt } from "../lib/i18n";

/**
 * Connect / pairing dialog (docs/05 §3.1–3.2; FR-CONN-1…7, FR-REG-2/3/6).
 *
 * Main panel: a single device list — paired devices merged with live mDNS
 * results by **serial number** — plus manual address entry and shortcuts
 * for USB, Bluetooth PAN and credential import. Sub-panels handle the USB
 * mode switch and the Sony/dptrp1 credential import.
 */

type Status = "connected" | "paired" | "new";
type Panel = "list" | "usb" | "import";

interface DeviceRow {
  key: string;
  serial: string | null;
  name: string;
  model: string | null;
  /** Live address if seen on the network, else the last known address. */
  address: string | null;
  status: Status;
  online: boolean;
}

export default function ConnectDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const connection = useApp((s) => s.connection);
  const refreshGlobalDevices = useApp((s) => s.refreshKnownDevices);
  const [panel, setPanel] = useState<Panel>("list");
  const [known, setKnown] = useState<KnownDevice[]>([]);
  const [scanned, setScanned] = useState<Discovered[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [manual, setManual] = useState("");
  const [bluetoothHint, setBluetoothHint] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [pairing, setPairing] = useState<DeviceInfo | null>(null);
  const [pin, setPin] = useState("");

  const refreshKnown = useCallback(() => {
    ipc
      .knownDevices()
      .then(setKnown)
      .catch(() => setKnown([]));
    void refreshGlobalDevices();
  }, [refreshGlobalDevices]);

  const discover = useCallback(async () => {
    setDiscovering(true);
    try {
      setScanned(await ipc.discoverDevices(5));
    } catch {
      // mDNS may be unavailable (firewall); manual entry still works.
    } finally {
      setDiscovering(false);
    }
  }, []);

  useEffect(() => {
    refreshKnown();
    void discover();
  }, [refreshKnown, discover]);

  // Merge paired devices with live scan results, keyed by serial (FR-CONN-7).
  const rows = useMemo<DeviceRow[]>(() => {
    const map = new Map<string, DeviceRow>();
    const connectedSerial = connection.state === "connected" ? connection.serial : null;

    for (const d of known) {
      map.set(d.serial, {
        key: d.serial,
        serial: d.serial,
        name: d.name,
        model: d.model ?? null,
        address: d.last_address ?? null,
        status: d.serial === connectedSerial ? "connected" : "paired",
        online: false,
      });
    }

    for (const s of scanned) {
      if (s.serial) {
        const existing = map.get(s.serial);
        if (existing) {
          existing.online = true;
          existing.address = s.address; // live address wins
          if (s.connected) existing.status = "connected";
        } else {
          map.set(s.serial, {
            key: s.serial,
            serial: s.serial,
            name: s.name,
            model: s.model,
            address: s.address,
            status: s.connected ? "connected" : s.paired ? "paired" : "new",
            online: true,
          });
        }
      } else {
        // Unidentified (probe failed): key by address, treat as new.
        map.set(`addr:${s.address}`, {
          key: `addr:${s.address}`,
          serial: null,
          name: s.name || s.address,
          model: s.model,
          address: s.address,
          status: "new",
          online: true,
        });
      }
    }

    const order: Record<Status, number> = { connected: 0, paired: 1, new: 2 };
    return [...map.values()].sort(
      (a, b) => order[a.status] - order[b.status] || a.name.localeCompare(b.name),
    );
  }, [known, scanned, connection]);

  const close = () => {
    if (pairing) void ipc.cancelPairing();
    onClose();
  };

  const connect = async (serial: string, address: string | null) => {
    setBusy(t("connect.connecting"));
    try {
      await ipc.connectKnownDevice(serial, address ?? undefined);
      toast(t("connect.connected"));
      onClose();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(null);
    }
  };

  /** Probe an address, then connect if already paired or start pairing. */
  const useAddress = async (address: string) => {
    setBusy(t("connect.contacting", { address }));
    try {
      const info = await ipc.probeDevice(address);
      const existing = known.find((d) => d.serial === info.serial_number);
      if (existing) {
        setBusy(t("connect.connectingTo", { name: existing.name }));
        await ipc.connectKnownDevice(existing.serial, address);
        toast(t("connect.connectedTo", { name: existing.name }));
        onClose();
      } else {
        setBusy(t("connect.startingPairing"));
        const pairInfo = await ipc.startPairing(address);
        setPairing(pairInfo);
      }
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(null);
    }
  };

  const forget = async (serial: string, name: string) => {
    if (!window.confirm(t("connect.forgetConfirm", { name }))) return;
    try {
      await ipc.forgetDevice(serial);
      refreshKnown();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const submitPin = async () => {
    setBusy(t("pairing.completing"));
    try {
      await ipc.submitPairingPin(pin);
      toast(t("pairing.success"));
      setPairing(null);
      onClose();
    } catch (err) {
      toast(errorMessage(err), "error");
      // The handshake is consumed; the user must restart pairing.
      setPairing(null);
      setPin("");
    } finally {
      setBusy(null);
    }
  };

  // ---- PIN entry ------------------------------------------------------------
  if (pairing) {
    return (
      <Dialog title={t("pairing.title")} onClose={close}>
        <p className="mb-1 font-medium">
          {pairing.model_name ?? "Digital Paper"} · {pairing.serial_number}
        </p>
        <p className="mb-3 text-text-secondary">{t("pairing.instructions")}</p>
        <div className="flex gap-2">
          <input
            autoFocus
            value={pin}
            onChange={(e) => setPin(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && pin.trim() && submitPin()}
            placeholder={t("pairing.placeholder")}
            aria-label={t("pairing.placeholder")}
            className="min-w-0 flex-1 border border-border bg-bg px-2 py-1.5 text-[15px] tracking-widest placeholder:text-text-secondary focus:border-text focus:outline-none"
          />
          <button
            disabled={pin.trim() === "" || busy !== null}
            onClick={submitPin}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
          >
            {t("connect.pair")}
          </button>
        </div>
        {busy && <p className="mt-3 text-text-secondary">{busy}</p>}
      </Dialog>
    );
  }

  // ---- sub-panels -------------------------------------------------------------
  if (panel === "usb") {
    return (
      <UsbPanel
        onBack={() => setPanel("list")}
        onClose={close}
        onSwitched={() => void discover()}
      />
    );
  }
  if (panel === "import") {
    return (
      <ImportPanel
        onBack={() => setPanel("list")}
        onDone={() => {
          refreshKnown();
          onClose();
        }}
      />
    );
  }

  // ---- main panel: unified device list ------------------------------------------
  return (
    <Dialog title={t("connect.title")} onClose={close}>
      <div className="mb-1.5 flex items-center justify-between">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
          {t("connect.devices")}
        </h3>
        <button
          onClick={() => void discover()}
          disabled={discovering}
          title={t("connect.searchAgain")}
          className="flex items-center gap-1 px-1 py-0.5 text-xs text-text-secondary hover:text-text disabled:opacity-50"
        >
          <RefreshIcon
            width={12}
            height={12}
            className={discovering ? "animate-spin" : ""}
          />
          {discovering ? t("connect.searching") : t("connect.search")}
        </button>
      </div>

      {rows.length === 0 ? (
        <p className="mb-4 border border-border px-2 py-2 text-xs text-text-secondary">
          {discovering ? t("connect.searching") : t("connect.noDevices")}
        </p>
      ) : (
        <ul className="mb-4 border border-border">
          {rows.map((row) => (
            <li
              key={row.key}
              className="flex items-center gap-2 border-b border-border px-2 py-1.5 last:border-b-0"
            >
              <TabletIcon className="shrink-0 text-text-secondary" />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="truncate">{row.name}</span>
                  {row.status === "connected" && (
                    <span className="shrink-0 border border-text px-1 text-[10px] uppercase tracking-wide">
                      {t("connect.badgeConnected")}
                    </span>
                  )}
                  {row.status === "paired" && (
                    <span className="shrink-0 text-[10px] uppercase tracking-wide text-text-secondary">
                      {t("connect.badgePaired")}
                    </span>
                  )}
                </div>
                <div className="truncate text-xs text-text-secondary">
                  {row.serial ?? t("connect.unknownSerial")}
                  {row.address ? ` · ${row.address}` : ""}
                  {!row.online && row.status !== "connected"
                    ? ` · ${t("connect.offline")}`
                    : ""}
                </div>
              </div>

              {row.status !== "connected" && row.serial && (
                <button
                  onClick={() => forget(row.serial!, row.name)}
                  className="px-2 py-1 text-xs text-text-secondary hover:text-text"
                >
                  {t("connect.forget")}
                </button>
              )}

              {row.status === "connected" ? (
                <button
                  onClick={() => void ipc.disconnectDevice().then(onClose)}
                  className="border border-border px-2.5 py-1 text-xs hover:border-text"
                >
                  {t("connect.disconnect")}
                </button>
              ) : row.status === "paired" ? (
                <button
                  disabled={busy !== null || (!row.online && !row.address)}
                  onClick={() => void connect(row.serial!, row.address)}
                  className="border border-accent bg-accent px-2.5 py-1 text-xs text-accent-foreground disabled:opacity-50"
                >
                  {t("connect.connect")}
                </button>
              ) : (
                <button
                  disabled={busy !== null || !row.address}
                  onClick={() => row.address && void useAddress(row.address)}
                  className="border border-accent bg-accent px-2.5 py-1 text-xs text-accent-foreground disabled:opacity-50"
                >
                  {t("connect.pair")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}

      <section>
        <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          {t("connect.manualAddress")}
        </h3>
        <div className="flex gap-2">
          <input
            value={manual}
            onChange={(e) => setManual(e.target.value)}
            onKeyDown={(e) =>
              e.key === "Enter" && manual.trim() && void useAddress(manual.trim())
            }
            placeholder={t("connect.manualPlaceholder")}
            aria-label={t("connect.manualAddress")}
            className="min-w-0 flex-1 border border-border bg-bg px-2 py-1.5 text-[13px] placeholder:text-text-secondary focus:border-text focus:outline-none"
          />
          <button
            disabled={manual.trim() === "" || busy !== null}
            onClick={() => void useAddress(manual.trim())}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
          >
            {t("connect.connect")}
          </button>
        </div>
        {bluetoothHint && (
          <p className="mt-2 border border-border px-2 py-1.5 text-xs text-text-secondary">
            {t("connect.bluetoothHint", { address: BLUETOOTH_PAN_ADDRESS })}
          </p>
        )}
      </section>

      {/* Other connection paths (FR-CONN-3/4, FR-REG-6) */}
      <div className="mt-4 flex flex-col items-start gap-1 border-t border-border pt-3 text-xs">
        <button
          onClick={() => setPanel("usb")}
          className="text-text-secondary underline-offset-2 hover:text-text hover:underline"
        >
          {t("connect.usbLink")}
        </button>
        <button
          onClick={() => {
            setManual(BLUETOOTH_PAN_ADDRESS);
            setBluetoothHint(true);
          }}
          className="text-text-secondary underline-offset-2 hover:text-text hover:underline"
        >
          {t("connect.bluetoothLink")}
        </button>
        <button
          onClick={() => setPanel("import")}
          className="text-text-secondary underline-offset-2 hover:text-text hover:underline"
        >
          {t("connect.importLink")}
        </button>
      </div>

      {busy && <p className="mt-3 text-text-secondary">{busy}</p>}
    </Dialog>
  );
}

// ---- USB panel (FR-CONN-4) --------------------------------------------------

function UsbPanel({
  onBack,
  onClose,
  onSwitched,
}: {
  onBack: () => void;
  onClose: () => void;
  onSwitched: () => void;
}) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [ports, setPorts] = useState<UsbCandidate[] | null>(null);
  const [mode, setMode] = useState<"auto" | "rndis" | "cdc-ecm">("auto");
  const [switching, setSwitching] = useState<string | null>(null);
  const [switchedMode, setSwitchedMode] = useState<string | null>(null);

  const scan = useCallback(() => {
    setPorts(null);
    ipc
      .usbPorts()
      .then(setPorts)
      .catch((err) => {
        toast(errorMessage(err), "error");
        setPorts([]);
      });
  }, [toast]);

  useEffect(() => {
    scan();
  }, [scan]);

  const switchPort = async (port: string) => {
    setSwitching(port);
    try {
      const applied = await ipc.usbSwitchMode(port, mode === "auto" ? undefined : mode);
      setSwitchedMode(applied === "rndis" ? "RNDIS" : "CDC/ECM");
      onSwitched();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSwitching(null);
    }
  };

  return (
    <Dialog title={t("usb.title")} onClose={onClose}>
      <p className="mb-3 text-text-secondary">{t("usb.intro")}</p>

      {switchedMode ? (
        <p className="mb-3 border border-border px-2 py-2 text-[13px]">
          {t("usb.switched", { mode: switchedMode })}
        </p>
      ) : (
        <>
          <label className="mb-3 flex items-center justify-between gap-4 text-[13px]">
            <span className="text-text-secondary">{t("usb.mode")}</span>
            <select
              value={mode}
              onChange={(e) => setMode(e.target.value as typeof mode)}
              className="border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none"
            >
              <option value="auto">{t("usb.modeAuto")}</option>
              <option value="rndis">{t("usb.modeRndis")}</option>
              <option value="cdc-ecm">{t("usb.modeEcm")}</option>
            </select>
          </label>

          {ports === null ? (
            <p className="text-xs text-text-secondary">{t("usb.searching")}</p>
          ) : ports.length === 0 ? (
            <p className="border border-border px-2 py-2 text-xs text-text-secondary">
              {t("usb.noPorts")}
            </p>
          ) : (
            <ul className="border border-border">
              {ports.map((p) => (
                <li
                  key={p.port}
                  className="flex items-center gap-2 border-b border-border px-2 py-1.5 text-[13px] last:border-b-0"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <span className="truncate font-mono">{p.port}</span>
                      {p.likely_digital_paper && (
                        <span className="shrink-0 border border-text px-1 text-[10px] uppercase tracking-wide">
                          {t("usb.likelyDevice")}
                        </span>
                      )}
                    </div>
                    <div className="truncate text-xs text-text-secondary">{p.label}</div>
                  </div>
                  <button
                    disabled={switching !== null}
                    onClick={() => void switchPort(p.port)}
                    className="border border-accent bg-accent px-2.5 py-1 text-xs text-accent-foreground disabled:opacity-50"
                  >
                    {switching === p.port ? t("usb.switching") : t("usb.switch")}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      <div className="mt-4 flex gap-2 border-t border-border pt-3">
        <button
          onClick={onBack}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("common.back")}
        </button>
        <button
          onClick={scan}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("usb.rescan")}
        </button>
      </div>
    </Dialog>
  );
}

// ---- credential import panel (FR-REG-6) ------------------------------------------

function originLabel(origin: string): string {
  if (origin === "sony") return tt("import.origin.sony");
  if (origin === "dptrp1") return tt("import.origin.dptrp1");
  return origin;
}

function ImportPanel({ onBack, onDone }: { onBack: () => void; onDone: () => void }) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [candidates, setCandidates] = useState<ImportCandidate[] | null>(null);
  const [deviceidPath, setDeviceidPath] = useState("");
  const [privatekeyPath, setPrivatekeyPath] = useState("");
  const [address, setAddress] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    ipc
      .importCandidates()
      .then((found) => {
        setCandidates(found);
        if (found.length > 0) {
          setDeviceidPath(found[0].deviceid_path);
          setPrivatekeyPath(found[0].privatekey_path);
        }
      })
      .catch(() => setCandidates([]));
  }, []);

  const pick = async (which: "deviceid" | "privatekey") => {
    const picked = await openFileDialog({
      multiple: false,
      filters: [{ name: "dat", extensions: ["dat", "*"] }],
    });
    if (typeof picked !== "string") return;
    if (which === "deviceid") setDeviceidPath(picked);
    else setPrivatekeyPath(picked);
  };

  const doImport = async () => {
    setBusy(true);
    try {
      const payload = await ipc.importCredentials(
        deviceidPath,
        privatekeyPath,
        address.trim(),
      );
      toast(t("import.success", { name: payload.name ?? "Digital Paper" }));
      onDone();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  const valid = deviceidPath !== "" && privatekeyPath !== "" && address.trim() !== "";

  const fileButton =
    "border border-border px-2 py-1 text-xs hover:border-text disabled:opacity-40";

  return (
    <Dialog title={t("import.title")} onClose={onDone}>
      <p className="mb-3 text-text-secondary">{t("import.intro")}</p>

      {candidates === null ? (
        <p className="text-xs text-text-secondary">{t("common.loading")}</p>
      ) : candidates.length > 0 ? (
        <fieldset className="mb-3">
          <legend className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("import.found")}
          </legend>
          <ul className="border border-border">
            {candidates.map((c) => (
              <li
                key={c.deviceid_path}
                className="border-b border-border last:border-b-0"
              >
                <label className="flex items-start gap-2 px-2 py-1.5 text-[13px]">
                  <input
                    type="radio"
                    name="import-candidate"
                    checked={deviceidPath === c.deviceid_path}
                    onChange={() => {
                      setDeviceidPath(c.deviceid_path);
                      setPrivatekeyPath(c.privatekey_path);
                    }}
                    className="mt-0.5"
                  />
                  <span className="min-w-0">
                    <span className="block">{originLabel(c.origin)}</span>
                    <span
                      className="block truncate text-xs text-text-secondary"
                      title={c.deviceid_path}
                    >
                      {c.deviceid_path}
                    </span>
                  </span>
                </label>
              </li>
            ))}
          </ul>
        </fieldset>
      ) : (
        <p className="mb-3 border border-border px-2 py-2 text-xs text-text-secondary">
          {t("import.notFound")}
        </p>
      )}

      <div className="mb-3 flex flex-col gap-1.5 text-xs">
        <div className="flex items-center gap-2">
          <button onClick={() => void pick("deviceid")} className={fileButton}>
            {t("import.pickDeviceId")}
          </button>
          <span className="min-w-0 truncate text-text-secondary" title={deviceidPath}>
            {deviceidPath || "—"}
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button onClick={() => void pick("privatekey")} className={fileButton}>
            {t("import.pickPrivateKey")}
          </button>
          <span className="min-w-0 truncate text-text-secondary" title={privatekeyPath}>
            {privatekeyPath || "—"}
          </span>
        </div>
      </div>

      <label className="mb-1 flex flex-col gap-1 text-[13px]">
        <span className="text-text-secondary">{t("import.address")}</span>
        <input
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && valid && !busy && void doImport()}
          placeholder={t("connect.manualPlaceholder")}
          className="border border-border bg-bg px-2 py-1.5 placeholder:text-text-secondary focus:border-text focus:outline-none"
        />
      </label>
      <p className="mb-3 text-xs text-text-secondary">{t("import.addressHint")}</p>

      <div className="flex gap-2 border-t border-border pt-3">
        <button
          onClick={onBack}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("common.back")}
        </button>
        <button
          disabled={!valid || busy}
          onClick={() => void doImport()}
          className="ml-auto border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
        >
          {busy ? t("import.importing") : t("import.import")}
        </button>
      </div>
      <p className="mt-2 text-xs text-text-secondary">{t("import.deleteHint")}</p>
    </Dialog>
  );
}
