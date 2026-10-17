use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::model::{uti, Capture, ClipItem, ItemDetail, ItemKind, Payload, Query, Representation};
use crate::settings::Settings;

const SCHEMA_VERSION: i64 = 1;
/// Larger copies are not recorded; they would bloat the database.
pub const MAX_TEXT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const PREVIEW_CHARS: usize = 300;
const SEARCH_CHARS: usize = 20_000;
const DETAIL_TEXT_CHARS: usize = 200_000;
const THUMB_SIZE: u32 = 320;

const ITEM_COLUMNS: &str = "id, kind, created_at, last_used_at, source_bundle_id, source_name, \
     preview, byte_size, pinned, thumb_name, image_width, image_height, file_count";

/// Persistent clipboard history: a SQLite database plus a folder of image blobs.
pub struct Store {
    conn: Connection,
    blob_dir: PathBuf,
}

/// Result of recording a capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    New(i64),
    /// The same content was already in history; it moved to the top.
    Existing(i64),
}

impl Inserted {
    pub fn id(self) -> i64 {
        match self {
            Inserted::New(id) | Inserted::Existing(id) => id,
        }
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Store {
    /// Open or create the store inside `dir`.
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let blob_dir = dir.join("blobs");
        fs::create_dir_all(&blob_dir)?;
        let conn = Connection::open(dir.join("history.sqlite"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        let store = Self { conn, blob_dir };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS items (
                    id INTEGER PRIMARY KEY,
                    kind TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    last_used_at INTEGER NOT NULL,
                    source_bundle_id TEXT,
                    source_name TEXT,
                    hash TEXT NOT NULL UNIQUE,
                    preview TEXT NOT NULL,
                    search_text TEXT NOT NULL,
                    byte_size INTEGER NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0,
                    text TEXT,
                    blob_name TEXT,
                    blob_uti TEXT,
                    thumb_name TEXT,
                    image_width INTEGER,
                    image_height INTEGER,
                    files_json TEXT,
                    file_count INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS items_last_used ON items(last_used_at DESC);
                CREATE TABLE IF NOT EXISTS representations (
                    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
                    uti TEXT NOT NULL,
                    data BLOB NOT NULL,
                    PRIMARY KEY (item_id, uti)
                );
                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );",
            )?;
        }
        self.conn
            .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
        Ok(())
    }

    pub fn blob_dir(&self) -> &Path {
        &self.blob_dir
    }

    /// Record a capture. Returns `None` when there is nothing worth storing.
    pub fn insert(&mut self, capture: &Capture) -> Result<Option<Inserted>> {
        let Some(kind) = capture.kind() else {
            return Ok(None);
        };
        let hash = content_hash(kind, capture);
        let now = now_ms();

        let existing: Option<i64> = self
            .conn
            .query_row("SELECT id FROM items WHERE hash = ?1", [&hash], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE items SET last_used_at = ?2,
                    source_bundle_id = COALESCE(?3, source_bundle_id),
                    source_name = COALESCE(?4, source_name)
                 WHERE id = ?1",
                params![id, now, capture.source_bundle_id, capture.source_name],
            )?;
            return Ok(Some(Inserted::Existing(id)));
        }

        let mut text: Option<&str> = None;
        let mut blob_name = None;
        let mut blob_uti = None;
        let mut thumb_name = None;
        let mut dims: Option<(u32, u32)> = None;
        let mut files_json = None;
        let (preview, search_text, byte_size) = match kind {
            ItemKind::Text | ItemKind::RichText => {
                let t = capture.text.as_deref().unwrap_or_default();
                if t.len() > MAX_TEXT_BYTES {
                    return Ok(None);
                }
                text = Some(t);
                let size = t.len() + capture.rich.iter().map(|r| r.data.len()).sum::<usize>();
                (make_preview(t), make_search_text(t), size)
            }
            ItemKind::Files => {
                let joined = capture.files.join("\n");
                files_json = Some(serde_json::to_string(&capture.files)?);
                let names: Vec<&str> = capture.files.iter().map(|p| file_name(p)).collect();
                (
                    make_preview(&names.join(", ")),
                    make_search_text(&joined),
                    joined.len(),
                )
            }
            ItemKind::Image => {
                let img = capture.image.as_ref().expect("image kind has image");
                if img.data.len() > MAX_IMAGE_BYTES {
                    return Ok(None);
                }
                let ext = if img.uti == uti::PNG { "png" } else { "tiff" };
                let name = format!("{hash}.{ext}");
                fs::write(self.blob_dir.join(&name), &img.data)?;
                blob_name = Some(name);
                blob_uti = Some(img.uti.clone());
                match make_thumbnail(&img.data, &self.blob_dir.join(format!("{hash}-thumb.png"))) {
                    Ok(d) => {
                        dims = Some(d);
                        thumb_name = Some(format!("{hash}-thumb.png"));
                    }
                    Err(e) => tracing::warn!("thumbnail failed: {e:#}"),
                }
                let preview = match dims {
                    Some((w, h)) => format!("Image {w} x {h}"),
                    None => "Image".to_string(),
                };
                (preview, String::new(), img.data.len())
            }
        };

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO items (kind, created_at, last_used_at, source_bundle_id, source_name, hash,
                preview, search_text, byte_size, text, blob_name, blob_uti, thumb_name,
                image_width, image_height, files_json, file_count)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                kind.as_str(),
                now,
                capture.source_bundle_id,
                capture.source_name,
                hash,
                preview,
                search_text,
                byte_size as i64,
                text,
                blob_name,
                blob_uti,
                thumb_name,
                dims.map(|d| d.0),
                dims.map(|d| d.1),
                files_json,
                capture.files.len() as i64,
            ],
        )?;
        let id = tx.last_insert_rowid();
        if matches!(kind, ItemKind::RichText) {
            for rep in &capture.rich {
                tx.execute(
                    "INSERT OR REPLACE INTO representations (item_id, uti, data) VALUES (?1, ?2, ?3)",
                    params![id, rep.uti, rep.data],
                )?;
            }
        }
        tx.commit()?;
        Ok(Some(Inserted::New(id)))
    }

    pub fn list(&self, q: &Query) -> Result<Vec<ClipItem>> {
        let mut sql = format!("SELECT {ITEM_COLUMNS} FROM items WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        for word in q.text.split_whitespace() {
            args.push(word.to_lowercase().into());
            sql.push_str(&format!(" AND instr(search_text, ?{}) > 0", args.len()));
        }
        if let Some(kind) = q.kind {
            // Rich text is text as far as filtering goes.
            if kind == ItemKind::Text {
                sql.push_str(" AND kind IN ('text', 'richtext')");
            } else {
                args.push(kind.as_str().to_string().into());
                sql.push_str(&format!(" AND kind = ?{}", args.len()));
            }
        }
        if let Some(src) = &q.source_bundle_id {
            args.push(src.clone().into());
            sql.push_str(&format!(" AND source_bundle_id = ?{}", args.len()));
        }
        if q.pinned_only {
            sql.push_str(" AND pinned = 1");
        }
        let limit = if q.limit == 0 { 200 } else { q.limit.min(1000) };
        sql.push_str(&format!(
            " ORDER BY last_used_at DESC, id DESC LIMIT {limit} OFFSET {}",
            q.offset
        ));
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| self.row_to_item(r))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get(&self, id: i64) -> Result<Option<ClipItem>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ITEM_COLUMNS} FROM items WHERE id = ?1"),
                [id],
                |r| self.row_to_item(r),
            )
            .optional()?)
    }

    pub fn detail(&self, id: i64) -> Result<Option<ItemDetail>> {
        let Some(item) = self.get(id)? else {
            return Ok(None);
        };
        let (text, blob_name, files_json): (Option<String>, Option<String>, Option<String>) =
            self.conn.query_row(
                "SELECT text, blob_name, files_json FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
        let mut text_truncated = false;
        let text = text.map(|t| {
            if t.chars().count() > DETAIL_TEXT_CHARS {
                text_truncated = true;
                t.chars().take(DETAIL_TEXT_CHARS).collect()
            } else {
                t
            }
        });
        Ok(Some(ItemDetail {
            item,
            text,
            text_truncated,
            image_path: blob_name.map(|n| self.blob_path(&n)),
            files: parse_files(files_json.as_deref()),
        }))
    }

    pub fn payload(&self, id: i64) -> Result<Option<Payload>> {
        type PayloadRow = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let row: Option<PayloadRow> = self
            .conn
            .query_row(
                "SELECT text, blob_name, blob_uti, files_json FROM items WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((text, blob_name, blob_uti, files_json)) = row else {
            return Ok(None);
        };
        let mut stmt = self
            .conn
            .prepare("SELECT uti, data FROM representations WHERE item_id = ?1")?;
        let rich = stmt
            .query_map([id], |r| {
                Ok(Representation {
                    uti: r.get(0)?,
                    data: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let image = match (blob_name, blob_uti) {
            (Some(name), Some(uti)) => Some(Representation {
                uti,
                data: fs::read(self.blob_dir.join(name))?,
            }),
            _ => None,
        };
        Ok(Some(Payload {
            text,
            rich,
            image,
            files: parse_files(files_json.as_deref()),
        }))
    }

    /// Mark an item as just used so it moves to the top.
    pub fn touch(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET last_used_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET pinned = ?2 WHERE id = ?1",
            params![id, pinned],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let blobs = self.blob_names("WHERE id = ?1", params![id])?;
        self.conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
        self.remove_blobs(&blobs);
        Ok(())
    }

    /// Remove all history. Pinned items survive unless `include_pinned`.
    pub fn clear(&self, include_pinned: bool) -> Result<usize> {
        let filter = if include_pinned {
            ""
        } else {
            "WHERE pinned = 0"
        };
        let blobs = self.blob_names(filter, [])?;
        let n = self
            .conn
            .execute(&format!("DELETE FROM items {filter}"), [])?;
        self.remove_blobs(&blobs);
        Ok(n)
    }

    /// Apply retention limits to unpinned items. Returns the number removed.
    pub fn prune(&self, max_items: u32, max_age_days: u32) -> Result<usize> {
        let mut removed = 0;
        if max_age_days > 0 {
            let cutoff = now_ms() - i64::from(max_age_days) * 86_400_000;
            let filter = "WHERE pinned = 0 AND last_used_at < ?1";
            let blobs = self.blob_names(filter, params![cutoff])?;
            removed += self
                .conn
                .execute(&format!("DELETE FROM items {filter}"), params![cutoff])?;
            self.remove_blobs(&blobs);
        }
        let filter = "WHERE id IN (SELECT id FROM items WHERE pinned = 0
                       ORDER BY last_used_at DESC, id DESC LIMIT -1 OFFSET ?1)";
        let blobs = self.blob_names(filter, params![max_items])?;
        removed += self
            .conn
            .execute(&format!("DELETE FROM items {filter}"), params![max_items])?;
        self.remove_blobs(&blobs);
        Ok(removed)
    }

    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))?)
    }

    /// Distinct source apps, most recent first, for the filter menu.
    pub fn sources(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_bundle_id, COALESCE(MAX(source_name), source_bundle_id) FROM items
             WHERE source_bundle_id IS NOT NULL
             GROUP BY source_bundle_id ORDER BY MAX(last_used_at) DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn load_settings(&self) -> Result<Settings> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'settings'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default()
            .normalized())
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [serde_json::to_string(settings)?],
        )?;
        Ok(())
    }

    fn row_to_item(&self, r: &Row<'_>) -> rusqlite::Result<ClipItem> {
        let kind: String = r.get(1)?;
        let thumb: Option<String> = r.get(9)?;
        Ok(ClipItem {
            id: r.get(0)?,
            kind: ItemKind::parse(&kind).unwrap_or(ItemKind::Text),
            created_at: r.get(2)?,
            last_used_at: r.get(3)?,
            source_bundle_id: r.get(4)?,
            source_name: r.get(5)?,
            preview: r.get(6)?,
            byte_size: r.get(7)?,
            pinned: r.get(8)?,
            thumb_path: thumb.map(|n| self.blob_path(&n)),
            image_width: r.get(10)?,
            image_height: r.get(11)?,
            file_count: r.get(12)?,
        })
    }

    fn blob_path(&self, name: &str) -> String {
        self.blob_dir.join(name).to_string_lossy().into_owned()
    }

    fn blob_names(&self, filter: &str, args: impl rusqlite::Params) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT blob_name, thumb_name FROM items {filter}"))?;
        let rows = stmt.query_map(args, |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (a, b) = row?;
            out.extend(a);
            out.extend(b);
        }
        Ok(out)
    }

    fn remove_blobs(&self, names: &[String]) {
        for name in names {
            let _ = fs::remove_file(self.blob_dir.join(name));
        }
    }
}

fn content_hash(kind: ItemKind, c: &Capture) -> String {
    let mut h = blake3::Hasher::new();
    h.update(kind.as_str().as_bytes());
    h.update(&[0]);
    match kind {
        ItemKind::Text | ItemKind::RichText => {
            h.update(c.text.as_deref().unwrap_or_default().as_bytes());
        }
        ItemKind::Files => {
            for f in &c.files {
                h.update(f.as_bytes());
                h.update(&[0]);
            }
        }
        ItemKind::Image => {
            if let Some(img) = &c.image {
                h.update(&img.data);
            }
        }
    }
    h.finalize().to_hex()[..32].to_string()
}

fn make_preview(text: &str) -> String {
    let mut out = String::with_capacity(PREVIEW_CHARS);
    let mut count = 0;
    let mut last_space = true;
    for ch in text.chars() {
        let ch = if ch.is_whitespace() { ' ' } else { ch };
        if ch == ' ' && last_space {
            continue;
        }
        last_space = ch == ' ';
        out.push(ch);
        count += 1;
        if count >= PREVIEW_CHARS {
            out.push_str("...");
            break;
        }
    }
    out.trim_end().to_string()
}

fn make_search_text(text: &str) -> String {
    text.chars()
        .take(SEARCH_CHARS)
        .collect::<String>()
        .to_lowercase()
}

fn file_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

fn parse_files(json: Option<&str>) -> Vec<String> {
    json.and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default()
}

fn make_thumbnail(data: &[u8], out: &Path) -> Result<(u32, u32)> {
    let img = image::load_from_memory(data)?;
    let dims = (img.width(), img.height());
    img.thumbnail(THUMB_SIZE, THUMB_SIZE)
        .save_with_format(out, image::ImageFormat::Png)?;
    Ok(dims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Capture {
        Capture {
            text: Some(s.into()),
            ..Default::default()
        }
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn empty_capture_is_ignored() {
        let (_d, mut s) = open();
        assert_eq!(s.insert(&Capture::default()).unwrap(), None);
        assert_eq!(s.insert(&text("   \n")).unwrap(), None);
        assert_eq!(s.count().unwrap(), 0);
    }

    #[test]
    fn duplicates_move_to_top() {
        let (_d, mut s) = open();
        let a = s.insert(&text("alpha")).unwrap().unwrap();
        s.insert(&text("beta")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let again = s.insert(&text("alpha")).unwrap().unwrap();
        assert_eq!(again, Inserted::Existing(a.id()));
        let items = s.list(&Query::default()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].preview, "alpha");
    }

    #[test]
    fn search_matches_all_words_case_insensitive() {
        let (_d, mut s) = open();
        s.insert(&text("Hello World from Rust")).unwrap();
        s.insert(&text("hello there")).unwrap();
        s.insert(&text("Příliš žluťoučký kůň")).unwrap();
        let q = |t: &str| Query {
            text: t.into(),
            ..Default::default()
        };
        assert_eq!(s.list(&q("hello")).unwrap().len(), 2);
        assert_eq!(s.list(&q("rust HELLO")).unwrap().len(), 1);
        assert_eq!(s.list(&q("ŽLUŤOUČKÝ")).unwrap().len(), 1);
        assert_eq!(s.list(&q("missing")).unwrap().len(), 0);
    }

    #[test]
    fn rich_text_keeps_representations() {
        let (_d, mut s) = open();
        let cap = Capture {
            text: Some("bold".into()),
            rich: vec![Representation {
                uti: uti::RTF.into(),
                data: b"{\\rtf1 \\b bold}".to_vec(),
            }],
            ..Default::default()
        };
        let id = s.insert(&cap).unwrap().unwrap().id();
        let item = s.get(id).unwrap().unwrap();
        assert_eq!(item.kind, ItemKind::RichText);
        let p = s.payload(id).unwrap().unwrap();
        assert_eq!(p.text.as_deref(), Some("bold"));
        assert_eq!(p.rich.len(), 1);
        let filtered = s
            .list(&Query {
                kind: Some(ItemKind::Text),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn images_get_blob_and_thumbnail() {
        let (_d, mut s) = open();
        let data = png(800, 400);
        let cap = Capture {
            image: Some(Representation {
                uti: uti::PNG.into(),
                data: data.clone(),
            }),
            ..Default::default()
        };
        let id = s.insert(&cap).unwrap().unwrap().id();
        let item = s.get(id).unwrap().unwrap();
        assert_eq!(item.kind, ItemKind::Image);
        assert_eq!(
            (item.image_width, item.image_height),
            (Some(800), Some(400))
        );
        assert!(Path::new(item.thumb_path.as_ref().unwrap()).exists());
        let p = s.payload(id).unwrap().unwrap();
        assert_eq!(p.image.unwrap().data, data);
        s.delete(id).unwrap();
        assert!(!Path::new(item.thumb_path.as_ref().unwrap()).exists());
    }

    #[test]
    fn files_capture() {
        let (_d, mut s) = open();
        let cap = Capture {
            files: vec!["/Users/me/a.txt".into(), "/Users/me/Folder/".into()],
            text: Some("ignored".into()),
            ..Default::default()
        };
        let id = s.insert(&cap).unwrap().unwrap().id();
        let item = s.get(id).unwrap().unwrap();
        assert_eq!(item.kind, ItemKind::Files);
        assert_eq!(item.preview, "a.txt, Folder");
        assert_eq!(item.file_count, 2);
        assert_eq!(s.detail(id).unwrap().unwrap().files.len(), 2);
    }

    #[test]
    fn prune_keeps_pinned_and_newest() {
        let (_d, mut s) = open();
        let first = s.insert(&text("item 0")).unwrap().unwrap().id();
        s.set_pinned(first, true).unwrap();
        for i in 1..30 {
            s.insert(&text(&format!("item {i}"))).unwrap();
        }
        let removed = s.prune(10, 0).unwrap();
        assert_eq!(removed, 19);
        assert_eq!(s.count().unwrap(), 11);
        assert!(s.get(first).unwrap().is_some());
        assert_eq!(s.clear(false).unwrap(), 10);
        assert_eq!(s.count().unwrap(), 1);
    }

    #[test]
    fn settings_round_trip() {
        let (_d, s) = open();
        assert_eq!(s.load_settings().unwrap(), Settings::default());
        let st = Settings {
            max_items: 42,
            paused: true,
            ..Settings::default()
        };
        s.save_settings(&st).unwrap();
        assert_eq!(s.load_settings().unwrap(), st);
    }

    #[test]
    fn preview_collapses_whitespace() {
        assert_eq!(make_preview("  a\n\n b\t c  "), "a b c");
        let long = "x".repeat(1000);
        assert_eq!(make_preview(&long).chars().count(), PREVIEW_CHARS + 3);
    }
}
