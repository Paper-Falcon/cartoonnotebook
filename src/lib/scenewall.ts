import { API_BASE } from "./api";

type AnyLib = {
  mount: (el: HTMLElement, opts: Record<string, unknown>) => Promise<AnyInstance>;
  httpSource: (base: string) => unknown;
};

type AnyInstance = {
  destroy: (opts?: unknown) => void;
  pause: () => void;
  resume: () => void;
};

let loader: Promise<AnyLib> | null = null;

/** 按需加载 WebWallGL（UMD 版，挂在 window.WebWallGL 上） */
export function loadWebWallGL(): Promise<AnyLib> {
  const w = window as unknown as { WebWallGL?: AnyLib };
  if (w.WebWallGL) return Promise.resolve(w.WebWallGL);
  if (!loader) {
    loader = new Promise<AnyLib>((resolve, reject) => {
      const el = document.createElement("script");
      el.src = `${API_BASE}/vendor/webwallgl.global.min.js`;
      el.onload = () => {
        if (w.WebWallGL) resolve(w.WebWallGL);
        else reject(new Error("WebWallGL 加载后未暴露全局对象"));
      };
      el.onerror = () => {
        loader = null;
        reject(new Error("WebWallGL 脚本加载失败"));
      };
      document.head.appendChild(el);
    });
  }
  return loader;
}

export interface SceneHandle {
  destroy: () => void;
}

/**
 * 把一张场景壁纸挂到容器里（WebWallGL 实时渲染）。
 * 引擎会自己取 project.json / scene.pkg / 素材，失败时抛错由调用方兜底成静态图。
 */
export async function mountScene(
  container: HTMLElement,
  id: string,
  fit: "cover" | "contain",
): Promise<SceneHandle> {
  const lib = await loadWebWallGL();
  const inst = (await lib.mount(container, {
    source: lib.httpSource(`${API_BASE}/api/wallpapers/${id}/file/`),
    fit,
    fps: 30,
    volume: 0,
  })) as AnyInstance;
  return {
    destroy: () => {
      try {
        inst.destroy();
      } catch {
        // 引擎在窗口关闭竞态下可能已自毁，忽略
      }
    },
  };
}
