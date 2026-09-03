import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import Dialog from "./Dialog";
import ContextMenu from "./ContextMenu";
import { TemplateIcon, UploadIcon } from "./icons";
import { useApp } from "../lib/store";
import { useBrowse } from "../lib/browse";
import { ipc, errorMessage, EVENTS, type NoteTemplate } from "../lib/ipc";
import { useT } from "../lib/i18n";
import { displayName } from "../lib/format";

/**
 * Templates view (docs/05 §3.4; FR-BRW-7, FR-TRF-6): template cards (grid)
 * or rows (list), following the toolbar view-mode toggle, with add (file
 * picker or PDF drop) and delete. Uploads run through the shared transfer
 * queue; the backend emits `templates:invalidated` when one finishes.
 */
export default function TemplatesView() {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const connection = useApp((s) => s.connection);
  const viewMode = useBrowse((s) => s.viewMode);
  const connected = connection.state === "connected";

  const [templates, setTemplates] = useState<NoteTemplate[] | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; tpl: NoteTemplate } | null>(
    null,
  );
  const [deleteTarget, setDeleteTarget] = useState<NoteTemplate | null>(null);
  const [dragOver, setDragOver] = useState(false);

  const refresh = useCallback(() => {
    if (!connected) return;
    ipc
      .listTemplates()
      .then(setTemplates)
      .catch((err) => toast(errorMessage(err), "error"));
  }, [connected, toast]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const unlisten = listen(EVENTS.templatesInvalidated, refresh);
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const uploadPaths = useCallback(
    async (paths: string[]) => {
      const pdfs = paths.filter((p) => p.toLowerCase().endsWith(".pdf"));
      if (pdfs.length === 0) {
        toast(t("templates.pdfOnly"), "error");
        return;
      }
      try {
        await ipc.uploadTemplates(pdfs);
        toast(t("templates.uploadQueued", { n: pdfs.length }));
      } catch (err) {
        toast(errorMessage(err), "error");
      }
    },
    [toast, t],
  );

  // OS drag & drop: dropping PDFs adds them as templates (docs/05 §3.4).
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over" || event.payload.type === "enter") {
        setDragOver(true);
      } else if (event.payload.type === "leave") {
        setDragOver(false);
      } else if (event.payload.type === "drop") {
        setDragOver(false);
        void uploadPaths(event.payload.paths);
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [uploadPaths]);

  const pickAndUpload = async () => {
    const picked = await openFileDialog({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length > 0) await uploadPaths(paths);
  };

  if (!templates) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-secondary">
        {t("templates.loading")}
      </div>
    );
  }

  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
        <button
          onClick={() => void pickAndUpload()}
          className="flex items-center gap-1.5 border border-border px-2.5 py-1 text-[13px] hover:border-text"
        >
          <UploadIcon width={14} height={14} />
          {t("templates.add")}
        </button>
        <span className="text-xs text-text-secondary">{t("templates.addHint")}</span>
      </div>

      {templates.length === 0 ? (
        <div className="flex flex-1 items-center justify-center p-8 text-center text-text-secondary">
          {t("templates.empty")}
        </div>
      ) : viewMode === "grid" ? (
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          <div className="grid grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-2">
            {templates.map((tpl) => (
              <button
                key={tpl.note_template_id}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, tpl });
                }}
                className="flex flex-col items-center gap-2 border border-border px-2 py-4 hover:border-text"
              >
                <TemplateIcon width={28} height={28} />
                <span
                  className="w-full truncate text-center text-xs"
                  title={tpl.template_name}
                >
                  {displayName(tpl.template_name)}
                </span>
              </button>
            ))}
          </div>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ul>
            {templates.map((tpl) => (
              <li
                key={tpl.note_template_id}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, tpl });
                }}
                className="flex h-9 items-center gap-2 border-b border-border px-2 text-[13px] hover:bg-surface"
              >
                <TemplateIcon className="shrink-0" />
                <span className="truncate" title={tpl.template_name}>
                  {displayName(tpl.template_name)}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {dragOver && (
        <div className="pointer-events-none absolute inset-0 z-30 flex items-center justify-center border-2 border-text bg-bg/80">
          <div className="flex items-center gap-2 text-[15px] font-medium">
            <UploadIcon width={20} height={20} />
            {t("templates.add")}
          </div>
        </div>
      )}

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={[
            {
              label: t("library.deleteAction"),
              onClick: () => setDeleteTarget(menu.tpl),
            },
          ]}
          onClose={() => setMenu(null)}
        />
      )}

      {deleteTarget && (
        <DeleteTemplateDialog
          template={deleteTarget}
          onClose={() => setDeleteTarget(null)}
          onDeleted={() => {
            setDeleteTarget(null);
            refresh();
          }}
        />
      )}
    </div>
  );
}

function DeleteTemplateDialog({
  template,
  onClose,
  onDeleted,
}: {
  template: NoteTemplate;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);

  const doDelete = async () => {
    setBusy(true);
    try {
      await ipc.deleteTemplate(template.note_template_id);
      toast(t("templates.deleted", { name: template.template_name }));
      onDeleted();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  return (
    <Dialog title={t("templates.deleteTitle")} onClose={onClose}>
      <p className="mb-4">
        {t("templates.deleteBody", { name: template.template_name })}
      </p>
      <div className="flex justify-end gap-2">
        <button
          onClick={onClose}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={() => void doDelete()}
          disabled={busy}
          className="border-2 border-text bg-bg px-3 py-1.5 font-semibold disabled:opacity-50"
        >
          {busy ? t("common.deleting") : t("common.delete")}
        </button>
      </div>
    </Dialog>
  );
}
