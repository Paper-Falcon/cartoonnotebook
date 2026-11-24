import { useState } from "react";
import { STATUS_LABEL, Work, coverUrl } from "../lib/api";
import { formatLabel } from "../lib/search";

interface Props {
  work: Work;
  onClick?: () => void;
  selectable?: boolean;
  selected?: boolean;
  /** 显示拖拽把手（⠿），按住它拖动可排序 */
  dragEnabled?: boolean;
  /** 当前卡是否正被拖起 */
  dragging?: boolean;
  /** 当前卡是否是放置目标 */
  dropTarget?: boolean;
  /** 把手的 mousedown：从这里开始手动拖拽 */
  onGripDown?: (e: React.MouseEvent) => void;
}

export default function WorkCard({
  work,
  onClick,
  selectable,
  selected,
  dragEnabled,
  dragging,
  dropTarget,
  onGripDown,
}: Props) {
  const [broken, setBroken] = useState(false);
  const src = coverUrl(work.coverPath);
  const isMovie = work.format?.toLowerCase() === "movie";
  const total = work.episodesTotal ?? 0;

  const meta = [
    work.year ? String(work.year) : "",
    work.format ? formatLabel(work.format.toLowerCase()) : "",
    work.status === "watching" && work.progress > 0
      ? `${work.progress}/${total || "?"}`
      : !isMovie && total
        ? `${total} 集`
        : "",
    work.rating > 0 ? `${work.rating} 分` : "",
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <button
      className={`card${selectable ? " is-selectable" : ""}${selected ? " is-picked" : ""}${
        dragging ? " is-dragging" : ""
      }${dropTarget ? " is-drop-target" : ""}`}
      type="button"
      onClick={onClick}
      title={work.titleCn}
    >
      {dragEnabled && (
        <span
          className="card-grip"
          title="按住拖动调整顺序"
          onMouseDown={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onGripDown?.(e);
          }}
        >
          ⠿
        </span>
      )}
      <div className="cover">
        {src && !broken ? (
          <img
            src={src}
            alt={work.titleCn}
            loading="lazy"
            onError={() => setBroken(true)}
          />
        ) : (
          <div className="cover-fallback">{work.titleCn.slice(0, 1)}</div>
        )}
        <span className={`chip chip-${work.status}`}>
          {STATUS_LABEL[work.status] ?? work.status}
        </span>
        {selectable && <span className="pick-dot">{selected ? "✓" : ""}</span>}
      </div>
      <h3>{work.titleCn}</h3>
      <p>{meta || "—"}</p>
    </button>
  );
}
