import { useEffect, useMemo, useRef, useState } from "react";
import SearchBox from "./components/SearchBox";
import WorkCard from "./components/WorkCard";
import WorkDetail from "./components/WorkDetail";
import PairDialog from "./components/PairDialog";
import ExportDialog from "./components/ExportDialog";
import TagsDialog from "./components/TagsDialog";
import WallpaperDialog from "./components/WallpaperDialog";
import { Appearance, Tag, WallpaperState, Work, STATUS_LABEL, addWork, addWorkTag, getAppearance, getWallpaperSetting, setAppearance as saveAppearanceApi, setWallpaperSetting, listTags, listWorks, patchWork, removeWork, wallpaperEntryUrl, wallpaperPreviewUrl, wallpaperVideoUrl } from "./lib/api";
import { Candidate, loadIndex } from "./lib/search";
import { loadWebWallGL, SceneHandle } from "./lib/scenewall";
import SettingsDialog from "./components/SettingsDialog";
import { API_BASE } from "./lib/api";
import { genreLabel, parseGenres } from "./lib/genres";

const TABS = [
  { key: "all", label: "全部" },
  { key: "watching", label: "在看" },
  { key: "done", label: "看完" },
  { key: "plan", label: "想看" },
  { key: "paused", label: "搁置" },
  { key: "dropped", label: "弃番" },
] as const;

type TabKey = (typeof TABS)[number]["key"];
type SortKey = "added" | "rating" | "year" | "title";

export default function App() {
  const [works, setWorks] = useState<Work[]>([]);
  const [loading, setLoading] = useState(true);
  const [indexReady, setIndexReady] = useState(false);
  const [indexSize, setIndexSize] = useState(0);
  const [toast, setToast] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [showPair, setShowPair] = useState(false);
  const [filter, setFilter] = useState<TabKey>("all");
  const [sort, setSort] = useState<SortKey>("added");
  const [showFilters, setShowFilters] = useState(false);
  const [year, setYear] = useState("all");
  const [genre, setGenre] = useState("all");
  const [rating, setRating] = useState("all");
  const [tagFilter, setTagFilter] = useState("all");
  const [tags, setTags] = useState<Tag[]>([]);
  const [showExport, setShowExport] = useState(false);
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [bulkTag, setBulkTag] = useState("");
  const [bulkBusy, setBulkBusy] = useState(false);
  const [showWallpaper, setShowWallpaper] = useState(false);
  const [showTags, setShowTags] = useState(false);
  const [wall, setWall] = useState<WallpaperState | null>(null);
  const toastTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    listWorks()
      .then(setWorks)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));

    refreshTags();

    loadIndex()
      .then((n) => {
        setIndexSize(n);
        setIndexReady(true);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, []);

  function refreshTags() {
    listTags()
      .then(setTags)
      .catch(() => setTags([]));
  }

  // 外观（字体/文字颜色）：后端持久化，启动时取一次
  const [appearance, setAppearance] = useState<Appearance>({
    font: "",
    fontCustom: "",
    color: "",
  });
  const [showSettings, setShowSettings] = useState(false);

  // 背景壁纸：设置存在后端，启动时取一次
  const wallActive = Boolean(
    wall?.id && wall.enabled && (wall.hasVideo || wall.hasWeb || wall.hasPreview),
  );
  // 视频模式还要这台机器真的放得动：确认放不出的走静态
  const videoOk = Boolean(wall?.hasVideo && !wall.unplayable?.includes(wall.id ?? ""));

  useEffect(() => {
    getAppearance()
      .then((a) =>
        setAppearance({
          font: a.font ?? "",
          fontCustom: a.fontCustom ?? "",
          color: a.color ?? "",
        }),
      )
      .catch(() => undefined);
    getWallpaperSetting()
      .then(setWall)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("has-wallpaper", wallActive);
    return () => root.classList.remove("has-wallpaper");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wall?.id, wall?.enabled]);

  // 外观：字体与文字颜色写进根 CSS 变量
  useEffect(() => {
    const root = document.documentElement;
    const effFont = appearance.fontCustom.trim() || appearance.font.trim();
    if (effFont) {
      root.style.setProperty("--font-sans", `"${effFont}", sans-serif`);
    } else {
      root.style.removeProperty("--font-sans");
    }
    if (appearance.color) {
      root.style.setProperty("--ink", appearance.color);
      root.style.setProperty(
        "--ink-2",
        `color-mix(in srgb, ${appearance.color} 62%, var(--bg))`,
      );
      root.style.setProperty(
        "--ink-3",
        `color-mix(in srgb, ${appearance.color} 45%, var(--bg))`,
      );
    } else {
      root.style.removeProperty("--ink");
      root.style.removeProperty("--ink-2");
      root.style.removeProperty("--ink-3");
    }
  }, [appearance]);

  async function handleSaveAppearance(a: Appearance) {
    const saved = await saveAppearanceApi(a);
    setAppearance({
      font: saved.font ?? a.font,
      fontCustom: saved.fontCustom ?? a.fontCustom,
      color: saved.color ?? a.color,
    });
    showToast("外观已保存");
  }

  // 窗口切到后台就暂停，别在看不见的时候空转解码
  useEffect(() => {
    if (!wallActive) return;
    const video = document.querySelector<HTMLVideoElement>(".wallpaper-layer video");
    if (!video) return;
    const onVisibility = () => {
      if (document.hidden) video.pause();
      else video.play().catch(() => undefined);
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [wallActive, wall?.id]);

  // 视频 10 秒还没出画面（readyState 太低）也判为播不了，避免一直停在封面上
  useEffect(() => {
    if (!wallActive || !wall?.hasVideo || wall.unplayable?.includes(wall.id ?? "")) {
      return;
    }
    const wid = wall.id;
    if (!wid) return;
    const timer = window.setTimeout(() => {
      const v = document.querySelector<HTMLVideoElement>(".wallpaper-layer video");
      if (v && v.readyState < 2) {
        void setWallpaperSetting({
          unplayable: [...(wall.unplayable ?? []), wid],
        })
          .then(setWall)
          .catch(() => undefined);
        showToast("这个壁纸迟迟出不了画面，已记住并改用静态预览");
      }
    }, 10000);
    return () => window.clearTimeout(timer);
  }, [wallActive, wall?.id, wall?.hasVideo]);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  function showToast(msg: string) {
    setToast(msg);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2600);
  }

  const existing = useMemo(
    () => new Set(works.map((w) => w.anilistId ?? -1)),
    [works],
  );

  // 支持 /?q=巨人 这种深链，方便直接打开搜索态
  const initialQuery = useMemo(
    () => new URLSearchParams(window.location.search).get("q") ?? "",
    [],
  );

  // /?work=12 直接打开某部番的详情
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const id = Number(params.get("work"));
    if (Number.isFinite(id) && id > 0) setSelectedId(id);
    if (params.get("pair") === "1") setShowPair(true);
    if (params.get("export") === "1") setShowExport(true);
    if (params.get("wall") === "1") setShowWallpaper(true);
    if (params.get("tags") === "1") setShowTags(true);
    if (params.get("pick") === "1") setPicking(true);
    if (params.get("filters") === "1") setShowFilters(true);
    // 深链：/ ?year=2013&genre=Drama&rating=8
    const y = params.get("year");
    const g = params.get("genre");
    const r = params.get("rating");
    if (y) {
      setYear(y);
      setShowFilters(true);
    }
    if (g) {
      setGenre(g);
      setShowFilters(true);
    }
    if (r) {
      setRating(r);
      setShowFilters(true);
    }
  }, []);

  const counts = useMemo(() => {
    const c: Record<string, number> = { all: works.length };
    for (const w of works) c[w.status] = (c[w.status] ?? 0) + 1;
    return c;
  }, [works]);

  const visible = useMemo(() => {
    let base = filter === "all" ? works : works.filter((w) => w.status === filter);
    if (year !== "all") base = base.filter((w) => w.year === Number(year));
    if (genre !== "all") base = base.filter((w) => parseGenres(w.genres).includes(genre));
    if (rating !== "all") {
      base =
        rating === "none"
          ? base.filter((w) => w.rating === 0)
          : base.filter((w) => w.rating >= Number(rating));
    }
    if (tagFilter !== "all") {
      base = base.filter((w) => (w.tags ?? []).includes(tagFilter));
    }

    const arr = [...base];
    switch (sort) {
      case "rating":
        arr.sort((a, b) => b.rating - a.rating || (b.year ?? 0) - (a.year ?? 0));
        break;
      case "year":
        arr.sort((a, b) => (b.year ?? 0) - (a.year ?? 0));
        break;
      case "title":
        arr.sort((a, b) => a.titleCn.localeCompare(b.titleCn, "zh-Hans-CN"));
        break;
      default:
        arr.sort((a, b) => b.id - a.id);
    }
    return arr;
  }, [works, filter, sort, year, genre, rating, tagFilter]);

  // 筛选下拉里的可选项，只列库里真实存在的
  const yearOptions = useMemo(() => {
    const set = new Set<number>();
    for (const w of works) if (w.year) set.add(w.year);
    return [...set].sort((a, b) => b - a);
  }, [works]);

  const genreOptions = useMemo(() => {
    const set = new Set<string>();
    for (const w of works) for (const g of parseGenres(w.genres)) set.add(g);
    return [...set].sort((a, b) =>
      genreLabel(a).localeCompare(genreLabel(b), "zh-Hans-CN"),
    );
  }, [works]);

  const activeFilters = [year, genre, rating, tagFilter].filter((v) => v !== "all").length;

  const selected = selectedId
    ? (works.find((w) => w.id === selectedId) ?? null)
    : null;

  async function handleAdd(c: Candidate) {
    try {
      const work = await addWork({
        anilistId: c.anilistId || undefined,
        bangumiId: c.bangumiId || undefined,
        titleCn: c.cn || c.jp,
        titleJp: c.jp || undefined,
        year: c.year || undefined,
      });
      setWorks((prev) =>
        prev.some((w) => w.id === work.id) ? prev : [work, ...prev],
      );
      showToast(`已归档《${work.titleCn}》`);
      // 封面在后台下载：几秒后静默刷新让封面浮现
      const refresh = () => {
        listWorks()
          .then((fresh) => {
            setWorks((prev) =>
              prev.map((w) => fresh.find((f) => f.id === w.id) ?? w),
            );
          })
          .catch(() => undefined);
      };
      window.setTimeout(refresh, 4000);
      window.setTimeout(refresh, 10000);
    } catch (e) {
      showToast(`加入失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }


  async function handlePatch(id: number, patch: Partial<Work>) {
    const snapshot = works;
    setWorks((prev) =>
      prev.map((w) => (w.id === id ? ({ ...w, ...patch } as Work) : w)),
    );
    try {
      const updated = await patchWork(id, patch);
      setWorks((prev) => prev.map((w) => (w.id === id ? updated : w)));
    } catch (e) {
      setWorks(snapshot);
      showToast(`保存失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }

  async function handleDelete(id: number) {
    try {
      await removeWork(id);
      setWorks((prev) => prev.filter((w) => w.id !== id));
      setSelectedId(null);
      showToast("已从档案馆移除");
    } catch (e) {
      showToast(`删除失败：${e instanceof Error ? e.message : String(e)}`);
    }
  }

  const doneCount = counts.done ?? 0;

  // ---------- 整理模式 ----------

  function togglePick(id: number) {
    setPicked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function exitPicking() {
    setPicking(false);
    setPicked(new Set());
    setBulkTag("");
  }

  async function bulkStatus(status: string) {
    const ids = [...picked];
    if (!ids.length || bulkBusy) return;
    setBulkBusy(true);
    try {
      const finishedAt = status === "done" ? new Date().toISOString().slice(0, 10) : null;
      for (const id of ids) await patchWork(id, { status, finishedAt });
      setWorks(await listWorks());
      showToast(`已把 ${ids.length} 部设为「${STATUS_LABEL[status] ?? status}」`);
      exitPicking();
    } catch (e) {
      showToast(`批量修改失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBulkBusy(false);
    }
  }

  async function bulkAddTag() {
    const name = bulkTag.trim();
    if (!name || !picked.size || bulkBusy) return;
    setBulkBusy(true);
    try {
      for (const id of [...picked]) await addWorkTag(id, name);
      setWorks(await listWorks());
      refreshTags();
      showToast(`已给 ${picked.size} 部加上「${name}」`);
      exitPicking();
    } catch (e) {
      showToast(`加标签失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBulkBusy(false);
    }
  }

  async function bulkDelete() {
    const ids = [...picked];
    if (!ids.length) return;
    if (!window.confirm(`确定从档案馆移除选中的 ${ids.length} 部？封面会保留在磁盘上。`)) return;
    setBulkBusy(true);
    try {
      for (const id of ids) await removeWork(id);
      setWorks(await listWorks());
      showToast(`已移除 ${ids.length} 部`);
      exitPicking();
    } catch (e) {
      showToast(`删除失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBulkBusy(false);
    }
  }

  return (
    <>
      {wallActive && wall && wall.id && (
        <div className="wallpaper-layer" aria-hidden="true">
          {videoOk ? (
            <video
              key={wall.id}
              src={wallpaperVideoUrl(wall.id)}
              poster={wallpaperPreviewUrl(wall.id)}
              autoPlay
              muted
              loop
              playsInline
              style={{
                objectFit: wall.fit === "contain" ? "contain" : "cover",
                filter: wall.blur > 0 ? `blur(${wall.blur}px)` : undefined,
                // 模糊会把边缘透明化，稍微放大一点盖住
                transform: wall.blur > 0 ? "scale(1.04)" : undefined,
              }}
              onError={() => {
                const wid = wall.id;
                if (wid && !wall.unplayable?.includes(wid)) {
                  void setWallpaperSetting({
                    unplayable: [...(wall.unplayable ?? []), wid],
                  })
                    .then(setWall)
                    .catch(() => undefined);
                }
                showToast("这个壁纸播不了，已记住并改用静态预览");
              }}
            />
          ) : wall.hasWeb ? (
            <iframe
              key={wall.id}
              src={wallpaperEntryUrl(wall.id)}
              title={wall.title ?? "壁纸"}
              sandbox="allow-scripts"
              className="wallpaper-web"
              scrolling="no"
            />
          ) : (
            <SceneLayer
              key={wall.id}
              id={wall.id}
              fit={wall.fit === "contain" ? "contain" : "cover"}
              blur={wall.blur}
            />
          )}
          <div
            className="wallpaper-veil"
            style={{ background: `rgba(251, 250, 247, ${wall.veil})` }}
          />
        </div>
      )}

      <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="mark" src="/icon.png" alt="" />
          <span className="brand-name">动漫档案馆</span>
        </div>

        <SearchBox
          indexReady={indexReady}
          indexSize={indexSize}
          existing={existing}
          initialQuery={initialQuery}
          works={works}
          onAdd={handleAdd}
          onOpen={(id) => setSelectedId(id)}
        />

        <div className="stats">
          <button
            className={`pair-btn${wallActive ? " is-active" : ""}`}
            onClick={() => setShowWallpaper(true)}
          >
            背景
            {wallActive && <i className="pair-dot" />}
          </button>
          <button className="pair-btn" onClick={() => setShowSettings(true)}>
            外观
          </button>
          <button className="pair-btn" onClick={() => setShowExport(true)}>
            备份
          </button>
          <button className="pair-btn" onClick={() => setShowPair(true)}>
            手机访问
          </button>
          <span className="stat-num">{works.length}</span>
          <span className="stat-label">部收藏</span>
          {doneCount > 0 && <span className="stat-sub">其中看完 {doneCount}</span>}
        </div>
      </header>

      <main className="content">
        {error && (
          <div className="banner">
            后端没连上：{error}
            <span>确认服务已在 127.0.0.1:8787 启动</span>
          </div>
        )}

        {loading ? (
          <div className="grid" aria-hidden="true">
            {Array.from({ length: 8 }).map((_, i) => (
              <div className="card skeleton" key={i}>
                <div className="cover" />
                <div className="sk-line" />
                <div className="sk-line short" />
              </div>
            ))}
          </div>
        ) : works.length === 0 ? (
          <div className="empty">
            <img className="empty-mark" src="/icon.png" alt="" />
            <h1>档案馆还是空的</h1>
            <p>在上方搜一部动画，把它收进来。封面和信息会自动补全。</p>
            <div className="empty-steps">
              <span>输入名字</span>
              <i />
              <span>选中那一部</span>
              <i />
              <span>封面落到本地</span>
            </div>
          </div>
        ) : (
          <>
            <div className="toolbar">
              <div className="tabs" role="tablist">
                {TABS.map((t) => {
                  const n = counts[t.key] ?? 0;
                  return (
                    <button
                      key={t.key}
                      role="tab"
                      aria-selected={filter === t.key}
                      className={`tab${filter === t.key ? " is-active" : ""}`}
                      onClick={() => setFilter(t.key)}
                    >
                      {t.label}
                      <span className="tab-count">{n}</span>
                    </button>
                  );
                })}
              </div>

              <div className="toolbar-right">
                <button
                  className={`filter-toggle${picking ? " is-on" : ""}`}
                  onClick={() => {
                    if (picking) exitPicking();
                    else setPicking(true);
                  }}
                >
                  {picking ? "退出整理" : "整理"}
                </button>

                <button
                  className={`filter-toggle${showFilters ? " is-on" : ""}`}
                  onClick={() => setShowFilters((v) => !v)}
                  aria-expanded={showFilters}
                >
                  筛选
                  {activeFilters > 0 && <span className="filter-count">{activeFilters}</span>}
                </button>

                <select
                  className="sort"
                  value={sort}
                  onChange={(e) => setSort(e.target.value as SortKey)}
                  aria-label="排序方式"
                >
                  <option value="added">最近加入</option>
                  <option value="rating">评分最高</option>
                  <option value="year">年份最新</option>
                  <option value="title">按标题</option>
                </select>
              </div>
            </div>

            {showFilters && (
              <div className="filter-bar">
                <label className="filter-item">
                  <span>年份</span>
                  <select value={year} onChange={(e) => setYear(e.target.value)}>
                    <option value="all">全部</option>
                    {yearOptions.map((y) => (
                      <option key={y} value={y}>
                        {y}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="filter-item">
                  <span>题材</span>
                  <select value={genre} onChange={(e) => setGenre(e.target.value)}>
                    <option value="all">全部</option>
                    {genreOptions.map((g) => (
                      <option key={g} value={g}>
                        {genreLabel(g)}
                      </option>
                    ))}
                  </select>
                </label>

                <label className="filter-item">
                  <span>评分</span>
                  <select value={rating} onChange={(e) => setRating(e.target.value)}>
                    <option value="all">不限</option>
                    <option value="9">9 分以上</option>
                    <option value="8">8 分以上</option>
                    <option value="7">7 分以上</option>
                    <option value="none">还没评分</option>
                  </select>
                </label>

                <label className="filter-item">
                  <span>标签</span>
                  <select value={tagFilter} onChange={(e) => setTagFilter(e.target.value)}>
                    <option value="all">全部</option>
                    {tags.map((t) => (
                      <option key={t.id} value={t.name}>
                        {t.name}（{t.count}）
                      </option>
                    ))}
                  </select>
                </label>
                <button className="filter-toggle" onClick={() => setShowTags(true)}>
                  管理
                </button>

                {activeFilters > 0 && (
                  <button
                    className="filter-clear"
                    onClick={() => {
                      setYear("all");
                      setGenre("all");
                      setRating("all");
                      setTagFilter("all");
                    }}
                  >
                    清除筛选
                  </button>
                )}
              </div>
            )}

            {visible.length === 0 ? (
              <div className="filter-empty">
                {filter !== "all" || activeFilters > 0
                  ? "当前条件下没有番，放宽一点试试。"
                  : "还没有番，用上面的搜索框加一部。"}
              </div>
            ) : (
              <div className="grid">
                {visible.map((w) => (
                  <WorkCard
                    key={w.id}
                    work={w}
                    selectable={picking}
                    selected={picked.has(w.id)}
                    onClick={() => {
                      if (picking) togglePick(w.id);
                      else setSelectedId(w.id);
                    }}
                  />
                ))}
              </div>
            )}
          </>
        )}
      </main>

      {picking && (
        <div className="bulk-bar">
          <span className="bulk-count">已选 {picked.size} 部</span>
          <button
            className="bulk-link"
            onClick={() => setPicked(new Set(visible.map((w) => w.id)))}
          >
            全选
          </button>
          <button className="bulk-link" onClick={() => setPicked(new Set())}>
            清空
          </button>

          <select
            className="sort"
            value=""
            onChange={(e) => {
              if (e.target.value) void bulkStatus(e.target.value);
            }}
            disabled={!picked.size || bulkBusy}
          >
            <option value="">改状态…</option>
            {TABS.filter((t) => t.key !== "all").map((t) => (
              <option key={t.key} value={t.key}>
                {t.label}
              </option>
            ))}
          </select>

          <input
            className="bulk-input"
            placeholder="加标签，回车确认"
            value={bulkTag}
            onChange={(e) => setBulkTag(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void bulkAddTag();
              }
            }}
            disabled={!picked.size || bulkBusy}
          />

          <button
            className="btn-danger"
            onClick={() => void bulkDelete()}
            disabled={!picked.size || bulkBusy}
          >
            删除
          </button>
          <button className="bulk-link" onClick={exitPicking}>
            退出
          </button>
        </div>
      )}

      <footer className="foot">
        <span>数据保存在本机 data/ 目录</span>
        <span>索引 {indexSize.toLocaleString()} 部 · 由 bangumi-data 构建</span>
      </footer>

      {selected && (
        <WorkDetail
          key={selected.id}
          work={selected}
          onPatch={handlePatch}
          onDelete={handleDelete}
          onClose={() => setSelectedId(null)}
          onToast={showToast}
          allTags={tags}
          onTagsChange={(id, next) => {
            setWorks((prev) => prev.map((w) => (w.id === id ? { ...w, tags: next } : w)));
            refreshTags();
          }}
        />
      )}

      {toast && <div className="toast">{toast}</div>}

      {showPair && (
        <PairDialog onClose={() => setShowPair(false)} onToast={showToast} />
      )}

      {showExport && (
        <ExportDialog onClose={() => setShowExport(false)} onToast={showToast} />
      )}

      {showTags && (
        <TagsDialog
          tags={tags}
          onClose={() => setShowTags(false)}
          onChanged={refreshTags}
          onToast={showToast}
        />
      )}

      {showWallpaper && (
        <WallpaperDialog
          state={wall}
          onClose={() => setShowWallpaper(false)}
          onChanged={setWall}
          onToast={showToast}
        />
      )}
      {showSettings && (
        <SettingsDialog
          initial={appearance}
          onClose={() => setShowSettings(false)}
          onSave={handleSaveAppearance}
        />
      )}
      {showSettings && (
        <SettingsDialog
          initial={appearance}
          onClose={() => setShowSettings(false)}
          onSave={handleSaveAppearance}
        />
      )}
      </div>
    </>
  );
}


type AnyInst = {
  destroy: (opts?: unknown) => void;
  pause: () => void;
  resume: () => void;
};

/**
 * 场景壁纸层：WebWallGL 实时渲染（引擎失败时自动兜底成预览图）。
 * 应用窗口隐藏时暂停渲染循环。
 */
function SceneLayer({
  id,
  fit,
  blur,
}: {
  id: string;
  fit: "cover" | "contain";
  blur: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let handle: SceneHandle | null = null;
    let onVis: (() => void) | null = null;

    (async () => {
      try {
        const lib = await loadWebWallGL();
        if (cancelled || !ref.current) return;
        const inst = (await lib.mount(ref.current, {
          source: lib.httpSource(`${API_BASE}/api/wallpapers/${id}/file/`),
          fit,
          fps: 30,
          volume: 0,
        })) as AnyInst;
        handle = {
          destroy: () => {
            try {
              inst.destroy();
            } catch {
              // 关窗竞态下引擎可能已自毁
            }
          },
        };
        onVis = () => {
          try {
            if (document.hidden) inst.pause();
            else inst.resume();
          } catch {
            // 引擎可能已销毁
          }
        };
        document.addEventListener("visibilitychange", onVis);
      } catch {
        if (!cancelled) setFailed(true);
      }
    })();

    return () => {
      cancelled = true;
      if (onVis) document.removeEventListener("visibilitychange", onVis);
      handle?.destroy();
    };
  }, [id, fit]);

  if (failed) {
    return (
      <img
        src={wallpaperPreviewUrl(id)}
        alt=""
        style={{
          objectFit: fit === "contain" ? "contain" : "cover",
          filter: blur > 0 ? `blur(${blur}px)` : undefined,
          transform: blur > 0 ? "scale(1.04)" : undefined,
        }}
      />
    );
  }
  return (
    <div
      ref={ref}
      className="wallpaper-canvas"
      style={{
        filter: blur > 0 ? `blur(${blur}px)` : undefined,
        transform: blur > 0 ? "scale(1.04)" : undefined,
      }}
    />
  );
}