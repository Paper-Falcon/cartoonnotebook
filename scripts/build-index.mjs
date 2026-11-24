import { pinyin } from "pinyin-pro";
import fs from "node:fs/promises";
import path from "node:path";

const SRC = "https://unpkg.com/bangumi-data@latest/dist/data.json";
const CACHE = path.resolve(".cache/bangumi-data.json");
const OUT = path.resolve("src-tauri/assets/index.min.json");
const TYPES = new Set(["tv", "movie", "ova", "web"]);

const pyFull = (s) => pinyin(s, { toneType: "none", type: "array" }).join("");
const pyInitial = (s) => pinyin(s, { pattern: "first", toneType: "none", type: "array" }).join("");
// 去掉虚词再取首字母：进击的巨人 -> jjdr（多数人的输入习惯）
const DROP_PARTICLES = /[的之与和了在是们]/g;

async function load() {
  try {
    const cached = await fs.readFile(CACHE, "utf8");
    console.log("使用本地缓存", CACHE);
    return JSON.parse(cached);
  } catch {
    console.log("下载 bangumi-data …");
    const res = await fetch(SRC);
    if (!res.ok) throw new Error(`下载失败 ${res.status}`);
    const text = await res.text();
    await fs.mkdir(path.dirname(CACHE), { recursive: true });
    await fs.writeFile(CACHE, text);
    console.log(`已缓存 ${(text.length / 1024 / 1024).toFixed(1)} MB`);
    return JSON.parse(text);
  }
}

const data = await load();
const items = [];

// ---------- 国漫补充：Bangumi Archive 全量库（离线，不依赖代理） ----------
// 下载 https://github.com/bangumi/Archive/releases（tag archive）最新
// dump-*.zip 到 .cache/bgm-archive-dump.zip，从中提取 type=2 且 origin 含
// 中国的动画条目，与 bangumi-data 合并。输出 details 到
// src-tauri/assets/bgm-details.json（简介/集数/封面地址），供添加作品时离线补全。
import { execSync } from "node:child_process";

const exists = async (p) => {
  try { await fs.stat(p); return true; } catch { return false; }
};

async function loadArchiveCn() {
  const zip = path.resolve(".cache/bgm-archive-dump.zip");
  const lines = path.resolve(".cache/bgm-archive-subjects.jsonl");
  if (!(await exists(zip))) {
    console.log("没有 .cache/bgm-archive-dump.zip，跳过国漫合并（不影响日漫索引）");
    return [];
  }
  if (!(await exists(lines))) {
    console.log("解包 Archive dump（只取 subject.jsonlines，稍等）…");
    execSync(
      `python scripts/extract-archive.py "${zip}" "${lines}"`,
      { stdio: "inherit" },
    );
  }
  const text = await fs.readFile(lines, "utf8");
  const cn = [];
  for (const line of text.split("\n")) {
    if (!line) continue;
    let s;
    try { s = JSON.parse(line); } catch { continue; }
    if (s.type !== 2) continue; // 2 = 动画（国漫已由 extract-archive.py 按「中国」标签过滤）
    const nameCn = (s.name_cn || "").trim();
    const name = (s.name || "").trim();
    if (!nameCn && !name) continue;
    cn.push({
      id: s.id,
      cn: nameCn || name,
      jp: name,
      year: s.date ? Number(String(s.date).slice(0, 4)) || 0 : 0,
      eps: s.eps ?? null,
      summary: (s.summary || "").trim(),
      cover: s.images?.large || s.images?.common || s.images?.medium || "",
      platform: s.platform || "",
    });
  }
  console.log(`Archive 中国动画: ${cn.length} 条`);
  return cn;
}

const archiveCn = await loadArchiveCn();
const archiveById = new Map(archiveCn.map((x) => [x.id, x]));
const details = {};
for (const x of archiveCn) {
  details[x.id] = {
    cn: x.cn, jp: x.jp, year: x.year, eps: x.eps,
    summary: x.summary.slice(0, 1200), cover: x.cover,
  };
}
await fs.writeFile(
  path.resolve("src-tauri/assets/bgm-details.json"),
  JSON.stringify(details),
);
console.log(`bgm-details.json 已写出（${Object.keys(details).length} 条）`);

for (const it of data.items) {
  if (!TYPES.has(it.type)) continue;
  const cn = it.titleTranslate?.["zh-Hans"]?.[0] ?? "";
  const jp = it.title ?? "";
  if (!cn && !jp) continue;

  const aniList = it.sites?.find((s) => s.site === "aniList");
  const bangumi = it.sites?.find((s) => s.site === "bangumi");
  const year = it.begin ? Number(String(it.begin).slice(0, 4)) : 0;
  const base = cn || jp;

  items.push([
    cn || jp,
    jp,
    year || 0,
    aniList ? Number(aniList.id) : 0,
    bangumi ? Number(bangumi.id) : 0,
    pyFull(base),
    pyInitial(base),
    pyInitial(base.replace(DROP_PARTICLES, "")),
    it.type,
  ]);
}

// ---------- Archive 国漫并入索引（bangumiId 去重，已存在的跳过） ----------
const seenBgm = new Set(items.map((r) => r[4]).filter(Boolean));
let added = 0;
for (const x of archiveCn) {
  if (seenBgm.has(x.id)) continue;
  seenBgm.add(x.id);
  const base = x.cn || x.jp;
  items.push([
    base,
    x.jp,
    x.year || 0,
    0, // AniList 上未必有，置 0；详情走 bgm-details.json
    x.id,
    pyFull(base),
    pyInitial(base),
    pyInitial(base.replace(DROP_PARTICLES, "")),
    "tv",
  ]);
  added++;
}
console.log(`国漫并入索引: ${added} 条（索引总数 ${items.length}）`);

await fs.mkdir(path.dirname(OUT), { recursive: true });
const out = { v: 1, generatedAt: new Date().toISOString().slice(0, 10), items };
const text = JSON.stringify(out);
await fs.writeFile(OUT, text);

const withAniList = items.filter((x) => x[3] > 0).length;
const withCn = items.filter((x) => x[0]).length;
console.log(`写入 ${OUT}`);
console.log(`条目 ${items.length} · 有中文名 ${withCn} · 可补全 ${withAniList}`);
console.log(`文件大小 ${(text.length / 1024 / 1024).toFixed(2)} MB`);
