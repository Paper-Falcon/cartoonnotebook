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

await fs.mkdir(path.dirname(OUT), { recursive: true });
const out = { v: 1, generatedAt: new Date().toISOString().slice(0, 10), items };
const text = JSON.stringify(out);
await fs.writeFile(OUT, text);

const withAniList = items.filter((x) => x[3] > 0).length;
const withCn = items.filter((x) => x[0]).length;
console.log(`写入 ${OUT}`);
console.log(`条目 ${items.length} · 有中文名 ${withCn} · 可补全 ${withAniList}`);
console.log(`文件大小 ${(text.length / 1024 / 1024).toFixed(2)} MB`);
