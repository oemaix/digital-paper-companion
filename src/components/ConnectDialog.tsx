import { useCallback, useEffect, useMemo, useState } from "react";
import Dialog from "./Dialog";
import { RefreshIcon, TabletIcon } from "./icons";
import { useApp } from "../lib/store";
import {
  ipc,
  errorMessage,
  type Discovered,
  type DeviceInfo,
  type KnownDevice,
} from "../lib/ipc";

/**
 * Connect / pairing dialog (docs/05 §3.1–3.2; FR-CONN-1/2/7, FR-REG-2/3).
 *
 * One dialog, two steps:
 * 1. a single device list — paired devices merged with live mDNS results by
 *    **serial number** (device identity, not address), plus a manual address
 * 2. PIN entry — after the handshake started, the device shows a PIN
 *
 * Because each device is keyed by serial, a device never appears twice and
 * the currently connected device is shown as such rather than offered for
 * pairing again — even if it is reachable at a new address.
 */

type Status = "connected" | "paired" | "new";

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
  const toast = useApp((s) => s.toast);
  const connection = useApp((s) => s.connection);
  const [known, setKnown] = useState<KnownDevice[]>([]);
  const [scanned, setScanned] = useState<Discovered[]>([]);
  const [discovering, setDiscovering] = useState(false);
  const [manual, setManual] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [pairing, setPairing] = useState<DeviceInfo | null>(null);
  const [pin, setPin] = useState("");

  const refreshKnown = useCallback(() => {
    ipc.knownDevices().then(setKnown).catch(() => setKnown([]));
  }, []);

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
    const connectedSerial =
      connection.state === "connected" ? connection.serial : null;

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
    setBusy("Connecting…");
    try {
      await ipc.connectKnownDevice(serial, address ?? undefined);
      toast("Connected");
      onClose();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(null);
    }
  };

  /** Probe an address, then connect if already paired or start pairing. */
  const useAddress = async (address: string) => {
    setBusy(`Contacting ${address}…`);
    try {
      const info = await ipc.probeDevice(address);
      const existing = known.find((d) => d.serial === info.serial_number);
      if (existing) {
        setBusy(`Connecting to ${existing.name}…`);
        await ipc.connectKnownDevice(existing.serial, address);
        toast(`Connected to ${existing.name}`);
        onClose();
      } else {
        setBusy("Starting pairing…");
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
    if (!window.confirm(`Forget "${name}"? Credentials will be deleted.`)) return;
    try {
      await ipc.forgetDevice(serial);
      refreshKnown();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const submitPin = async () => {
    setBusy("Completing pairing…");
    try {
      await ipc.submitPairingPin(pin);
      toast("Device paired and connected");
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

  // ---- step 2: PIN entry ----------------------------------------------------
  if (pairing) {
    return (
      <Dialog title="Pair device" onClose={close}>
        <p className="mb-1 font-medium">
          {pairing.model_name ?? "Digital Paper"} · {pairing.serial_number}
        </p>
        <p className="mb-3 text-text-secondary">
          The device is now showing a pairing code on its screen. Enter it
          below to finish pairing.
        </p>
        <div className="flex gap-2">
          <input
            autoFocus
            value={pin}
            onChange={(e) => setPin(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && pin.trim() && submitPin()}
            placeholder="Pairing code"
            className="min-w-0 flex-1 border border-border bg-bg px-2 py-1.5 text-[15px] tracking-widest placeholder:text-text-secondary focus:border-text focus:outline-none"
          />
          <button
            disabled={pin.trim() === "" || busy !== null}
            onClick={submitPin}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
          >
            Pair
          </button>
        </div>
        {busy && <p className="mt-3 text-text-secondary">{busy}</p>}
      </Dialog>
    );
  }

  // ---- step 1: unified device list ------------------------------------------
  return (
    <Dialog title="Connect to device" onClose={close}>
      <div className="mb-1.5 flex items-center justify-between">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Devices
        </h3>
        <button
          onClick={() => void discover()}
          disabled={discovering}
          title="Search the network again"
          className="flex items-center gap-1 px-1 py-0.5 text-xs text-text-secondary hover:text-text disabled:opacity-50"
        >
          <RefreshIcon width={12} height={12} className={discovering ? "animate-spin" : ""} />
          {discovering ? "Searching…" : "Search"}
        </button>
      </div>

      {rows.length === 0 ? (
        <p className="mb-4 border border-border px-2 py-2 text-xs text-text-secondary">
          {discovering
            ? "Searching for devices…"
            : "No paired or discoverable devices. Network discovery only works for a few minutes after the device's Wi-Fi is switched on — or enter the address manually below."}
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
                      Connected
                    </span>
                  )}
                  {row.status === "paired" && (
                    <span className="shrink-0 text-[10px] uppercase tracking-wide text-text-secondary">
                      Paired
                    </span>
                  )}
                </div>
                <div className="truncate text-xs text-text-secondary">
                  {row.serial ?? "unknown serial"}
                  {row.address ? ` · ${row.address}` : ""}
                  {!row.online && row.status !== "connected" ? " · offline" : ""}
                </div>
              </div>

              {row.status !== "connected" && row.serial && (
                <button
                  onClick={() => forget(row.serial!, row.name)}
                  className="px-2 py-1 text-xs text-text-secondary hover:text-text"
                >
                  Forget
                </button>
              )}

              {row.status === "connected" ? (
                <button
                  onClick={() => void ipc.disconnectDevice().then(onClose)}
                  className="border border-border px-2.5 py-1 text-xs hover:border-text"
                >
                  Disconnect
                </button>
              ) : row.status === "paired" ? (
                <button
                  disabled={busy !== null || (!row.online && !row.address)}
                  title={!row.online && !row.address ? "No known address" : undefined}
                  onClick={() => void connect(row.serial!, row.address)}
                  className="border border-accent bg-accent px-2.5 py-1 text-xs text-accent-foreground disabled:opacity-50"
                >
                  Connect
                </button>
              ) : (
                <button
                  disabled={busy !== null || !row.address}
                  onClick={() => row.address && void useAddress(row.address)}
                  className="border border-accent bg-accent px-2.5 py-1 text-xs text-accent-foreground disabled:opacity-50"
                >
                  Pair
                </button>
              )}
            </li>
          ))}
        </ul>
      )}

      <section>
        <h3 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Manual address
        </h3>
        <div className="flex gap-2">
          <input
            value={manual}
            onChange={(e) => setManual(e.target.value)}
            onKeyDown={(e) =>
              e.key === "Enter" && manual.trim() && void useAddress(manual.trim())
            }
            placeholder="e.g. 10.0.1.12 or digitalpaper.local"
            className="min-w-0 flex-1 border border-border bg-bg px-2 py-1.5 text-[13px] placeholder:text-text-secondary focus:border-text focus:outline-none"
          />
          <button
            disabled={manual.trim() === "" || busy !== null}
            onClick={() => void useAddress(manual.trim())}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
          >
            Connect
          </button>
        </div>
      </section>

      {busy && <p className="mt-3 text-text-secondary">{busy}</p>}
    </Dialog>
  );
}
