import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  Media,
  deleteMedia,
  importMedia,
  listMedia,
  mediaFileUrl,
  mediaThumbUrl,
  patchMedia,
  patchMediaEpisode,
  patchMediaPosition,
  repointMedia,
  uploadMedia,
} from "../lib/api";
import {
  captureFrame,
  extractVideoPoster,
  formatBytes,
  formatTime,
  isTauri,
  makeThumb,
} from "../lib/media";

interface Props {
  workId: number;
  mode: string;
  onModeChange: (mode: string) => void;
  onToast: (msg: string) => void;
}

export default function MediaGallery({ workId, mode, onModeChange, onToast }: Props) {
  const [items, setItems] = useState<Media[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [preview, setPreview] = useState<Media | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let alive = true;
    listMedia(workId)
      .then((list) => {
        if (!alive) return;
        setItems(list);
        // /?work=1&media=3 直接打开某个素材
        const want = Number(new URLSearchParams(window.location.search).get("media"));
        if (want > 0) {
          const hit = list.find((m) => m.id === want);
          if (hit) setPreview(hit);
        }
      })
      .catch(() => {
        if (alive) setItems([]);
      });
    return () => {
      alive = false;
    };
  }, [workId]);

  // 粘贴截图：拖进来太慢，粘一下最快
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      const files = Array.from(e.clipboardData?.files ?? []).filter((f) =>
        f.type.startsWith("image/"),
      );
      if (!files.length) return;
      e.preventDefault();
      void addFiles(files);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workId]);

  // 桌面端拖拽：Tauri 会给出真实路径，能走"引用"模式
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const un = await getCurrentWebview().onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "enter" || payload.type === "over") setDragging(true);
          else if (payload.type === "leave") setDragging(false);
          else if (payload.type === "drop") {
            setDragging(false);
            void importPaths(payload.paths);
          }
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        /* 浏览器里没有这个 API，忽略 */
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workId, mode]);

  async function importPaths(paths: string[]) {
    const files = paths.filter((p) => !/\.(txt|md|json|exe|dll|zip|rar)$/i.test(p));
    if (!files.length) return;
    setBusy(mode === "ref" ? "登记中…" : "复制中…");
    try {
      for (const p of files) {
        const media = await importMedia(workId, p, mode);
        setItems((prev) => [media, ...prev]);
      }
      onToast(mode === "ref" ? `已引用 ${files.length} 个文件` : `已归档 ${files.length} 个文件`);
    } catch (e) {
      onToast(`导入失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(null);
    }
  }

  async function addFiles(files: File[]) {
    setBusy("上传中…");
    try {
      for (const file of files) {
        const isVideo =
          file.type.startsWith("video/") || /\.(mp4|webm|mov|mkv|m4v|avi)$/i.test(file.name);

        let thumb: Blob | undefined;
        let poster: Blob | undefined;
        let duration: number | undefined;

        if (isVideo) {
          try {
            const r = await extractVideoPoster(file);
            poster = r.poster;
            duration = r.duration;
          } catch {
            /* 浏览器解不了的封装（如 mkv），先不带封面 */
          }
        } else {
          try {
            thumb = await makeThumb(file);
          } catch {
            /* 忽略 */
          }
        }

        const media = await uploadMedia(workId, file, {
          kind: isVideo ? "video" : "image",
          thumb,
          poster,
          duration,
          filename: file.name || (isVideo ? "video" : "image"),
        });
        setItems((prev) => [media, ...prev]);
      }
      onToast(`已加入 ${files.length} 个素材`);
    } catch (e) {
      onToast(`上传失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(null);
    }
  }

  async function remove(media: Media) {
    if (!window.confirm("确定删除这个素材？归档文件会先移到 data/.trash")) return;
    try {
      await deleteMedia(media.id);
      setItems((prev) => prev.filter((m) => m.id !== media.id));
      setPreview(null);
      onToast("已删除");
    } catch (e) {
      onToast(`删除失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }

  // 按集分组：有集号的按集号升序，没集号的垫底
  const grouped = useMemo(() => {
    const map = new Map<number | null, Media[]>();
    for (const m of items) {
      const key = m.episode ?? null;
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(m);
    }
    return [...map.entries()].sort((a, b) => {
      if (a[0] === null) return 1;
      if (b[0] === null) return -1;
      return a[0] - b[0];
    });
  }, [items]);

  const hasEpisodes = grouped.some(([ep]) => ep !== null);

  return (
    <section className="field">
      <label className="field-label">
        素材
        <span className="field-value">
          {items.length} 个 · {mode === "ref" ? "引用" : "归档"}
          {hasEpisodes ? ` · ${grouped.filter(([e]) => e !== null).length} 集` : ""}
        </span>
      </label>

      <div className="media-grid">
        {grouped.map(([ep, list]) => (
          <Fragment key={ep === null ? "none" : `ep-${ep}`}>
            {hasEpisodes && (
              <div className="media-group-head">
                {ep === null ? "未分集" : `第 ${ep} 集`}
                <span>{list.length}</span>
              </div>
            )}
            {list.map((m) => (
              <button
                key={m.id}
                className={`media-tile${m.missing ? " is-missing" : ""}`}
                onClick={() => setPreview(m)}
                title={m.path}
              >
                {m.thumb || m.poster ? (
                  // /thumb 在缩略图缺失时会自动回落到 poster
                  <img src={mediaThumbUrl(m.id)} alt="" loading="lazy" />
                ) : m.kind === "video" ? (
                  <span className="media-play">▶</span>
                ) : (
                  <img src={mediaFileUrl(m.id)} alt="" loading="lazy" />
                )}
                {m.kind === "video" && (
                  <span className="media-badge">▶ {formatTime(m.duration)}</span>
                )}
                {m.storage === "ref" && <span className="media-tag">引用</span>}
                {m.kind === "video" && m.duration && m.position > 1 && (
                  <span className="media-progress">
                    <i
                      style={{
                        width: `${Math.min(100, (m.position / (m.duration || 1)) * 100)}%`,
                      }}
                    />
                  </span>
                )}
                {m.missing ? <span className="media-lost">文件丢失</span> : null}
              </button>
            ))}
          </Fragment>
        ))}

        <button
          className="media-tile media-add"
          onClick={() => fileRef.current?.click()}
          disabled={busy !== null}
        >
          {busy ? <span className="media-busy">{busy}</span> : <span className="media-plus">＋</span>}
        </button>
      </div>

      <input
        ref={fileRef}
        type="file"
        multiple
        accept="image/*,video/*"
        hidden
        onChange={(e) => {
          const files = Array.from(e.target.files ?? []);
          e.target.value = "";
          if (files.length) void addFiles(files);
        }}
      />

      <div className="media-hint-row">
        <p className="media-hint">
          Ctrl+V 粘贴截图，或把文件拖进窗口。桌面端拖入时可选择只记路径。
        </p>
        <div className="mode-switch">
          <button
            className={`mode-btn${mode === "ref" ? " is-on" : ""}`}
            onClick={() => onModeChange("ref")}
          >
            引用
          </button>
          <button
            className={`mode-btn${mode === "archive" ? " is-on" : ""}`}
            onClick={() => onModeChange("archive")}
          >
            归档
          </button>
        </div>
      </div>

      {dragging &&
        createPortal(<div className="drop-veil">松开鼠标，加入素材</div>, document.body)}

      {preview &&
        createPortal(
          <Lightbox
            media={preview}
            workId={workId}
            onClose={() => setPreview(null)}
            onDelete={() => void remove(preview)}
            onCaptured={(m) => setItems((prev) => [m, ...prev])}
            onEpisode={(id, ep) => {
              setItems((prev) => prev.map((x) => (x.id === id ? { ...x, episode: ep } : x)));
              setPreview((cur) => (cur && cur.id === id ? { ...cur, episode: ep } : cur));
            }}
            onProgress={(id, pos) => {
              setItems((prev) => prev.map((x) => (x.id === id ? { ...x, position: pos } : x)));
            }}
            onRepointed={(m) => {
              setItems((prev) => prev.map((x) => (x.id === m.id ? m : x)));
              setPreview(m);
            }}
            onToast={onToast}
          />,
          document.body,
        )}
    </section>
  );
}

interface LightboxProps {
  media: Media;
  workId: number;
  onClose: () => void;
  onDelete: () => void;
  onCaptured: (m: Media) => void;
  onEpisode: (id: number, ep: number | null) => void;
  onProgress: (id: number, position: number) => void;
  onRepointed: (m: Media) => void;
  onToast: (msg: string) => void;
}

function Lightbox({
  media,
  workId,
  onClose,
  onDelete,
  onCaptured,
  onEpisode,
  onProgress,
  onRepointed,
  onToast,
}: LightboxProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [caption, setCaption] = useState(media.caption ?? "");
  const [episode, setEpisode] = useState(media.episode ?? 0);
  const [capturing, setCapturing] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const epTimer = useRef<number | undefined>(undefined);
  const lastSaved = useRef(media.position ?? 0);
  const [resumedFrom, setResumedFrom] = useState<number | null>(null);

  useEffect(() => () => window.clearTimeout(epTimer.current), []);

  // 关闭灯箱时把最后的播放位置落库
  useEffect(() => {
    const id = media.id;
    return () => {
      const v = videoRef.current;
      if (v && v.currentTime > 1) void patchMediaPosition(id, v.currentTime).catch(() => undefined);
    };
  }, [media.id]);

  async function repoint() {
    try {
      let path: string | null = null;
      if (isTauri()) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const picked = await open({ multiple: false, directory: false, title: "选择这个素材的新位置" });
        path = typeof picked === "string" ? picked : null;
      } else {
        path = window.prompt("粘贴这个素材的新路径");
      }
      if (!path) return;
      const updated = await repointMedia(media.id, path);
      onRepointed(updated);
      onToast("已指向新位置");
    } catch (e) {
      onToast(`重新定位失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }

  function changeEpisode(v: string) {
    const n = Math.max(0, Number(v) || 0);
    setEpisode(n);
    window.clearTimeout(epTimer.current);
    epTimer.current = window.setTimeout(() => {
      void patchMediaEpisode(media.id, n)
        .then((m) => onEpisode(media.id, m.episode ?? null))
        .catch(() => onToast("集号没保存上"));
    }, 500);
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function changeCaption(v: string) {
    setCaption(v);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      void patchMedia(media.id, v).catch(() => onToast("备注没保存上"));
    }, 600);
  }

  async function grab() {
    const video = videoRef.current;
    if (!video) return;
    setCapturing(true);
    try {
      const blob = await captureFrame(video);
      const created = await uploadMedia(workId, blob, {
        kind: "image",
        stamp: video.currentTime,
        filename: "frame.jpg",
      });
      onCaptured(created);
      onToast(`已截取 ${formatTime(video.currentTime)} 这一帧`);
    } catch (e) {
      onToast(`截帧失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setCapturing(false);
    }
  }

  return (
    <div className="lightbox" onClick={onClose}>
      <div className="lightbox-inner" onClick={(e) => e.stopPropagation()}>
        <button className="lightbox-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        {media.kind === "video" ? (
          <video
            ref={videoRef}
            className="lightbox-media"
            src={mediaFileUrl(media.id)}
            controls
            autoPlay
            onLoadedMetadata={(e) => {
              const v = e.currentTarget;
              const pos = media.position ?? 0;
              if (pos > 3 && pos < v.duration - 3) {
                v.currentTime = pos;
                setResumedFrom(pos);
              }
            }}
            onTimeUpdate={(e) => {
              const v = e.currentTarget;
              if (Math.abs(v.currentTime - lastSaved.current) < 3) return;
              lastSaved.current = v.currentTime;
              onProgress(media.id, v.currentTime);
              void patchMediaPosition(media.id, v.currentTime).catch(() => undefined);
            }}
          />
        ) : (
          <img className="lightbox-media" src={mediaFileUrl(media.id)} alt={media.caption ?? ""} />
        )}

        <div className="lightbox-bar">
          <label className="lightbox-ep">
            第
            <input
              type="number"
              min={0}
              value={episode || ""}
              placeholder="—"
              onChange={(e) => changeEpisode(e.target.value)}
            />
            集
          </label>
          <input
            className="lightbox-caption"
            value={caption}
            placeholder="给这个素材写点备注…"
            onChange={(e) => changeCaption(e.target.value)}
          />
          <span className="lightbox-meta">
            {media.storage === "ref" ? "引用" : "归档"} · {formatBytes(media.bytes)}
            {media.stamp != null ? ` · ${formatTime(media.stamp)}` : ""}
            {resumedFrom ? ` · 从 ${formatTime(resumedFrom)} 续播` : ""}
          </span>
          <div className="lightbox-actions">
            {media.kind === "video" && (
              <button className="btn-ghost" onClick={() => void grab()} disabled={capturing}>
                {capturing ? "截取中…" : "截这一帧"}
              </button>
            )}
            {media.missing > 0 && (
              <button className="btn-ghost" onClick={() => void repoint()}>
                重新定位
              </button>
            )}
            <button className="btn-danger" onClick={onDelete}>
              删除
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
