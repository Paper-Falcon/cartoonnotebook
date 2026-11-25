import { Fragment, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Work } from "../lib/api";
import { search, Candidate } from "../lib/search";

export interface UnrecognizedFile {
  path: string;
  episode: number | null;
}

interface Props {
  files: UnrecognizedFile[];
  works: Work[];
  /** 全部处理完（含放弃）后回调，App 用来 showToast */
  onDone: (msg: string) => void;
}

/**
 * 主页拖放识别不出来时的兜底：文件名里没有番名（自己录的屏、截图），
 * 让用户挑归属的番——库里已有的直接选，库里没有的用索引建新的。
 */
export default function DropAssignDialog({
  files,
  works,
  onDone,
}: Props) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const boxRef = useRef<HTMLDivElement>(null);

  // 库里已有：按查询过滤
  const libHits = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return works.slice(0, 6);
    return works
      .filter((w) =>
        [w.titleCn, w.titleJp, w.titleEn, w.aliases]
          .filter(Boolean)
          .some((t) => t!.toLowerCase().includes(q)),
      )
      .slice(0, 6);
  }, [query, works]);

  // 索引候选：建新条目用
  const idxHits = useMemo(() => {
    if (!query.trim()) return [] as Candidate[];
    return search(query)
      .filter(
        (c) =>
          !libHits.some(
            (w) =>
              (c.bangumiId > 0 && w.bangumiId === c.bangumiId) ||
              (c.anilistId > 0 && w.anilistId === c.anilistId),
          ),
      )
      .slice(0, 4);
  }, [query, libHits]);

  async function importTo(work: Work) {
    const mode = work.mediaMode === "ref" ? "ref" : "archive";
    const { importMedia, patchMediaEpisode } = await import("../lib/api");
    setBusy("导入中…");
    try {
      let ok = 0;
      for (const f of files) {
        try {
          const media = await importMedia(work.id, f.path, mode);
          if (f.episode !== null && f.episode > 0) {
            try {
              await patchMediaEpisode(media.id, f.episode);
            } catch {
              /* 分集写不上不影响导入 */
            }
          }
          ok++;
        } catch {
          /* 单个失败继续 */
        }
      }
      onDone(`已把 ${ok} 个素材挂到《${work.titleCn}》`);
    } finally {
      setBusy(null);
    }
  }

  async function createAndImport(c: Candidate) {
    setBusy("建条目中…");
    try {
      const { addWork } = await import("../lib/api");
      const work = await addWork({
        anilistId: c.anilistId || undefined,
        bangumiId: c.bangumiId || undefined,
        titleCn: c.cn || c.jp,
        titleJp: c.jp || undefined,
        year: c.year || undefined,
      });
      await importTo(work);
    } catch (e) {
      setBusy(null);
      onDone(`新建条目失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }

  function abandon() {
    onDone(`已放弃 ${files.length} 个未识别素材`);
  }

  return createPortal(
    <div
      className="pair-backdrop"
      onMouseDown={(e) => {
        if (boxRef.current && !boxRef.current.contains(e.target as Node)) return;
      }}
    >
      <div className="pair-card drop-assign" ref={boxRef}>
        <h2>这些素材属于哪部番？</h2>
        <p className="drop-assign-sub">
          文件名里没有番名（自己录的屏/截图），认不出来。选一部挂进去：
        </p>

        <div className="drop-assign-files">
          {files.map((f, i) => (
            <Fragment key={f.path}>
              {i < 5 && (
                <span className="drop-assign-file">
                  {f.path.split(/[\\/]/).pop()}
                  {f.episode ? ` · 第${f.episode}集` : ""}
                </span>
              )}
            </Fragment>
          ))}
          {files.length > 5 && (
            <span className="drop-assign-file">…共 {files.length} 个文件</span>
          )}
        </div>

        <input
          className="drop-assign-input"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="输入番名筛选库里已有的，或从索引建新的"
          spellCheck={false}
          autoFocus
        />

        <div className="drop-assign-list">
          {busy ? (
            <div className="drop-assign-empty">{busy}</div>
          ) : (
            <>
              {libHits.map((w) => (
                <button
                  key={`lib-${w.id}`}
                  className="drop-assign-item"
                  onClick={() => void importTo(w)}
                >
                  <span className="result-cn">{w.titleCn}</span>
                  <span className="result-add is-done">库里已有</span>
                </button>
              ))}
              {query.trim() &&
                idxHits.map((c) => (
                  <button
                    key={`idx-${c.bangumiId || c.anilistId || c.cn}`}
                    className="drop-assign-item"
                    onClick={() => void createAndImport(c)}
                  >
                    <span className="result-cn">{c.cn || c.jp}</span>
                    <span className="result-sub">
                      {[c.jp !== (c.cn || c.jp) ? c.jp : "", c.year || "", "索引 · 未入库"]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                    <span className="result-add">新建</span>
                  </button>
                ))}
              {libHits.length === 0 && idxHits.length === 0 && (
                <div className="drop-assign-empty">
                  {query.trim() ? "没找到匹配的番，换个名字试试" : "输入番名开始筛选"}
                </div>
              )}
            </>
          )}
        </div>

        <div className="drop-assign-actions">
          <button className="drop-assign-abandon" onClick={abandon}>
            放弃这些文件
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
