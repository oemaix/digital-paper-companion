import type { SVGProps } from "react";

/**
 * Monochrome stroke icon set (docs/05 §5.1): 1.5 px stroke, `currentColor`,
 * 24x24 viewBox rendered at 16/20 px. Rectangles use sharp corners to match
 * the app's e-ink-inspired design language.
 */
function makeIcon(children: React.ReactNode) {
  return function Icon(props: SVGProps<SVGSVGElement>) {
    return (
      <svg
        xmlns="http://www.w3.org/2000/svg"
        viewBox="0 0 24 24"
        width={16}
        height={16}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        {...props}
      >
        {children}
      </svg>
    );
  };
}

export const FolderIcon = makeIcon(
  <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />,
);

export const NoteIcon = makeIcon(
  <>
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
    <path d="M10 9H8" />
    <path d="M16 13H8" />
    <path d="M16 17H8" />
  </>,
);

export const TemplateIcon = makeIcon(
  <>
    <rect width="18" height="7" x="3" y="3" />
    <rect width="9" height="7" x="3" y="14" />
    <rect width="5" height="7" x="16" y="14" />
  </>,
);

export const ListIcon = makeIcon(
  <>
    <path d="M3 6h.01" />
    <path d="M3 12h.01" />
    <path d="M3 18h.01" />
    <path d="M8 6h13" />
    <path d="M8 12h13" />
    <path d="M8 18h13" />
  </>,
);

export const GridIcon = makeIcon(
  <>
    <rect width="7" height="7" x="3" y="3" />
    <rect width="7" height="7" x="14" y="3" />
    <rect width="7" height="7" x="3" y="14" />
    <rect width="7" height="7" x="14" y="14" />
  </>,
);

export const ConnectIcon = makeIcon(
  <>
    <path d="M12 22v-5" />
    <path d="M9 8V2" />
    <path d="M15 8V2" />
    <path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z" />
  </>,
);

// Horizontal opposing arrows (⇄): visually distinct from the circular
// RefreshIcon and matching the two-way nature of folder sync.
export const SyncIcon = makeIcon(
  <>
    <path d="m17 2 4 4-4 4" />
    <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
    <path d="m7 22-4-4 4-4" />
    <path d="M21 13v1a4 4 0 0 1-4 4H3" />
  </>,
);

export const SettingsIcon = makeIcon(
  <>
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  </>,
);

export const ChevronRightIcon = makeIcon(<path d="m9 18 6-6-6-6" />);

export const CloseIcon = makeIcon(
  <>
    <path d="M18 6 6 18" />
    <path d="m6 6 12 12" />
  </>,
);

export const FileIcon = makeIcon(
  <>
    <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
    <path d="M14 2v4a2 2 0 0 0 2 2h4" />
  </>,
);

export const DownloadIcon = makeIcon(
  <>
    <path d="M12 15V3" />
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <path d="m7 10 5 5 5-5" />
  </>,
);

/** Bidirectional arrows — the transfer queue, not a one-shot download. */
export const TransfersIcon = makeIcon(
  <>
    <path d="M7 3v12" />
    <path d="m3 7 4-4 4 4" />
    <path d="M17 21V9" />
    <path d="m13 17 4 4 4-4" />
  </>,
);

export const UploadIcon = makeIcon(
  <>
    <path d="M12 3v12" />
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
    <path d="m7 8 5-5 5 5" />
  </>,
);

export const FolderPlusIcon = makeIcon(
  <>
    <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
    <path d="M12 10v6" />
    <path d="M9 13h6" />
  </>,
);

export const SearchIcon = makeIcon(
  <>
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.3-4.3" />
  </>,
);

export const RefreshIcon = makeIcon(
  <>
    <path d="M21 12a9 9 0 1 1-2.64-6.36L21 8" />
    <path d="M21 3v5h-5" />
  </>,
);

export const TrashIcon = makeIcon(
  <>
    <path d="M3 6h18" />
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
    <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
  </>,
);

export const PencilIcon = makeIcon(
  <path d="M21.17 6.83a2.85 2.85 0 0 0-4-4L3.84 16.17a2 2 0 0 0-.5.83l-1.32 4.35a.5.5 0 0 0 .62.62l4.35-1.32a2 2 0 0 0 .83-.5Z" />,
);

export const TabletIcon = makeIcon(
  <>
    <rect width="16" height="20" x="4" y="2" />
    <path d="M9 4h6" />
  </>,
);

export const ArrowUpIcon = makeIcon(<path d="m5 12 7-7 7 7M12 19V5" />);

export const CameraIcon = makeIcon(
  <>
    <path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z" />
    <circle cx="12" cy="13" r="3" />
  </>,
);

export const ClockIcon = makeIcon(
  <>
    <circle cx="12" cy="12" r="9" />
    <path d="M12 7v5l3 2" />
  </>,
);

/** Clock with an arrow into a tablet: push this computer's time to the device. */
export const ClockToDeviceIcon = makeIcon(
  <>
    <circle cx="8" cy="7" r="5" />
    <path d="M8 4.5V7l1.8 1.2" />
    <rect width="8" height="11" x="14" y="11" />
    <path d="M16.5 13.5h3" />
    <path d="M8 12v3a2 2 0 0 0 2 2h1" />
    <path d="m9 14.5 2.5 2.5-2.5 2.5" />
  </>,
);

export const PlusIcon = makeIcon(<path d="M12 5v14M5 12h14" />);

/** Closed padlock: a secured (WPA2/WPA3) Wi-Fi network. */
export const LockIcon = makeIcon(
  <>
    <rect width="14" height="10" x="5" y="11" />
    <path d="M8 11V7a4 4 0 0 1 8 0v4" />
  </>,
);

/** Padlock with a dashed shackle: weak security (WEP / WPA1). */
export const LockWeakIcon = makeIcon(
  <>
    <rect width="14" height="10" x="5" y="11" />
    <path d="M8 11V7a4 4 0 0 1 8 0v4" strokeDasharray="2.5 2" />
  </>,
);

/** Open padlock: an unsecured network. */
export const LockOpenIcon = makeIcon(
  <>
    <rect width="14" height="10" x="5" y="11" />
    <path d="M8 11V6a4 4 0 0 1 7.7-1.5" />
  </>,
);

export const SpinnerIcon = makeIcon(<path d="M21 12a9 9 0 1 1-6.22-8.56" />);

/** Run now: a sharp play triangle. */
export const PlayIcon = makeIcon(<path d="m6 4 14 8-14 8Z" />);

/** Stop/cancel a running job: a sharp square. */
export const StopIcon = makeIcon(<rect width="12" height="12" x="6" y="6" />);

/** Preview/dry-run: an eye. */
export const EyeIcon = makeIcon(
  <>
    <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" />
    <circle cx="12" cy="12" r="3" />
  </>,
);

/** Run history: a clock winding back. */
export const HistoryIcon = makeIcon(
  <>
    <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
    <path d="M3 3v5h5" />
    <path d="M12 7v5l4 2" />
  </>,
);
