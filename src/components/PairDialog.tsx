import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { PairInfo, allowFirewall, getFirewallStatus, getPairInfo } from "../lib/api";

interface Props {
  onClose: () => void;
  onToast: (msg: string) => void;
}

export default function PairDialog({ onClose, onToast }: Props) {
  const [info, setInfo] = useState<PairInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 防火墙放行状态：新电脑上没放行时给一键操作
  const [fwRule, setFwRule] = useState<boolean | null>(null);
  const [fwBusy, setFwBusy] = useState(false);

  function load() {
    setError(null);
    getPairInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
    getFirewallStatus()
      .then((r) => setFwRule(r.rule))
      .catch(() => setFwRule(false));
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

        {fwRule === false && (
          <div className="pair-fw">
            <span>
              首次使用需放行 Windows 防火墙（会弹一个系统确认框，点「是」）
            </span>
            <button
              className="pair-fw-btn"
              disabled={fwBusy}
              onClick={() => {
                setFwBusy(true);
                allowFirewall()
                  .catch(() => undefined)
                  .finally(() => {
                    // 等 UAC 确认完成后复查状态
                    window.setTimeout(() => {
                      getFirewallStatus()
                        .then((r) => {
                          setFwRule(r.rule);
                          if (r.rule) onToast("防火墙已放行");
                        })
                        .catch(() => undefined)
                        .finally(() => setFwBusy(false));
                    }, 2500);
                  });
              }}
            >
              {fwBusy ? "等待确认…" : "一键放行"}
            </button>
          </div>
        )}

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

            {(info.alternates?.length ?? 0) > 0 && (
              <>
                <div className="pair-alt-head">
                  主地址扫不出来？这台机器有多个网卡，试试备选：
                </div>
                <div className="pair-alts">
                  {(info.alternates ?? []).map((a) => (
                    <div className="pair-alt" key={a.ip}>
                      <div
                        className="pair-qr pair-qr-sm"
                        dangerouslySetInnerHTML={{ __html: a.qr }}
                      />
                      <div className="pair-alt-meta">
                        <div className="pair-alt-ip">{a.ip}</div>
                        <div className="pair-alt-adapter">{a.adapter}</div>
                        {a.adapter.startsWith("公网") && (
                          <div className="pair-alt-warn">
                            互联网可达，链接勿外传
                          </div>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </>
            )}
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
