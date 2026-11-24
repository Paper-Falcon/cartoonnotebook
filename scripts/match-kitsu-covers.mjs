// 把 bgm-details.json 里被墙的 lain.bgm.tv 封面地址换成 Kitsu 图床（可达）。
// 对每条国漫用中文名搜 Kitsu，标题校验通过才采用；结果缓存到
// .cache/kitsu-matches.json，可断点续跑。
import fs from "node:fs/promises";
import path from "node:path";

const DETAILS = path.resolve("src-tauri/assets/bgm-details.json");
const CACHE = path.resolve(".cache/kitsu-matches.json");
const CONCURRENCY = 3;

const norm = (s) =>
  (s || "")
    .toLowerCase()
    .replace(/[\s:：·\-—_（）()'"「」『』,.。!！?？|]/g, "");

function pickKitsuTitle(attrs) {
  const t = attrs.titles || {};
  return {
    zh: t.zh_cn || t.en_cn || "",
    jp: t.ja_jp || t.en_jp || "",
    en: t.en || "",
    canonical: (attrs.canonicalTitle || "").split("|")[0].trim(),
  };
}

/** 标题匹配：归一化后互相包含，或公共前缀 ≥3 字（处理「第2季」vs「2」这类差异） */
// 去掉季数/篇章标记再比：「灵笼 上半季」「魔道祖师 第2季」「魔道祖师 2」
// 都能归一成「灵笼」「魔道祖师」
const stripSeason = (s) => {
  let out = norm(s);
  for (let i = 0; i < 3; i++) {
    out = out
      .replace(/(第[0-9一二三四五六七八九十]+[季部篇章]|上半季|下半季|中章|终章|完结篇|特别篇|精编版|剧场版|劇場版|总集篇|OVA|season\d+|part\d+|[0-9ivx]+)$/, "")
      .replace(/^(剧场版|劇場版|总集篇|OVA|特别篇)/, "")
      .trim();
  }
  return out;
};

function titleMatch(query, attrs) {
  const q = norm(query);
  const q2 = stripSeason(query);
  if (!q) return 0;
  const t = pickKitsuTitle(attrs);
  let best = 0;
  for (const cand of [t.zh, t.jp, t.en, t.canonical]) {
    const c = norm(cand);
    const c2 = stripSeason(cand);
    if (!c) continue;
    if (c === q || c.includes(q) || q.includes(c)) best = Math.max(best, 3);
    else if (q2 && c2 && (c2 === q2 || c2.includes(q2) || q2.includes(c2)))
      best = Math.max(best, 2.5);
    else if (q2 && c2) {
      let plen = 0;
      while (plen < q2.length && plen < c2.length && q2[plen] === c2[plen]) plen++;
      if (plen >= 12) best = Math.max(best, 3);
      else if (plen >= 8) best = Math.max(best, 2.6);
      else if (plen >= 3) best = Math.max(best, 1.2);
    }
  }
  return best;
}

async function searchKitsu(q) {
  const url =
    "https://kitsu.io/api/edge/anime?page[limit]=5&filter[text]=" +
    encodeURIComponent(q);
  const r = await fetch(url, {
    headers: { Accept: "application/vnd.api+json", "User-Agent": "CartoonNotebook/0.1" },
    signal: AbortSignal.timeout(15000),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const d = await r.json();
  return (d.data || []).map((a) => ({
    attrs: a.attributes || {},
    poster: (a.attributes?.posterImage || {})?.large || (a.attributes?.posterImage || {})?.medium || (a.attributes?.posterImage || {})?.original || "",
  }));
}

async function main() {
  const details = JSON.parse(await fs.readFile(DETAILS, "utf8"));
  let cache = {};
  try {
    cache = JSON.parse(await fs.readFile(CACHE, "utf8"));
  } catch {}

  const entries = Object.entries(details);
  const todo = entries.filter(([id, d]) => {
    if (cache[id] !== undefined) return false; // 已处理过
    return !d.cover || d.cover.includes("bgm.tv"); // 需要换源
  });
  console.log(
    `共 ${entries.length} 条，缓存命中 ${entries.length - todo.length} 条，待匹配 ${todo.length} 条`,
  );

  let done = 0;
  let matched = 0;
  const ids = todo.map(([id]) => id);
  let cursor = 0;

  async function worker() {
    while (cursor < ids.length) {
      const id = ids[cursor++];
      const d = details[id];
      const q = d.cn || d.jp || "";
      let url = "";
      try {
        const results = await searchKitsu(q);
        let bestScore = 0;
        let bestPoster = "";
        for (const r of results) {
          if (!r.poster) continue;
          const score = titleMatch(q, r.attrs);
          if (score > bestScore) {
            bestScore = score;
            bestPoster = r.poster;
          }
        }
        // 校验不通过就不给封面（宁缺毋滥）
        url = bestScore >= 2.5 ? bestPoster : "";
        if (url) matched++;
      } catch (e) {
        console.log(`  #${id}「${q}」搜索失败：${e.message}，下次重试`);
        continue; // 不写缓存，下次重跑
      }
      cache[id] = url;
      done++;
      if (done % 200 === 0) {
        await fs.writeFile(CACHE, JSON.stringify(cache));
        console.log(`进度 ${done}/${ids.length}，已匹配 ${matched}`);
      }
      await new Promise((r) => setTimeout(r, 120));
    }
  }

  await Promise.all(Array.from({ length: CONCURRENCY }, worker));
  await fs.writeFile(CACHE, JSON.stringify(cache));

  // 回写 details：匹配到的换 Kitsu 地址，没匹配的清掉被墙地址
  let replaced = 0;
  for (const [id, d] of entries) {
    const c = cache[id];
    if (c === undefined) continue;
    if (c && d.cover !== c) replaced++;
    d.cover = c;
  }
  await fs.writeFile(DETAILS, JSON.stringify(details));
  console.log(`完成：匹配 ${matched}/${todo.length}，详情更新 ${replaced} 条封面地址`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
