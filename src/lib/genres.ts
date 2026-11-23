/** AniList 的题材是英文枚举，统一在这里译成中文 */

export const GENRE_CN: Record<string, string> = {
  Action: "动作",
  Adventure: "冒险",
  Comedy: "喜剧",
  Drama: "剧情",
  Ecchi: "擦边",
  Fantasy: "奇幻",
  Horror: "恐怖",
  "Mahou Shoujo": "魔法少女",
  Mecha: "机甲",
  Music: "音乐",
  Mystery: "悬疑",
  Psychological: "心理",
  Romance: "恋爱",
  "Sci-Fi": "科幻",
  "Slice of Life": "日常",
  Sports: "运动",
  Supernatural: "超自然",
  Thriller: "惊悚",
};

export const genreLabel = (g: string) => GENRE_CN[g] ?? g;

export function parseGenres(raw: string | null | undefined): string[] {
  if (!raw) return [];
  try {
    const v = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}
