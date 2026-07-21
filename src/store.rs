use crate::model::{ItemType, TextUnit, Translation, WorkspaceMeta};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use std::collections::BTreeMap;
use std::path::Path;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(workspace: &Path) -> Result<Self> {
        std::fs::create_dir_all(workspace)?;
        let path = workspace.join("attx.db");
        let conn =
            Connection::open(&path).with_context(|| format!("open db {}", path.display()))?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS units (
                id TEXT PRIMARY KEY,
                engine TEXT NOT NULL,
                domain TEXT NOT NULL,
                location TEXT NOT NULL,
                item_type TEXT NOT NULL,
                role TEXT NOT NULL,
                original_lines TEXT NOT NULL,
                source_line_paths TEXT NOT NULL,
                context TEXT NOT NULL,
                payload TEXT NOT NULL,
                source_hash TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS translations (
                unit_id TEXT PRIMARY KEY,
                translation_lines TEXT NOT NULL,
                source_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            "#,
        )?;
        Ok(Self { conn })
    }

    pub fn set_meta(&self, meta: &WorkspaceMeta) -> Result<()> {
        let pairs = [
            ("engine", meta.engine.as_str()),
            ("game_path", meta.game_path.as_str()),
            ("content_root", meta.content_root.as_str()),
            ("source_lang", meta.source_lang.as_str()),
            ("target_lang", meta.target_lang.as_str()),
            ("created_at", meta.created_at.as_str()),
        ];
        for (k, v) in pairs {
            self.conn.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![k, v],
            )?;
        }
        Ok(())
    }

    pub fn meta(&self) -> Result<WorkspaceMeta> {
        let get = |k: &str| -> Result<String> {
            self.conn
                .query_row("SELECT value FROM meta WHERE key=?1", params![k], |r| {
                    r.get(0)
                })
                .with_context(|| format!("meta missing key {k}"))
        };
        Ok(WorkspaceMeta {
            engine: get("engine")?,
            game_path: get("game_path")?,
            content_root: get("content_root")?,
            source_lang: get("source_lang")?,
            target_lang: get("target_lang")?,
            created_at: get("created_at")?,
        })
    }

    pub fn replace_units(&self, units: &[TextUnit]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM units", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO units(id,engine,domain,location,item_type,role,original_lines,source_line_paths,context,payload,source_hash)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )?;
            for u in units {
                let lines = serde_json::to_string(&u.original_lines)?;
                let paths = serde_json::to_string(&u.source_line_paths)?;
                let hash = TextUnit::source_hash(&u.original_lines);
                stmt.execute(params![
                    u.id,
                    u.engine,
                    u.domain,
                    u.location,
                    u.item_type.as_str(),
                    u.role,
                    lines,
                    paths,
                    u.context,
                    u.payload,
                    hash,
                ])?;
            }
        }
        // Drop translations whose source_hash no longer matches
        tx.execute(
            "DELETE FROM translations WHERE unit_id NOT IN (SELECT id FROM units)
             OR source_hash NOT IN (SELECT source_hash FROM units WHERE units.id = translations.unit_id)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn all_units(&self) -> Result<Vec<TextUnit>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,engine,domain,location,item_type,role,original_lines,source_line_paths,context,payload FROM units ORDER BY domain, location",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, engine, domain, location, item_type, role, lines, paths, context, payload) =
                row?;
            out.push(TextUnit {
                id,
                engine,
                domain,
                location,
                item_type: ItemType::parse(&item_type),
                role,
                original_lines: serde_json::from_str(&lines)?,
                source_line_paths: serde_json::from_str(&paths)?,
                context,
                payload,
            });
        }
        Ok(out)
    }

    pub fn pending_units(&self) -> Result<Vec<TextUnit>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id,u.engine,u.domain,u.location,u.item_type,u.role,u.original_lines,u.source_line_paths,u.context,u.payload
             FROM units u
             LEFT JOIN translations t ON t.unit_id = u.id AND t.source_hash = u.source_hash
             WHERE t.unit_id IS NULL
             ORDER BY u.context, u.location",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, engine, domain, location, item_type, role, lines, paths, context, payload) =
                row?;
            out.push(TextUnit {
                id,
                engine,
                domain,
                location,
                item_type: ItemType::parse(&item_type),
                role,
                original_lines: serde_json::from_str(&lines)?,
                source_line_paths: serde_json::from_str(&paths)?,
                context,
                payload,
            });
        }
        Ok(out)
    }

    pub fn save_translation(&self, tr: &Translation) -> Result<()> {
        let lines = serde_json::to_string(&tr.translation_lines)?;
        let now = chrono_like_now();
        self.conn.execute(
            "INSERT INTO translations(unit_id, translation_lines, source_hash, updated_at)
             VALUES(?1,?2,?3,?4)
             ON CONFLICT(unit_id) DO UPDATE SET
               translation_lines=excluded.translation_lines,
               source_hash=excluded.source_hash,
               updated_at=excluded.updated_at",
            params![tr.unit_id, lines, tr.source_hash, now],
        )?;
        Ok(())
    }

    pub fn all_translations(&self) -> Result<BTreeMap<String, Translation>> {
        let mut stmt = self
            .conn
            .prepare("SELECT unit_id, translation_lines, source_hash FROM translations")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut map = BTreeMap::new();
        for row in rows {
            let (id, lines, hash) = row?;
            map.insert(
                id.clone(),
                Translation {
                    unit_id: id,
                    translation_lines: serde_json::from_str(&lines)?,
                    source_hash: hash,
                },
            );
        }
        Ok(map)
    }

    pub fn counts(&self) -> Result<(usize, usize, usize)> {
        let total: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM units", [], |r| r.get(0))?;
        let translated: usize = self.conn.query_row(
            "SELECT COUNT(*) FROM units u
             INNER JOIN translations t ON t.unit_id=u.id AND t.source_hash=u.source_hash",
            [],
            |r| r.get(0),
        )?;
        Ok((total, translated, total.saturating_sub(translated)))
    }
}

fn chrono_like_now() -> String {
    // avoid chrono dep: unix secs
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn workspace_db(workspace: &Path) -> Result<Store> {
    if !workspace.is_dir() {
        bail!("workspace not found: {}", workspace.display());
    }
    Store::open(workspace)
}
