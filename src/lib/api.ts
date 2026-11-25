export interface Work {
  id: number;
  anilistId: number | null;
  bangumiId: number | null;
  /** 手动排序值（拖拽写入），0 = 未排过 */
  sortOrder: number;
  titleCn: string;
  titleJp: string | null;
  titleEn: string | null;
  aliases: string | null;
  year: number | null;
  format: string | null;
  episodesTotal: number | null;
  coverPath: string | null;
  synopsis: string | null;
  genres: string | null;
  status: string;
  rating: number;
  progress: number;
  favorite: number;
  finishedAt: string | null;
  note: string | null;
  mediaMode: string;
  createdAt: string;
  tags: string[];
}

export interface AddWorkPayload {
  anilistId?: number;
  bangumiId?: number;
  titleCn?: string;
  titleJp?: string;
  year?: number;
}

/**
 * 开发时页面来自 vite(5173)，/api 由 vite 代理到 8787，用相对路径即可。
 * 打包后页面来自 tauri://localhost，相对路径会打到 Tauri 的自定义协议上，
 * 必须显式指向本机服务；此时服务端的 CORS 已放开。
 */
export const API_BASE =
  import.meta.env.DEV || !("__TAURI_INTERNALS__" in window)
    ? ""
    : "http://127.0.0.1:8787";

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${url}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
  });
  if (!res.ok) {
    let detail = `${res.status}`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body.error) detail = body.error;
    } catch {
      /* 忽略非 JSON 错误体 */
    }
    throw new Error(detail);
  }
  return (await res.json()) as T;
}

export const listWorks = () => request<Work[]>("/api/works");

export interface Appearance {
  font: string;
  fontCustom: string;
  color: string;
}

export const getAppearance = () => request<Partial<Appearance>>("/api/appearance");

export const setAppearance = (a: Appearance) =>
  request<Partial<Appearance>>("/api/appearance", {
    method: "PUT",
    body: JSON.stringify(a),
  });

export interface BgmHit {
  id: number;
  name: string;
  nameCn: string;
  year: number | null;
  eps: number | null;
  cover: string | null;
}

/** Bangumi 在线搜索：国漫、日漫都能搜（网络不通时接口会报错，前端自行降级） */
export const bgmSearch = (q: string) =>
  request<BgmHit[]>(`/api/bgm/search?q=${encodeURIComponent(q)}`);

export const addWork = (payload: AddWorkPayload) =>
  request<Work>("/api/works", { method: "POST", body: JSON.stringify(payload) });

export const patchWork = (id: number, patch: Partial<Work>) =>
  request<Work>(`/api/works/${id}`, { method: "PATCH", body: JSON.stringify(patch) });

export const removeWork = (id: number) =>
  request<{ ok: boolean }>(`/api/works/${id}`, { method: "DELETE" });

export const coverUrl = (path: string | null | undefined) =>
  path ? `${API_BASE}/api/cover/${encodeURIComponent(path)}` : null;

// ---------------- 素材 ----------------

export interface Media {
  id: number;
  workId: number;
  episode: number | null;
  kind: "image" | "video" | string;
  storage: "ref" | "archive" | string;
  path: string;
  thumb: string | null;
  poster: string | null;
  bytes: number | null;
  duration: number | null;
  caption: string | null;
  stamp: number | null;
  missing: number;
  /** 上次播放到第几秒 */
  position: number;
  createdAt: string;
}

export const listMedia = (workId: number) =>
  request<Media[]>(`/api/works/${workId}/media`);

/** 按本机路径导入（拖拽场景）：归档由服务端直接复制，不经浏览器 */
export const importMedia = (workId: number, path: string, mode?: string) =>
  request<Media>(`/api/works/${workId}/media/import`, {
    method: "POST",
    body: JSON.stringify({ path, mode }),
  });

export interface UploadOptions {
  kind?: "image" | "video";
  stamp?: number;
  duration?: number;
  thumb?: Blob;
  poster?: Blob;
  filename?: string;
}

/** 上传内存里的 Blob（粘贴 / 文件选择 / 截帧） */
export async function uploadMedia(
  workId: number,
  blob: Blob,
  opts: UploadOptions = {},
): Promise<Media> {
  const fd = new FormData();
  fd.append("file", blob, opts.filename ?? "blob");
  if (opts.kind) fd.append("kind", opts.kind);
  if (opts.stamp != null) fd.append("stamp", String(opts.stamp));
  if (opts.duration != null) fd.append("duration", String(opts.duration));
  if (opts.thumb) fd.append("thumb", opts.thumb, "thumb.jpg");
  if (opts.poster) fd.append("poster", opts.poster, "poster.jpg");

  // 注意：不能带 Content-Type，浏览器要自己加 multipart boundary
  const res = await fetch(`${API_BASE}/api/works/${workId}/media/upload`, {
    method: "POST",
    body: fd,
  });
  if (!res.ok) {
    let detail = `${res.status}`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body.error) detail = body.error;
    } catch {
      /* 忽略 */
    }
    throw new Error(detail);
  }
  return (await res.json()) as Media;
}

export const mediaFileUrl = (id: number) => `${API_BASE}/api/media/${id}/file`;
export const mediaThumbUrl = (id: number) => `${API_BASE}/api/media/${id}/thumb`;

export const deleteMedia = (id: number) =>
  request<{ ok: boolean }>(`/api/media/${id}`, { method: "DELETE" });

export const patchMedia = (id: number, caption: string) =>
  request<Media>(`/api/media/${id}`, {
    method: "PATCH",
    body: JSON.stringify({ caption }),
  });

// ---------------- 手机配对 ----------------

export interface PairInfo {
  url: string;
  code: string;
  lanIp: string | null;
  port: number;
  qr: string;
  alternates?: { ip: string; adapter: string; url: string; qr: string }[];
}

export const getPairInfo = () => request<PairInfo>("/api/pair");

export const getFirewallStatus = () =>
  request<{ rule: boolean }>("/api/firewall");

export const allowFirewall = () =>
  request<{ spawned: boolean }>("/api/firewall", { method: "POST" });

// ---------------- 标签 ----------------

export interface Tag {
  id: number;
  name: string;
  count: number;
}

export const listTags = () => request<Tag[]>("/api/tags");

export const createTag = (name: string) =>
  request<Tag>("/api/tags", { method: "POST", body: JSON.stringify({ name }) });

export const renameTag = (id: number, name: string) =>
  request<Tag>(`/api/tags/${id}`, { method: "PATCH", body: JSON.stringify({ name }) });

export const deleteTag = (id: number) =>
  request<{ ok: boolean }>(`/api/tags/${id}`, { method: "DELETE" });

export const addWorkTag = (workId: number, name: string) =>
  request<string[]>(`/api/works/${workId}/tags`, {
    method: "POST",
    body: JSON.stringify({ name }),
  });

export const removeWorkTag = (workId: number, tagId: number) =>
  request<string[]>(`/api/works/${workId}/tags/${tagId}`, { method: "DELETE" });

export const patchMediaEpisode = (id: number, episode: number) =>
  request<Media>(`/api/media/${id}`, {
    method: "PATCH",
    body: JSON.stringify({ episode }),
  });

export const patchMediaPosition = (id: number, position: number) =>
  request<Media>(`/api/media/${id}`, {
    method: "PATCH",
    body: JSON.stringify({ position }),
  });

/** 引用模式的素材换路径（文件被移动后重新指回去） */
export const repointMedia = (id: number, path: string) =>
  request<Media>(`/api/media/${id}`, {
    method: "PATCH",
    body: JSON.stringify({ path }),
  });

// ---------------- 导出 ----------------

export interface ExportInfo {
  name: string;
  bytes: number;
  works: number;
  media: number;
  createdAt: string;
}

export const exportLibrary = () => request<ExportInfo>("/api/export");
export const exportDownloadUrl = () => `${API_BASE}/api/export/download`;

// ---------------- Wallpaper Engine 壁纸 ----------------

export interface Wallpaper {
  id: string;
  title: string;
  kind: "video" | "scene" | "web" | string;
  video: string | null;
  /** 网页壁纸的入口文件（由 /entry 接口注入 WE shim 后渲染） */
  entry: string | null;
  preview: string | null;
  tags: string[];
  bytes: number;
}

export interface WallpaperState {
  id: string | null;
  /** 临时开关：关掉但保留选择 */
  enabled: boolean;
  /** 薄纱浓度 0..1 */
  veil: number;
  /** 模糊像素 0..24 */
  blur: number;
  /** cover / contain */
  fit: string;
  title: string | null;
  kind: string | null;
  hasPreview: boolean;
  hasVideo: boolean;
  hasWeb: boolean;
  /** 本机确认播不出来、已降级静态的 id */
  unplayable: string[];
}

export const listWallpapers = (refresh = false) =>
  request<Wallpaper[]>(`/api/wallpapers${refresh ? "?refresh=1" : ""}`);

export const getWallpaperSetting = () => request<WallpaperState>("/api/wallpaper");

/** 只传要改的字段；清空背景传 id: "" */
export const setWallpaperSetting = (patch: {
  id?: string;
  enabled?: boolean;
  veil?: number;
  blur?: number;
  fit?: string;
  unplayable?: string[];
}) =>
  request<WallpaperState>("/api/wallpaper", {
    method: "PATCH",
    body: JSON.stringify(patch),
  });

export const wallpaperPreviewUrl = (id: string) =>
  `${API_BASE}/api/wallpapers/${id}/preview`;
export const wallpaperVideoUrl = (id: string) =>
  `${API_BASE}/api/wallpapers/${id}/video`;

export const wallpaperEntryUrl = (id: string) =>
  `${API_BASE}/api/wallpapers/${id}/entry`;

export const STATUS_LABEL: Record<string, string> = {
  plan: "想看",
  watching: "在看",
  done: "看完",
  paused: "搁置",
  dropped: "弃番",
};
