import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  Wallpaper,
  WallpaperState,
  listWallpapers,
  setWallpaperSetting,
  wallpaperPreviewUrl,
} from "../lib/api";
import { formatBytes } from "../lib/media";

interface Props {
  state: WallpaperState | null;
  onClose: () => void;
  onChanged: (s: WallpaperState) => void;
  onToast: (msg: string) => void;
}

export default function WallpaperDialog({ state, onClose, onChanged, onToast }: Props) {
  const [list, setList] = useState<Wallpaper[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [local, setLocal] = useState<WallpaperState | null>(state);
  const [reloadKey, setReloadKey] = useState(0);
  const [onlyVideo, setOnlyVideo] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => setLocal(state), [state]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  useEffect(() => {
    let alive = true;
    setList(null);
    listWallpapers(reloadKey > 0)
      .then((rows) => {
        if (alive) setList(rows);
      })
      .catch((e: unknown) => {
        if (alive) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      alive = false;
    };
  }, [reloadKey]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const usable = useMemo(
    () => (list ?? []).filter((w) => w.video || w.preview),
    [list],
  );
  const videoCount = usable.filter((w) => w.video).length;
  const shown = onlyVideo ? usable.filter((w) => w.video) : usable;

  /** 改了参数先本地生效，再防抖落库 */
  function patch(p: Parameters<typeof setWallpaperSetting>[0], immediate = false) {
    setLocal((cur) =>
      cur
        ? ({
            ...cur,
            ...p,
          } as WallpaperState)
        : cur,
    );
    window.clearTimeout(timer.current);
    const run = () => {
      setWallpaperSetting(p)
        .then(onChanged)
        .catch((e: unknown) =>
          onToast(`保存失败：${e instanceof Error ? e.message : String(e)}`),
        );
    };
    if (immediate) run();
    else timer.current = window.setTimeout(run, 180);
  }

  function pick(w: Wallpaper) {
    patch({ id: w.id, enabled: true }, true);
    onToast(`背景已换成「${w.title}」`);
  }

  function remove() {
    patch({ id: "" }, true);
    onToast("已移除背景");
  }

  const veil = local?.veil ?? 0.8;
  const blur = local?.blur ?? 3;
  const fit = local?.fit ?? "cover";

  return createPortal(
    <div className="modal-backdrop" onClick={onClose}>
      <div className="pair-card wp-card" onClick={(e) => e.stopPropagation()}>
        <button className="lightbox-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        <h2>背景壁纸</h2>
        <p className="pair-sub">
          读的是 Wallpaper Engine 创意工坊目录。视频壁纸循环播放；场景、网页壁纸和识别不出编码的视频，自动用预览图当静态背景——全部可选。
        </p>

        <div className="wp-controls">
          <label className="wp-ctl">
            <span>薄纱 {Math.round(veil * 100)}%</span>
            <input
              type="range"
              min={0}
              max={100}
              value={Math.round(veil * 100)}
              onChange={(e) => patch({ veil: Number(e.target.value) / 100 })}
            />
          </label>

          <label className="wp-ctl">
            <span>模糊 {Math.round(blur)}px</span>
            <input
              type="range"
              min={0}
              max={24}
              value={Math.round(blur)}
              onChange={(e) => patch({ blur: Number(e.target.value) })}
            />
          </label>

          <label className="wp-ctl wp-ctl-short">
            <span>填充</span>
            <select value={fit} onChange={(e) => patch({ fit: e.target.value })}>
              <option value="cover">铺满</option>
              <option value="contain">完整</option>
            </select>
          </label>

          <label className="wp-ctl wp-ctl-short">
            <span>显示</span>
            <button
              className={`filter-toggle${local?.enabled ? " is-on" : ""}`}
              onClick={() => patch({ enabled: !local?.enabled })}
              disabled={!local?.id}
            >
              {local?.enabled ? "开" : "关"}
            </button>
          </label>
        </div>

        <div className="wp-actions">
          <button className="filter-toggle" onClick={() => setReloadKey((k) => k + 1)}>
            重新扫描
          </button>
          <button
            className={`filter-toggle${onlyVideo ? " is-on" : ""}`}
            onClick={() => setOnlyVideo((v) => !v)}
          >
            只看视频
          </button>
          <button className="filter-toggle" onClick={remove} disabled={!local?.id}>
            移除背景
          </button>
          <span className="wp-count">
            {list === null
              ? "扫描中…"
              : `${usable.length} 个可选 · 视频 ${videoCount} · 静态 ${usable.length - videoCount}`}
            {local?.title ? ` · 当前：${local.title}` : ""}
          </span>
        </div>

        {error ? (
          <div className="pair-error">
            {error}
            <button className="btn-ghost" onClick={() => setReloadKey((k) => k + 1)}>
              重试
            </button>
          </div>
        ) : list === null ? (
          <div className="pair-loading">正在扫描 Steam 创意工坊…</div>
        ) : shown.length === 0 ? (
          <div className="pair-loading">
            没找到可用的壁纸。确认 Wallpaper Engine 装过壁纸，且 Steam 库目录可读。
          </div>
        ) : (
          <div className="wp-grid">
            {shown.map((w) => (
              <button
                key={w.id}
                className={`wp-item${local?.id === w.id ? " is-on" : ""}`}
                onClick={() => pick(w)}
                title={w.title}
              >
                <img src={wallpaperPreviewUrl(w.id)} alt={w.title} loading="lazy" />
                <span className="wp-title">{w.title}</span>
                <span className="wp-meta">
                  {w.video ? formatBytes(w.bytes) : "静态"}
                  {w.tags[0] ? ` · ${w.tags[0]}` : ""}
                </span>
                {!w.video && <span className="wp-static">静态</span>}
                {local?.id === w.id && <span className="wp-badge">使用中</span>}
              </button>
            ))}
          </div>
        )}

        <p className="pair-note">
          参数改完立刻生效并记住。视频壁纸常驻播放比较耗电，4K 素材建议调低薄纱或者只当静态用。
        </p>
      </div>
    </div>,
    document.body,
  );
}
