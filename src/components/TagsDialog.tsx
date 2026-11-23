import { useState } from "react";
import { createPortal } from "react-dom";
import { Tag, createTag, deleteTag, renameTag } from "../lib/api";

interface Props {
  tags: Tag[];
  onClose: () => void;
  onChanged: () => void;
  onToast: (msg: string) => void;
}

export default function TagsDialog({ tags, onClose, onChanged, onToast }: Props) {
  const [rows, setRows] = useState<Tag[]>(tags);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);

  function sync(t: Tag | null, removedId?: number) {
    if (removedId != null) setRows((prev) => prev.filter((r) => r.id !== removedId));
    else if (t) setRows((prev) => prev.map((r) => (r.id === t.id ? t : r)));
    onChanged();
  }

  async function add() {
    const name = draft.trim();
    if (!name || busy) return;
    setBusy(true);
    try {
      const created = await createTag(name);
      setRows((prev) =>
        prev.some((r) => r.id === created.id) ? prev : [...prev, created],
      );
      setDraft("");
      onChanged();
    } catch (e) {
      onToast(`创建失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  async function rename(row: Tag, input: string) {
    const trimmed = input.trim();
    if (!trimmed || trimmed === row.name || busy) return;
    setBusy(true);
    try {
      const updated = await renameTag(row.id, trimmed);
      if (updated) {
        sync(updated);
        onToast("已重命名");
      } else {
        onToast("标签名不能为空");
      }
    } catch (e) {
      onToast(`重命名失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  async function remove(row: Tag) {
    const tip =
      row.count > 0 ? `「${row.name}」正在被 ${row.count} 部作品使用，删除后会全部摘掉。` : `删除空标签「${row.name}」？`;
    if (!window.confirm(tip)) return;
    setBusy(true);
    try {
      await deleteTag(row.id);
      sync(null, row.id);
      onToast("已删除");
    } catch (e) {
      onToast(`删除失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return createPortal(
    <div className="modal-backdrop" onClick={onClose}>
      <div className="pair-card wp-card" onClick={(e) => e.stopPropagation()}>
        <button className="lightbox-close" onClick={onClose} aria-label="关闭">
          ×
        </button>

        <h2>标签管理</h2>
        <p className="pair-sub">
          重命名或删除会同步到所有作品。删除一个被使用的标签，作品上的它会一并摘掉。
        </p>

        <div className="tag-create-row">
          <input
            className="tag-input"
            value={draft}
            placeholder="新建标签，回车确认"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void add();
              }
            }}
          />
          <button className="filter-toggle" onClick={() => void add()} disabled={!draft.trim() || busy}>
            新建
          </button>
        </div>

        <div className="tags-list">
          {rows.length === 0 ? (
            <div className="pair-loading">还没有标签</div>
          ) : (
            rows.map((row) => (
              <TagRow
                key={row.id}
                row={row}
                busy={busy}
                onRename={(name) => void rename(row, name)}
                onRemove={() => void remove(row)}
              />
            ))
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}

function TagRow({
  row,
  busy,
  onRename,
  onRemove,
}: {
  row: Tag;
  busy: boolean;
  onRename: (name: string) => void;
  onRemove: () => void;
}) {
  const [name, setName] = useState(row.name);
  const dirty = name.trim() !== row.name && name.trim().length > 0;

  return (
    <div className="tag-row-line">
      <input
        className="tag-name"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && dirty) onRename(name);
        }}
      />
      <span className="tag-line-count">{row.count}</span>
      <button
        className="filter-toggle"
        onClick={() => onRename(name)}
        disabled={!dirty || busy}
        title="重命名"
      >
        改名
      </button>
      <button className="filter-toggle is-danger" onClick={onRemove} disabled={busy} title="删除">
        删除
      </button>
    </div>
  );
}
