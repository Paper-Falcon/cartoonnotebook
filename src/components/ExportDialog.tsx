import { useState } from "react";
import { createPortal } from "react-dom";
import { API_BASE, ExportInfo, exportDownloadUrl, exportLibrary } from "../lib/api";
import { formatBytes } from "../lib/media";

interface Props {
  onClose: () => void;
  onToast: (msg: string) => void;
}

export default function ExportDialog({ onClose, onToast }: Props) {
  const [busy, setBusy] = useState(false);
  const [info, setInfo] = useState<ExportInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [restoreMsg, setRestoreMsg] = useState<string | null>(null);
  const [restoreError, setRestoreError] = useState(false);

  async function run() {
    setBusy(true);
    setError(null);
    try {
      const result = await exportLibrary();
      setInfo(result);
      onToast(`已导出 ${formatBytes(result.bytes)}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function onRestoreFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setRestoring(true);
    setRestoreMsg(null);
    setRestoreError(false);
    try {
      const fd = new FormData();
      fd.append("file", file);
      const res = await fetch(`${API_BASE}/api/restore`, { method: "POST", body: fd });
      if (!res.ok) {
        const j = (await res.json().catch(() => ({}))) as { error?: string };
        throw new Error(j.error || `HTTP ${res.status}`);
      }
      setRestoreMsg("恢复包已就绪：关闭应用再重新打开，就会替换成备份里的数据。");
      onToast("恢复包已就绪");
    } catch (err) {
      setRestoreError(true);
      setRestoreMsg(`恢复失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setRestoring(false);
      e.target.value = "";
    }
  }

  return createPortal(
    <div className="modal-backdrop" onClick={onClose}>
      <div className="pair-card export-card" onClick={(e) => e.stopPropagation()}>
        <button className="lightbox-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        <h2>备份与恢复</h2>
        <p className="pair-sub">
          打成一个 zip：数据库快照 + 全部封面 + 归档素材，外加一份清单（含引用模式的原文件路径）。
        </p>

        {info ? (
          <div className="export-result">
            <div className="export-name">{info.name}</div>
            <div className="export-meta">
              {info.works} 部作品 · {info.media} 个素材 · {formatBytes(info.bytes)}
            </div>
            <div className="export-btn-row">
              <a className="btn-primary" href={exportDownloadUrl()} download>
                下载备份
              </a>
              <button className="btn-ghost" onClick={() => void run()} disabled={busy}>
                再导一份
              </button>
            </div>
          </div>
        ) : (
          <div className="export-btn-row">
            <button className="btn-primary" onClick={() => void run()} disabled={busy}>
              {busy ? "正在打包…" : "开始导出"}
            </button>
          </div>
        )}

        {error && <div className="pair-error">{error}</div>}

        <div className="restore-box">
          <h3>从备份恢复</h3>
          <p className="pair-sub">
            选择之前导出的 zip。恢复包先就位，<b>重启应用后生效</b>；当前数据会自动留一份到
            data/.restore-backup-*，删错了还能捞回来。
          </p>
          <input
            type="file"
            accept=".zip"
            onChange={(e) => void onRestoreFile(e)}
            disabled={restoring}
          />
          {restoring && <div className="pair-loading">正在接收恢复包…</div>}
          {restoreMsg && (
            <div className={`restore-msg${restoreError ? " is-error" : ""}`}>{restoreMsg}</div>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
