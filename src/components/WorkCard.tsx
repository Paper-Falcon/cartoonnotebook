import { useState } from "react";
import { STATUS_LABEL, Work, coverUrl } from "../lib/api";
import { formatLabel } from "../lib/search";

interface Props {
  work: Work;
  onClick?: () => void;
  selectable?: boolean;
  selected?: boolean;
}

export default function WorkCard({ work, onClick, selectable, selected }: Props) {
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
      className={`card${selectable ? " is-selectable" : ""}${selected ? " is-picked" : ""}`}
      type="button"
      onClick={onClick}
      title={work.titleCn}
    >
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
