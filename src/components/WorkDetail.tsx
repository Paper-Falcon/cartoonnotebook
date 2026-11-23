import { useEffect, useRef, useState } from "react";
import { Tag, Work, addWorkTag, coverUrl, removeWorkTag } from "../lib/api";
import { formatLabel } from "../lib/search";
import { genreLabel, parseGenres } from "../lib/genres";
import MediaGallery from "./MediaGallery";

const STATUSES = [
  { key: "plan", label: "想看" },
  { key: "watching", label: "在看" },
  { key: "done", label: "看完" },
  { key: "paused", label: "搁置" },
  { key: "dropped", label: "弃番" },
];

interface Props {
  work: Work;
  onPatch: (id: number, patch: Partial<Work>) => void;
  onDelete: (id: number) => void;
  onClose: () => void;
  onToast: (msg: string) => void;
  allTags: Tag[];
  onTagsChange: (id: number, tags: string[]) => void;
}

const today = () => new Date().toISOString().slice(0, 10);

export default function WorkDetail({
  work,
  onPatch,
  onDelete,
  onClose,
  onToast,
  allTags,
  onTagsChange,
}: Props) {
  const [note, setNote] = useState(work.note ?? "");
  const [broken, setBroken] = useState(false);
  const [tags, setTags] = useState<string[]>(work.tags ?? []);
  const [tagDraft, setTagDraft] = useState("");
  const [synopsis, setSynopsis] = useState(work.synopsis ?? "");
  const noteTimer = useRef<number | undefined>(undefined);
  const synTimer = useRef<number | undefined>(undefined);
  const genres = parseGenres(work.genres);

  useEffect(() => () => window.clearTimeout(synTimer.current), []);

  function changeSynopsis(v: string) {
    setSynopsis(v);
    window.clearTimeout(synTimer.current);
    synTimer.current = window.setTimeout(() => onPatch(work.id, { synopsis: v }), 700);
  }

  async function pushTag() {
    const name = tagDraft.trim();
    if (!name) return;
    try {
      const next = await addWorkTag(work.id, name);
      setTags(next);
      setTagDraft("");
      onTagsChange(work.id, next);
    } catch (e) {
      onToast(`加标签失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }

  async function dropTag(name: string) {
    const hit = allTags.find((t) => t.name === name);
    if (!hit) return;
    try {
      const next = await removeWorkTag(work.id, hit.id);
      setTags(next);
      onTagsChange(work.id, next);
    } catch (e) {
      onToast(`移除失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }
  const total = work.episodesTotal ?? 0;

  useEffect(() => {
    setNote(work.note ?? "");
    setBroken(false);
  }, [work.id]);

  useEffect(() => () => window.clearTimeout(noteTimer.current), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  function changeNote(v: string) {
    setNote(v);
    window.clearTimeout(noteTimer.current);
    noteTimer.current = window.setTimeout(() => onPatch(work.id, { note: v }), 600);
  }

  function setStatus(status: string) {
    const patch: Partial<Work> = { status };
    if (status === "done" && !work.finishedAt) patch.finishedAt = today();
    if (status !== "done" && work.finishedAt) patch.finishedAt = "";
    onPatch(work.id, patch);
  }

  function setProgress(next: number) {
    const v = Math.max(0, total > 0 ? Math.min(next, total) : next);
    const patch: Partial<Work> = { progress: v };
    if (total > 0 && v === total && work.status !== "done") {
      patch.status = "done";
      patch.finishedAt = work.finishedAt || today();
    }
    onPatch(work.id, patch);
  }

  const meta = [
    work.year ? String(work.year) : "",
    work.format ? formatLabel(work.format.toLowerCase()) : "",
    total > 0 && work.format?.toLowerCase() !== "movie" ? `${total} 集` : "",
  ]
    .filter(Boolean)
    .join(" · ");

  const src = coverUrl(work.coverPath);
  const pct = total > 0 ? Math.min(100, Math.round((work.progress / total) * 100)) : 0;

  return (
    <>
      <div className="backdrop" onClick={onClose} />
      <aside className="drawer" role="dialog" aria-label={work.titleCn}>
        <button className="drawer-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        <header className="drawer-head">
          <div className="drawer-cover">
            {src && !broken ? (
              <img src={src} alt={work.titleCn} onError={() => setBroken(true)} />
            ) : (
              <div className="cover-fallback">{work.titleCn.slice(0, 1)}</div>
            )}
          </div>
          <div className="drawer-info">
            <h2>{work.titleCn}</h2>
            {work.titleJp && work.titleJp !== work.titleCn && <p className="drawer-jp">{work.titleJp}</p>}
            <p className="drawer-meta">{meta || "—"}</p>
            {genres.length > 0 && (
              <div className="genre-row">
                {genres.slice(0, 4).map((g) => (
                  <span className="genre" key={g}>
                    {genreLabel(g)}
                  </span>
                ))}
              </div>
            )}
          </div>
        </header>

        <div className="drawer-body">
          <section className="field">
            <label className="field-label">状态</label>
            <div className="seg">
              {STATUSES.map((s) => (
                <button
                  key={s.key}
                  className={`seg-btn${work.status === s.key ? " is-active" : ""}`}
                  onClick={() => setStatus(s.key)}
                >
                  {s.label}
                </button>
              ))}
            </div>
          </section>

          <section className="field">
            <label className="field-label">
              评分
              <span className="field-value">{work.rating > 0 ? `${work.rating} / 10` : "未评"}</span>
            </label>
            <div className="rate-row">
              {Array.from({ length: 10 }, (_, i) => i + 1).map((n) => (
                <button
                  key={n}
                  className={`rate-btn${work.rating >= n ? " is-on" : ""}`}
                  onClick={() => onPatch(work.id, { rating: work.rating === n ? 0 : n })}
                  aria-label={`${n} 分`}
                >
                  {n}
                </button>
              ))}
            </div>
          </section>

          <section className="field">
            <label className="field-label">
              进度
              <span className="field-value">
                {work.progress} / {total > 0 ? total : "?"}
              </span>
            </label>
            <div className="stepper">
              <button onClick={() => setProgress(work.progress - 1)} disabled={work.progress <= 0}>
                −
              </button>
              <div className="progress-track">
                <div className="progress-fill" style={{ width: `${pct}%` }} />
              </div>
              <button onClick={() => setProgress(work.progress + 1)}>＋</button>
            </div>
          </section>

          <section className="field">
            <label className="field-label">看完日期</label>
            <input
              type="date"
              className="date-input"
              value={work.finishedAt?.slice(0, 10) ?? ""}
              onChange={(e) => onPatch(work.id, { finishedAt: e.target.value })}
            />
          </section>

          <section className="field">
            <label className="field-label">短评</label>
            <textarea
              className="note"
              value={note}
              placeholder="看完想说什么…"
              onChange={(e) => changeNote(e.target.value)}
              rows={4}
            />
          </section>

          <section className="field">
            <label className="field-label">
              标签
              <span className="field-value">{tags.length ? `${tags.length} 个` : "未加"}</span>
            </label>
            <div className="tag-row">
              {tags.map((t) => (
                <span className="tag-chip" key={t}>
                  {t}
                  <button onClick={() => void dropTag(t)} aria-label={`移除 ${t}`}>
                    ×
                  </button>
                </span>
              ))}
              <input
                className="tag-input"
                list="tag-suggest"
                value={tagDraft}
                placeholder={tags.length ? "再加一个…" : "加个标签，回车确认"}
                onChange={(e) => setTagDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void pushTag();
                  }
                }}
              />
              <datalist id="tag-suggest">
                {allTags.map((t) => (
                  <option key={t.id} value={t.name} />
                ))}
              </datalist>
            </div>
          </section>

          <section className="field">
            <label className="field-label">
              简介
              <span className="field-value">
                {synopsis.trim()
                  ? /[\u3400-\u9fa5]/.test(synopsis)
                    ? "点击可编辑"
                    : "英文原文 · 可直接改成中文"
                  : "还没有"}
              </span>
            </label>
            <textarea
              className="note synopsis-edit"
              value={synopsis}
              rows={6}
              placeholder="还没有简介。可以自己写两句，或粘贴一段中文简介。"
              onChange={(e) => changeSynopsis(e.target.value)}
            />
          </section>

          <MediaGallery
            workId={work.id}
            mode={work.mediaMode}
            onModeChange={(m) => onPatch(work.id, { mediaMode: m })}
            onToast={onToast}
          />
        </div>

        <footer className="drawer-foot">
          <span className="drawer-id">
            {work.anilistId ? `AniList #${work.anilistId}` : "手动记录"}
          </span>
          <button
            className="btn-danger"
            onClick={() => {
              if (window.confirm(`确定要从档案馆删除《${work.titleCn}》？`)) onDelete(work.id);
            }}
          >
            删除
          </button>
        </footer>
      </aside>
    </>
  );
}
