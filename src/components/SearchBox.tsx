import { useEffect, useMemo, useRef, useState } from "react";
import { bgmSearch, BgmHit, Work } from "../lib/api";
import { Candidate, formatLabel, search } from "../lib/search";

interface Props {
  indexReady: boolean;
  indexSize: number;
  existing: Set<number>;
  initialQuery?: string;
  works: Work[];
  onAdd: (c: Candidate) => Promise<void>;
  onOpen: (id: number) => void;
}

type Item =
  | { kind: "local"; work: Work }
  | { kind: "remote"; c: Candidate }
  | { kind: "bgm"; h: BgmHit };

export default function SearchBox({
  indexReady,
  indexSize,
  existing,
  initialQuery = "",
  works,
  onAdd,
  onOpen,
}: Props) {
  const [q, setQ] = useState(initialQuery);
  const [open, setOpen] = useState(initialQuery.length > 0);
  const [active, setActive] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const boxRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const keyword = q.trim().toLowerCase();

  // 库里已有：先给命中，省得再去网上找一遍
  const local = useMemo(() => {
    if (!keyword) return [];
    return works
      .filter(
        (w) =>
          [w.titleCn, w.titleJp, w.titleEn, w.aliases, ...(w.tags ?? [])]
            .filter(Boolean)
            .some((t) => t!.toLowerCase().includes(keyword)),
      )
      .slice(0, 4);
  }, [keyword, works]);

  // 依赖里必须带上 indexReady / indexSize：索引是异步加载的，
  // 只依赖 q 的话，索引到位后结果不会重算（会一直显示“没找到”）。
  const remote = useMemo(() => {
    if (!q.trim()) return [];
    return search(q)
      .filter((c) => !(c.anilistId > 0 && existing.has(c.anilistId)))
      .slice(0, 6);
  }, [q, indexReady, indexSize, existing]);

  // 本地索引没有命中时，自动去 Bangumi 搜（国漫的主力来源）
  const [bgm, setBgm] = useState<BgmHit[]>([]);
  const [bgmBusy, setBgmBusy] = useState(false);
  const [bgmError, setBgmError] = useState<string | null>(null);

  useEffect(() => {
    const kw = q.trim();
    if (kw.length < 2 || remote.length > 0) {
      setBgm([]);
      setBgmBusy(false);
      return;
    }
    let alive = true;
    setBgmBusy(true);
    setBgmError(null);
    const timer = window.setTimeout(() => {
      bgmSearch(kw)
        .then((hits) => {
          if (alive) setBgm(hits);
        })
        .catch((e: unknown) => {
          // 把失败亮出来：否则用户分不清"没有"和"连不上"
          if (alive) {
            setBgm([]);
            setBgmError(
              e instanceof Error
                ? e.message
                : "Bangumi 连不上（可能需要开启代理的系统代理模式）",
            );
          }
        })
        .finally(() => {
          if (alive) setBgmBusy(false);
        });
    }, 500);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [q, remote.length]);

  const items = useMemo<Item[]>(
    () => [
      ...local.map((work) => ({ kind: "local" as const, work })),
      ...remote.map((c) => ({ kind: "remote" as const, c })),
      ...bgm.map((h) => ({ kind: "bgm" as const, h })),
    ],
    [local, remote, bgm],
  );

  useEffect(() => {
    setActive(0);
  }, [q]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        setOpen(true);
      }
    };
    document.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  async function pickBgm(h: BgmHit) {
    await pick({
      anilistId: 0,
      bangumiId: h.id,
      cn: h.nameCn || h.name,
      jp: h.name,
      year: h.year ?? 0,
      type: "tv",
      score: 0,
    });
  }

  async function pick(c: Candidate) {
    const key = String(c.anilistId || `${c.cn}${c.jp}`);
    if (busy) return;
    setBusy(key);
    try {
      await onAdd(c);
      setQ("");
      setOpen(false);
    } finally {
      setBusy(null);
    }
  }

  function openLocal(id: number) {
    onOpen(id);
    setQ("");
    setOpen(false);
    inputRef.current?.blur();
  }

  function onInputKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      setOpen(false);
      return;
    }
    if (!items.length) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => (i + 1) % items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => (i - 1 + items.length) % items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const hit = items[active];
      if (!hit) return;
      if (hit.kind === "local") openLocal(hit.work.id);
      else if (hit.kind === "bgm") void pickBgm(hit.h);
      else void pick(hit.c);
    }
  }

  const showPanel = open && q.trim().length > 0;

  return (
    <div className="searchbox" ref={boxRef}>
      <div className="search-field">
        <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
          <circle cx="11" cy="11" r="7" fill="none" stroke="currentColor" strokeWidth="1.8" />
          <path d="M16.5 16.5 21 21" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
        </svg>
        <input
          ref={inputRef}
          value={q}
          onChange={(e) => {
            setQ(e.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={onInputKeyDown}
          placeholder="搜番名：库里的和网上的都能搜"
          spellCheck={false}
          autoComplete="off"
        />
        {q ? (
          <button
            className="clear"
            onClick={() => {
              setQ("");
              inputRef.current?.focus();
            }}
            aria-label="清空"
          >
            ×
          </button>
        ) : (
          <kbd>Ctrl K</kbd>
        )}
      </div>

      {showPanel && (
        <div className="results" role="listbox">
          {local.length > 0 && (
            <>
              <div className="result-group">库里已有</div>
              {local.map((w, i) => (
                <button
                  key={`local-${w.id}`}
                  className={`result result-local${i === active ? " is-active" : ""}`}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => openLocal(w.id)}
                >
                  <span className="result-main">
                    <span className="result-cn">{w.titleCn}</span>
                    <span className="result-sub">
                      {[
                        w.titleJp && w.titleJp !== w.titleCn ? w.titleJp : "",
                        w.year ? String(w.year) : "",
                        STATUS_CN[w.status] ?? "",
                        w.rating > 0 ? `${w.rating} 分` : "",
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  </span>
                  <span className="result-add is-done">打开</span>
                </button>
              ))}
            </>
          )}

          {remote.length > 0 && (
            <>
              <div className="result-group">
                {local.length > 0 ? "网上还有其他" : "网上可添加"}
              </div>
              {remote.map((c, j) => {
                const idx = local.length + j;
                const key = String(c.anilistId || `${c.cn}${c.jp}`);
                return (
                  <button
                    key={`remote-${key}-${j}`}
                    className={`result${idx === active ? " is-active" : ""}`}
                    onMouseEnter={() => setActive(idx)}
                    onClick={() => void pick(c)}
                    disabled={busy === key}
                  >
                    <span className="result-main">
                      <span className="result-cn">{c.cn || c.jp}</span>
                      <span className="result-sub">
                        {[c.jp !== c.cn ? c.jp : "", c.year || "", formatLabel(c.type)]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                    </span>
                    <span className="result-add">{busy === key ? "加入中" : "加入"}</span>
                  </button>
                );
              })}
            </>
          )}

          {bgmError && (
            <div className="result-empty">
              Bangumi 连不上，国漫在线搜索不可用
              <span>{bgmError}</span>
              <span>开代理的「系统代理」模式后重试即可</span>
            </div>
          )}

          {(bgm.length > 0 || bgmBusy) && (
            <>
              <div className="result-group">Bangumi 在线 · 国漫也能搜</div>
              {bgmBusy && <div className="result-empty">Bangumi 搜索中…</div>}
              {bgm.map((h, k) => {
                const idx = local.length + remote.length + k;
                const c: Candidate = {
                  anilistId: 0,
                  bangumiId: h.id,
                  cn: h.nameCn || h.name,
                  jp: h.name,
                  year: h.year ?? 0,
                  type: "tv",
                  score: 0,
                };
                return (
                  <button
                    key={`bgm-${h.id}`}
                    className={`result${idx === active ? " is-active" : ""}`}
                    onMouseEnter={() => setActive(idx)}
                    onClick={() => void pick(c)}
                    disabled={busy === String(h.id)}
                  >
                    <span className="result-main">
                      <span className="result-cn">{h.nameCn || h.name}</span>
                      <span className="result-sub">
                        {[h.name !== (h.nameCn || h.name) ? h.name : "", h.year ?? "", h.eps ? `${h.eps} 集` : ""]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                    </span>
                    <span className="result-add">{busy === String(h.id) ? "加入中" : "加入"}</span>
                  </button>
                );
              })}
            </>
          )}

          {local.length === 0 &&
            remote.length === 0 &&
            bgm.length === 0 &&
            !bgmBusy &&
            !bgmError && (
            <div className="result-empty">
              {!indexReady
                ? "索引载入中…"
                : bgmBusy
                  ? "Bangumi 搜索中…"
                  : `没找到「${q.trim()}」`}
              <span>国漫会自动去 Bangumi 搜；日漫可以试试日文原名</span>
              </div>
            )}

          <div className="result-foot">
            索引 {indexSize.toLocaleString()} 部 · 库里 {works.length} 部
          </div>
        </div>
      )}
    </div>
  );
}

const STATUS_CN: Record<string, string> = {
  plan: "想看",
  watching: "在看",
  done: "看完",
  paused: "搁置",
  dropped: "弃番",
};
