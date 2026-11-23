import { API_BASE } from "./api";

/**
 * 标题索引与匹配。
 * 索引来自后端 /api/index（构建期由 bangumi-data 生成），
 * 每条记录：[中文名, 日文名, 年份, AniList ID, Bangumi ID, 全拼, 首字母, 去虚词首字母, 类型]
 *
 * 没有用 MiniSearch：它的默认分词会把中文整串当一个 token，
 * "巨人" 这类中间子串搜不到。8931 条记录做一次线性子串扫描不到 1ms，
 * 再按命中位置排序，效果反而更贴合中文习惯。
 */
export type IndexRow = [
  string,
  string,
  number,
  number,
  number,
  string,
  string,
  string,
  string,
];

export interface Candidate {
  cn: string;
  jp: string;
  year: number;
  anilistId: number;
  bangumiId: number;
  type: string;
  score: number;
}

interface Store {
  items: IndexRow[];
  hay: string[];
}

let store: Store | null = null;
let loading: Promise<number> | null = null;

export function indexCount(): number {
  return store ? store.items.length : 0;
}

export function loadIndex(): Promise<number> {
  if (store) return Promise.resolve(store.items.length);
  if (loading) return loading;

  loading = fetch(`${API_BASE}/api/index`)
    .then((r) => {
      if (!r.ok) throw new Error(`索引加载失败 ${r.status}`);
      return r.json();
    })
    .then((data: { items: IndexRow[] }) => {
      const items = data.items ?? [];
      const hay = items.map((row) =>
        `${row[0]}${row[1]}${row[5]}${row[6]}${row[7]}`.toLowerCase(),
      );
      store = { items, hay };
      return items.length;
    })
    .catch((err) => {
      loading = null;
      throw err;
    });

  return loading;
}

const FORMAT_LABEL: Record<string, string> = {
  tv: "TV",
  movie: "剧场版",
  ova: "OVA",
  web: "WEB",
};

export const formatLabel = (type: string) => FORMAT_LABEL[type] ?? "动画";

export function search(query: string, limit = 14): Candidate[] {
  if (!store) return [];
  const needle = query.trim().toLowerCase().replace(/\s+/g, "");
  if (!needle) return [];

  const { items, hay } = store;
  const hits: Candidate[] = [];

  for (let i = 0; i < items.length; i += 1) {
    const pos = hay[i].indexOf(needle);
    if (pos < 0) continue;

    const [cn, jp, year, anilistId, bangumiId, py, pyi, pyi2, type] = items[i];
    const lc = cn.toLowerCase();
    const lj = jp.toLowerCase();

    let score: number;
    if (lc.startsWith(needle)) score = 130;
    else if (lc.includes(needle)) score = 110;
    else if (lg(py).startsWith(needle)) score = 90;
    else if (lg(pyi).startsWith(needle)) score = 80;
    else if (lg(pyi2).startsWith(needle)) score = 78;
    else if (lj.startsWith(needle)) score = 65;
    else score = 35;

    score -= pos * 0.2;
    score -= Math.min(cn.length || 12, 30) * 0.2;

    hits.push({ cn, jp, year, anilistId, bangumiId, type, score });
  }

  hits.sort((a, b) => b.score - a.score || (b.year || 0) - (a.year || 0));

  const seen = new Set<string>();
  const out: Candidate[] = [];
  for (const hit of hits) {
    const key = hit.anilistId
      ? `a${hit.anilistId}`
      : `${hit.cn}|${hit.jp}|${hit.year}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(hit);
    if (out.length >= limit) break;
  }
  return out;
}

function lg(s: string) {
  return s.toLowerCase();
}
