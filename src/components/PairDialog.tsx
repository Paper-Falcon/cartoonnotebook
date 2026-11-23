import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { PairInfo, getPairInfo } from "../lib/api";

interface Props {
  onClose: () => void;
  onToast: (msg: string) => void;
}

export default function PairDialog({ onClose, onToast }: Props) {
  const [info, setInfo] = useState<PairInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  function load() {
    setError(null);
    getPairInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }

  useEffect(() => {
    load();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function copy() {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(info.url);
      onToast("链接已复制");
    } catch {
      onToast("复制失败，请手动选中链接");
    }
  }

  return createPortal(
    <div className="modal-backdrop" onClick={onClose}>
      <div className="pair-card" onClick={(e) => e.stopPropagation()}>
        <button className="lightbox-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        <h2>手机访问</h2>
        <p className="pair-sub">手机和电脑连同一个 Wi-Fi，扫码即可。一次配对，长期有效。</p>

        {error ? (
          <div className="pair-error">
            {error}
            <button className="btn-ghost" onClick={load}>
              重试
            </button>
          </div>
        ) : !info ? (
          <div className="pair-loading">正在生成…</div>
        ) : (
          <>
            <div
              className="pair-qr"
              // 服务端用 qrcode crate 生成的 SVG，内容是我们要跳转的地址
              dangerouslySetInnerHTML={{ __html: info.qr }}
            />

            <div className="pair-row">
              <span className="pair-label">地址</span>
              <code className="pair-url">{info.url}</code>
              <button className="btn-ghost" onClick={() => void copy()}>
                复制
              </button>
            </div>

            <div className="pair-row">
              <span className="pair-label">配对码</span>
              <span className="pair-code">{info.code}</span>
              <button className="btn-ghost" onClick={load}>
                换一个
              </button>
            </div>

            <p className="pair-note">
              {info.lanIp
                ? "二维码 5 分钟内有效，用一次就作废。电脑首次启动若弹出防火墙提示，需要允许「专用网络」。"
                : "没检测到局域网地址，检查一下电脑是否连着 Wi-Fi。"}
            </p>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
