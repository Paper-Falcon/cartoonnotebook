/** 图片缩略图、视频首帧、视频截帧——全部在浏览器里用 canvas 完成，不依赖 ffmpeg */

export async function makeThumb(file: Blob, max = 480): Promise<Blob> {
  const url = URL.createObjectURL(file);
  try {
    const img = new Image();
    img.src = url;
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = () => reject(new Error("图片无法解析"));
    });
    const scale = Math.min(1, max / Math.max(img.width, img.height));
    const w = Math.max(1, Math.round(img.width * scale));
    const h = Math.max(1, Math.round(img.height * scale));
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    canvas.getContext("2d")!.drawImage(img, 0, 0, w, h);
    return await toJpeg(canvas, 0.86);
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** 取视频约 1 秒处的画面做封面，同时拿到时长与分辨率 */
export async function extractVideoPoster(
  file: Blob,
): Promise<{ poster: Blob; duration: number; width: number; height: number }> {
  const url = URL.createObjectURL(file);
  const video = document.createElement("video");
  try {
    video.src = url;
    video.muted = true;
    video.preload = "metadata";

    await new Promise<void>((resolve, reject) => {
      video.onloadedmetadata = () => resolve();
      video.onerror = () => reject(new Error("视频格式无法解析"));
    });

    const duration = Number.isFinite(video.duration) ? video.duration : 0;
    video.currentTime = Math.min(1, Math.max(0.05, duration * 0.1));
    await new Promise<void>((resolve) => {
      video.onseeked = () => resolve();
    });

    const w = video.videoWidth || 640;
    const h = video.videoHeight || 360;
    const scale = Math.min(1, 640 / Math.max(w, h));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(w * scale));
    canvas.height = Math.max(1, Math.round(h * scale));
    canvas.getContext("2d")!.drawImage(video, 0, 0, canvas.width, canvas.height);

    return {
      poster: await toJpeg(canvas, 0.85),
      duration,
      width: w,
      height: h,
    };
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** 截当前帧 */
export async function captureFrame(video: HTMLVideoElement): Promise<Blob> {
  const w = video.videoWidth;
  const h = video.videoHeight;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  canvas.getContext("2d")!.drawImage(video, 0, 0, w, h);
  return await toJpeg(canvas, 0.92);
}

function toJpeg(canvas: HTMLCanvasElement, quality: number): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error("画面转换失败"))),
      "image/jpeg",
      quality,
    );
  });
}

export const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function formatBytes(bytes: number | null): string {
  if (!bytes) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

export function formatTime(sec: number | null): string {
  if (sec == null || !Number.isFinite(sec)) return "";
  const s = Math.max(0, Math.floor(sec));
  const m = Math.floor(s / 60);
  const r = s % 60;
  return `${m}:${String(r).padStart(2, "0")}`;
}
