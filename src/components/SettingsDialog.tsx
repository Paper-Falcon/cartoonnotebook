import { useState } from "react";
import { Appearance } from "../lib/api";

interface Props {
  initial: Appearance;
  onClose: () => void;
  onSave: (a: Appearance) => Promise<void>;
}

const PRESET_FONTS = [
  { v: "", label: "系统默认" },
  { v: "微软雅黑", label: "微软雅黑" },
  { v: "等线", label: "等线" },
  { v: "黑体", label: "黑体" },
  { v: "宋体", label: "宋体" },
  { v: "楷体", label: "楷体" },
  { v: "仿宋", label: "仿宋" },
  { v: "Segoe UI", label: "Segoe UI" },
];

export default function SettingsDialog({ initial, onClose, onSave }: Props) {
  const [font, setFont] = useState(initial.font || "");
  const [fontCustom, setFontCustom] = useState(initial.fontCustom || "");
  const [color, setColor] = useState(initial.color || "");
  const [busy, setBusy] = useState(false);

  // 实时预览：自定义字体名优先
  const effFont = fontCustom.trim() || font;
  const effColor = color || "#1f1d1a";

  async function save() {
    if (busy) return;
    setBusy(true);
    try {
      await onSave({
        font: font.trim(),
        fontCustom: fontCustom.trim(),
        color,
      });
      onClose();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="settings-overlay"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="settings-panel" role="dialog" aria-label="外观设置">
        <div className="settings-head">
          <h2>外观设置</h2>
          <button className="settings-close" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>

        <div className="settings-body">
          <label className="settings-row">
            <span>界面字体</span>
            <select value={font} onChange={(e) => setFont(e.target.value)}>
              {PRESET_FONTS.map((f) => (
                <option key={f.v} value={f.v}>
                  {f.label}
                </option>
              ))}
            </select>
          </label>

          <label className="settings-row">
            <span>自定义字体名</span>
            <input
              type="text"
              value={fontCustom}
              onChange={(e) => setFontCustom(e.target.value)}
              placeholder="留空则用上面的选择；填了会优先"
              spellCheck={false}
            />
          </label>

          <label className="settings-row">
            <span>文字主色</span>
            <input
              type="color"
              value={effColor}
              onChange={(e) => setColor(e.target.value)}
            />
            <button
              className="settings-reset"
              onClick={() => setColor("")}
              disabled={!color}
            >
              恢复默认
            </button>
          </label>

          <div className="settings-preview">
            <div
              className="settings-preview-inner"
              style={{
                fontFamily: effFont ? `"${effFont}", sans-serif` : undefined,
                color: effColor,
              }}
            >
              <b>预览：档案馆标题</b>
              <p>
                正文文字效果。简介、标签、笔记都会跟随这里选择的字体与颜色。
              </p>
              <span className="dim">次要文字的层级对比。</span>
            </div>
          </div>
        </div>

        <div className="settings-foot">
          <button className="settings-cancel" onClick={onClose}>
            取消
          </button>
          <button className="settings-save" onClick={() => void save()}>
            {busy ? "保存中…" : "保存并应用"}
          </button>
        </div>
      </div>
    </div>
  );
}
