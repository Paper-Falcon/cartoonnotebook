use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS works (
  id             INTEGER PRIMARY KEY,
  anilist_id     INTEGER UNIQUE,
  bangumi_id     INTEGER,
  title_cn       TEXT NOT NULL,
  title_jp       TEXT,
  title_en       TEXT,
  aliases        TEXT,
  year           INTEGER,
  format         TEXT,
  episodes_total INTEGER,
  cover_path     TEXT,
  synopsis       TEXT,
  genres         TEXT,
  status         TEXT NOT NULL DEFAULT 'plan',
  rating         INTEGER NOT NULL DEFAULT 0,
  progress       INTEGER NOT NULL DEFAULT 0,
  favorite       INTEGER NOT NULL DEFAULT 0,
  finished_at    TEXT,
  note           TEXT,
  media_mode     TEXT NOT NULL DEFAULT 'ref',
  created_at     TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE IF NOT EXISTS media (
  id          INTEGER PRIMARY KEY,
  work_id     INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
  episode     INTEGER,
  kind        TEXT NOT NULL,
  storage     TEXT NOT NULL DEFAULT 'ref',
  path        TEXT NOT NULL,
  thumb       TEXT,
  poster      TEXT,
  bytes       INTEGER,
  width       INTEGER,
  height      INTEGER,
  duration    REAL,
  caption     TEXT,
  stamp       REAL,
  missing     INTEGER NOT NULL DEFAULT 0,
  position    REAL NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE IF NOT EXISTS tags (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS work_tags (
  work_id INTEGER NOT NULL REFERENCES works(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (work_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_works_status  ON works(status);
CREATE INDEX IF NOT EXISTS idx_works_year    ON works(year);
CREATE INDEX IF NOT EXISTS idx_media_work    ON media(work_id);
"#;

const WORK_COLS: &str = "id, anilist_id, bangumi_id, title_cn, title_jp, title_en, aliases, year, \
     format, episodes_total, cover_path, synopsis, genres, status, rating, progress, favorite, \
     finished_at, note, media_mode, created_at";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub id: i64,
    pub anilist_id: Option<i64>,
    pub bangumi_id: Option<i64>,
    pub title_cn: String,
    pub title_jp: Option<String>,
    pub title_en: Option<String>,
    pub aliases: Option<String>,
    pub year: Option<i64>,
    pub format: Option<String>,
    pub episodes_total: Option<i64>,
    pub cover_path: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Option<String>,
    pub status: String,
    pub rating: i64,
    pub progress: i64,
    pub favorite: i64,
    pub finished_at: Option<String>,
    pub note: Option<String>,
    pub media_mode: String,
    pub created_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Work {
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Work {
            id: row.get("id")?,
            anilist_id: row.get("anilist_id")?,
            bangumi_id: row.get("bangumi_id")?,
            title_cn: row.get("title_cn")?,
            title_jp: row.get("title_jp")?,
            title_en: row.get("title_en")?,
            aliases: row.get("aliases")?,
            year: row.get("year")?,
            format: row.get("format")?,
            episodes_total: row.get("episodes_total")?,
            cover_path: row.get("cover_path")?,
            synopsis: row.get("synopsis")?,
            genres: row.get("genres")?,
            status: row.get("status")?,
            rating: row.get("rating")?,
            progress: row.get("progress")?,
            favorite: row.get("favorite")?,
            finished_at: row.get("finished_at")?,
            note: row.get("note")?,
            media_mode: row.get("media_mode")?,
            created_at: row.get("created_at")?,
            tags: Vec::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWork {
    pub anilist_id: Option<i64>,
    pub bangumi_id: Option<i64>,
    pub title_cn: String,
    pub title_jp: Option<String>,
    pub title_en: Option<String>,
    pub year: Option<i64>,
    pub format: Option<String>,
    pub episodes_total: Option<i64>,
    pub cover_path: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkPatch {
    pub title_cn: Option<String>,
    pub status: Option<String>,
    pub rating: Option<i64>,
    pub progress: Option<i64>,
    pub favorite: Option<i64>,
    pub finished_at: Option<String>,
    pub note: Option<String>,
    pub media_mode: Option<String>,
    pub synopsis: Option<String>,
}

pub fn open(dir: &Path) -> Result<Connection> {
    std::fs::create_dir_all(dir)?;
    let conn = Connection::open(dir.join("library.db"))?;
    conn.execute_batch(SCHEMA)?;
    // 早期版本的 media 表没有 thumb 列，老库在这里补齐（报错说明已存在，忽略）
    let _ = conn.execute("ALTER TABLE media ADD COLUMN thumb TEXT", []);
    let _ = conn.execute("ALTER TABLE media ADD COLUMN position REAL NOT NULL DEFAULT 0", []);
    Ok(conn)
}

pub fn list(conn: &Connection) -> Result<Vec<Work>> {
    let sql = format!(
        "SELECT {WORK_COLS} FROM works ORDER BY (finished_at IS NULL), finished_at DESC, id DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], Work::from_row)?;
    let mut works = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    attach_tags(conn, &mut works)?;
    Ok(works)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Work>> {
    let sql = format!("SELECT {WORK_COLS} FROM works WHERE id = ?1");
    let mut works: Vec<Work> = conn
        .query_row(&sql, params![id], Work::from_row)
        .optional()?
        .into_iter()
        .collect();
    attach_tags(conn, &mut works)?;
    Ok(works.pop())
}

/// 一次查询把全部作品的标签挂上，避免 N+1
fn attach_tags(conn: &Connection, works: &mut [Work]) -> Result<()> {
    if works.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT wt.work_id, t.name FROM work_tags wt
         JOIN tags t ON t.id = wt.tag_id ORDER BY t.name",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;

    let mut map: std::collections::HashMap<i64, Vec<String>> = std::collections::HashMap::new();
    for row in rows {
        let (work_id, name) = row?;
        map.entry(work_id).or_default().push(name);
    }
    for w in works.iter_mut() {
        if let Some(tags) = map.remove(&w.id) {
            w.tags = tags;
        }
    }
    Ok(())
}

pub fn find_by_anilist(conn: &Connection, anilist_id: i64) -> Result<Option<Work>> {
    let sql = format!("SELECT {WORK_COLS} FROM works WHERE anilist_id = ?1");
    Ok(conn
        .query_row(&sql, params![anilist_id], Work::from_row)
        .optional()?)
}

pub fn find_by_bangumi(conn: &Connection, bangumi_id: i64) -> Result<Option<Work>> {
    let sql = format!("SELECT {WORK_COLS} FROM works WHERE bangumi_id = ?1");
    Ok(conn
        .query_row(&sql, params![bangumi_id], Work::from_row)
        .optional()?)
}

pub fn insert(conn: &Connection, w: &NewWork) -> Result<Work> {
    conn.execute(
        "INSERT INTO works (anilist_id, bangumi_id, title_cn, title_jp, title_en, year, format,
                            episodes_total, cover_path, synopsis, genres)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            w.anilist_id,
            w.bangumi_id,
            w.title_cn,
            w.title_jp,
            w.title_en,
            w.year,
            w.format,
            w.episodes_total,
            w.cover_path,
            w.synopsis,
            w.genres
        ],
    )?;
    let id = conn.last_insert_rowid();
    Ok(get(conn, id)?.expect("刚插入的作品必须存在"))
}

pub fn update(conn: &Connection, id: i64, p: &WorkPatch) -> Result<Option<Work>> {
    let Some(mut cur) = get(conn, id)? else {
        return Ok(None);
    };
    if let Some(v) = &p.title_cn {
        cur.title_cn = v.clone();
    }
    if let Some(v) = &p.status {
        cur.status = v.clone();
    }
    if let Some(v) = p.rating {
        cur.rating = v;
    }
    if let Some(v) = p.progress {
        cur.progress = v;
    }
    if let Some(v) = p.favorite {
        cur.favorite = v;
    }
    if let Some(v) = &p.finished_at {
        // 前端清空日期时会传空串，这里统一落成 NULL
        cur.finished_at = if v.trim().is_empty() {
            None
        } else {
            Some(v.clone())
        };
    }
    if let Some(v) = &p.note {
        cur.note = Some(v.clone());
    }
    if let Some(v) = &p.media_mode {
        cur.media_mode = v.clone();
    }
    if let Some(v) = &p.synopsis {
        // 空串表示清掉简介
        cur.synopsis = if v.trim().is_empty() {
            None
        } else {
            Some(v.clone())
        };
    }

    // 标成"看完"却没有日期时，用当天兜底（不只依赖前端）
    if cur.status == "done" && cur.finished_at.is_none() {
        cur.finished_at = conn
            .query_row("SELECT date('now','localtime')", [], |r| r.get::<_, String>(0))
            .ok();
    }

    conn.execute(
        "UPDATE works SET title_cn = ?2, status = ?3, rating = ?4, progress = ?5, favorite = ?6,
                          finished_at = ?7, note = ?8, media_mode = ?9, synopsis = ?10
         WHERE id = ?1",
        params![
            id,
            cur.title_cn,
            cur.status,
            cur.rating,
            cur.progress,
            cur.favorite,
            cur.finished_at,
            cur.note,
            cur.media_mode,
            cur.synopsis
        ],
    )?;
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<bool> {
    let n = conn.execute("DELETE FROM works WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM works", [], |r| r.get(0))?)
}

// ---------------- 素材 ----------------

const MEDIA_COLS: &str = "id, work_id, episode, kind, storage, path, thumb, poster, bytes, \
     width, height, duration, caption, stamp, missing, position, created_at";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: i64,
    pub work_id: i64,
    pub episode: Option<i64>,
    pub kind: String,
    pub storage: String,
    pub path: String,
    pub thumb: Option<String>,
    pub poster: Option<String>,
    pub bytes: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration: Option<f64>,
    pub caption: Option<String>,
    pub stamp: Option<f64>,
    pub missing: i64,
    /// 上次播放到第几秒，用来续播
    pub position: f64,
    pub created_at: String,
}

impl Media {
    fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Media {
            id: row.get("id")?,
            work_id: row.get("work_id")?,
            episode: row.get("episode")?,
            kind: row.get("kind")?,
            storage: row.get("storage")?,
            path: row.get("path")?,
            thumb: row.get("thumb")?,
            poster: row.get("poster")?,
            bytes: row.get("bytes")?,
            width: row.get("width")?,
            height: row.get("height")?,
            duration: row.get("duration")?,
            caption: row.get("caption")?,
            stamp: row.get("stamp")?,
            missing: row.get("missing")?,
            position: row.get("position")?,
            created_at: row.get("created_at")?,
        })
    }
}

#[derive(Debug)]
pub struct NewMedia {
    pub work_id: i64,
    pub kind: String,
    pub storage: String,
    pub path: String,
    pub thumb: Option<String>,
    pub poster: Option<String>,
    pub bytes: Option<i64>,
    pub duration: Option<f64>,
    pub stamp: Option<f64>,
}

pub fn list_media(conn: &Connection, work_id: i64) -> Result<Vec<Media>> {
    let sql = format!(
        "SELECT {MEDIA_COLS} FROM media WHERE work_id = ?1 ORDER BY id DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![work_id], Media::from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_media(conn: &Connection, id: i64) -> Result<Option<Media>> {
    let sql = format!("SELECT {MEDIA_COLS} FROM media WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id], Media::from_row)
        .optional()?)
}

pub fn insert_media(conn: &Connection, m: &NewMedia) -> Result<Media> {
    conn.execute(
        "INSERT INTO media (work_id, kind, storage, path, thumb, poster, bytes, duration, stamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            m.work_id,
            m.kind,
            m.storage,
            m.path,
            m.thumb,
            m.poster,
            m.bytes,
            m.duration,
            m.stamp
        ],
    )?;
    let id = conn.last_insert_rowid();
    Ok(get_media(conn, id)?.expect("刚插入的素材必须存在"))
}

pub fn delete_media(conn: &Connection, id: i64) -> Result<bool> {
    let n = conn.execute("DELETE FROM media WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn set_media_caption(
    conn: &Connection,
    id: i64,
    caption: Option<String>,
) -> Result<Option<Media>> {
    conn.execute(
        "UPDATE media SET caption = ?2 WHERE id = ?1",
        params![id, caption],
    )?;
    get_media(conn, id)
}

pub fn set_media_missing(conn: &Connection, id: i64, missing: bool) -> Result<()> {
    conn.execute(
        "UPDATE media SET missing = ?2 WHERE id = ?1",
        params![id, i64::from(missing)],
    )?;
    Ok(())
}

// ---------------- 键值设置 ----------------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------------- 标签 ----------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

pub fn list_tags(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, COUNT(wt.work_id) AS n
         FROM tags t LEFT JOIN work_tags wt ON wt.tag_id = t.id
         GROUP BY t.id, t.name
         ORDER BY n DESC, t.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Tag {
            id: r.get(0)?,
            name: r.get(1)?,
            count: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn ensure_tag(conn: &Connection, name: &str) -> Result<i64> {
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![name])?;
    Ok(conn.query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| {
        r.get(0)
    })?)
}

pub fn tags_of(conn: &Connection, work_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM work_tags wt JOIN tags t ON t.id = wt.tag_id
         WHERE wt.work_id = ?1 ORDER BY t.name",
    )?;
    let rows = stmt.query_map(params![work_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn add_work_tag(conn: &Connection, work_id: i64, name: &str) -> Result<Vec<String>> {
    let name = name.trim();
    if name.is_empty() {
        return tags_of(conn, work_id);
    }
    let tag_id = ensure_tag(conn, name)?;
    conn.execute(
        "INSERT OR IGNORE INTO work_tags (work_id, tag_id) VALUES (?1, ?2)",
        params![work_id, tag_id],
    )?;
    tags_of(conn, work_id)
}

pub fn remove_work_tag(conn: &Connection, work_id: i64, tag_id: i64) -> Result<Vec<String>> {
    conn.execute(
        "DELETE FROM work_tags WHERE work_id = ?1 AND tag_id = ?2",
        params![work_id, tag_id],
    )?;
    tags_of(conn, work_id)
}

pub fn rename_tag(conn: &Connection, id: i64, name: &str) -> Result<Option<Tag>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    conn.execute("UPDATE tags SET name = ?2 WHERE id = ?1", params![id, name])?;
    Ok(conn
        .query_row(
            "SELECT t.id, t.name, (SELECT COUNT(*) FROM work_tags wt WHERE wt.tag_id = t.id)
             FROM tags t WHERE t.id = ?1",
            params![id],
            |r| {
                Ok(Tag {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    count: r.get(2)?,
                })
            },
        )
        .optional()?)
}

pub fn delete_tag(conn: &Connection, id: i64) -> Result<bool> {
    let n = conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn set_media_episode(conn: &Connection, id: i64, episode: Option<i64>) -> Result<Option<Media>> {
    conn.execute(
        "UPDATE media SET episode = ?2 WHERE id = ?1",
        params![id, episode],
    )?;
    get_media(conn, id)
}

/// 导出用的紧凑清单：包含引用模式的原始路径，方便换机后重连
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRef {
    pub id: i64,
    pub work_id: i64,
    pub title_cn: String,
    pub kind: String,
    pub storage: String,
    pub path: String,
    pub episode: Option<i64>,
    pub missing: i64,
}

pub fn list_media_refs(conn: &Connection) -> Result<Vec<MediaRef>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.work_id, w.title_cn, m.kind, m.storage, m.path, m.episode, m.missing
         FROM media m JOIN works w ON w.id = m.work_id ORDER BY m.work_id, m.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MediaRef {
            id: r.get(0)?,
            work_id: r.get(1)?,
            title_cn: r.get(2)?,
            kind: r.get(3)?,
            storage: r.get(4)?,
            path: r.get(5)?,
            episode: r.get(6)?,
            missing: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn set_media_position(conn: &Connection, id: i64, position: f64) -> Result<()> {
    conn.execute(
        "UPDATE media SET position = ?2 WHERE id = ?1",
        params![id, position],
    )?;
    Ok(())
}

/// 引用模式的素材改路径：只更新登记信息，不搬文件
pub fn set_media_path(
    conn: &Connection,
    id: i64,
    path: &str,
    bytes: Option<i64>,
    kind: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE media SET path = ?2, bytes = ?3, kind = ?4, missing = 0 WHERE id = ?1",
        params![id, path, bytes, kind],
    )?;
    Ok(())
}
