//! SQLite persistence for transcript history. One database in the app data
//! dir; dictation results are written from day one so history exists before
//! its UI grows richer. Core functions take `&Connection` so tests run on an
//! in-memory database without Tauri.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::settings::SettingsState;

pub struct Db(pub Mutex<Connection>);

pub const PAGE_SIZE: u32 = 50;

/// Numbered migrations; each runs in its own transaction and bumps
/// `meta.schema_version`. Append-only — never edit a shipped entry.
const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        "
    CREATE TABLE transcripts (
        id INTEGER PRIMARY KEY,
        kind TEXT NOT NULL CHECK (kind IN ('dictation','file','meeting')),
        created_at INTEGER NOT NULL,
        duration_ms INTEGER NOT NULL DEFAULT 0,
        model TEXT,
        language TEXT,
        profile_id TEXT,
        text TEXT NOT NULL,
        translated_text TEXT,
        translation_provider TEXT
    );
    CREATE INDEX transcripts_created_at ON transcripts(created_at DESC);

    CREATE VIRTUAL TABLE transcripts_fts USING fts5(
        text,
        translated_text,
        content='transcripts',
        content_rowid='id',
        tokenize='unicode61 remove_diacritics 2'
    );

    CREATE TRIGGER transcripts_ai AFTER INSERT ON transcripts BEGIN
        INSERT INTO transcripts_fts(rowid, text, translated_text)
        VALUES (new.id, new.text, coalesce(new.translated_text, ''));
    END;
    CREATE TRIGGER transcripts_ad AFTER DELETE ON transcripts BEGIN
        INSERT INTO transcripts_fts(transcripts_fts, rowid, text, translated_text)
        VALUES ('delete', old.id, old.text, coalesce(old.translated_text, ''));
    END;
    CREATE TRIGGER transcripts_au AFTER UPDATE ON transcripts BEGIN
        INSERT INTO transcripts_fts(transcripts_fts, rowid, text, translated_text)
        VALUES ('delete', old.id, old.text, coalesce(old.translated_text, ''));
        INSERT INTO transcripts_fts(rowid, text, translated_text)
        VALUES (new.id, new.text, coalesce(new.translated_text, ''));
    END;
    ",
    ),
    (
        2,
        "
    CREATE TABLE segments (
        transcript_id INTEGER NOT NULL,
        idx INTEGER NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        speaker TEXT,
        text TEXT NOT NULL,
        PRIMARY KEY (transcript_id, idx)
    );
    ALTER TABLE transcripts ADD COLUMN source_path TEXT;
    ",
    ),
];

impl Db {
    pub fn open_default(app: &AppHandle) -> Result<Self, String> {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Self::open(dir.join("speakly.db"))
    }

    pub fn open(path: PathBuf) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        migrate(&conn).map_err(|e| e.to_string())?;
        Ok(Self(Mutex::new(conn)))
    }
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT)")?;
    let current: i64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    for (version, sql) in MIGRATIONS {
        if *version > current {
            conn.execute_batch(&format!(
                "BEGIN;\n{sql}\nINSERT OR REPLACE INTO meta(key, value) VALUES ('schema_version', '{version}');\nCOMMIT;"
            ))?;
        }
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
pub fn insert_transcript(
    conn: &Connection,
    kind: &str,
    profile_id: Option<&str>,
    model: Option<&str>,
    language: Option<&str>,
    duration_ms: i64,
    text: &str,
    translated: Option<(&str, &str)>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO transcripts
            (kind, created_at, duration_ms, model, language, profile_id, text, translated_text, translation_provider)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            kind,
            now_ms(),
            duration_ms,
            model,
            language,
            profile_id,
            text,
            translated.map(|(t, _)| t),
            translated.map(|(_, p)| p),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Turn free text into an FTS5 prefix query: each whitespace token becomes a
/// quoted `"token"*` term (quotes doubled), implicitly ANDed.
fn fts_expr(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

pub fn search(
    conn: &Connection,
    query: Option<&str>,
    kind: Option<&str>,
    page: u32,
) -> rusqlite::Result<Value> {
    let offset = page * PAGE_SIZE;
    let limit = PAGE_SIZE + 1;
    let row_to_json = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Value> {
        Ok(json!({
            "id": r.get::<_, i64>(0)?,
            "kind": r.get::<_, String>(1)?,
            "createdAt": r.get::<_, i64>(2)?,
            "durationMs": r.get::<_, i64>(3)?,
            "language": r.get::<_, Option<String>>(4)?,
            "text": r.get::<_, String>(5)?,
            "translatedText": r.get::<_, Option<String>>(6)?,
        }))
    };

    let expr = query.and_then(fts_expr);
    let mut items: Vec<Value> = match &expr {
        Some(expr) => {
            let mut stmt = conn.prepare(
                "SELECT t.id, t.kind, t.created_at, t.duration_ms, t.language, t.text, t.translated_text
                 FROM transcripts_fts f JOIN transcripts t ON t.id = f.rowid
                 WHERE transcripts_fts MATCH ?1 AND (?4 IS NULL OR t.kind = ?4)
                 ORDER BY t.created_at DESC, t.id DESC LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt.query_map(params![expr, limit, offset, kind], row_to_json)?;
            rows.collect::<rusqlite::Result<_>>()?
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, kind, created_at, duration_ms, language, text, translated_text
                 FROM transcripts
                 WHERE (?3 IS NULL OR kind = ?3)
                 ORDER BY created_at DESC, id DESC LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![limit, offset, kind], row_to_json)?;
            rows.collect::<rusqlite::Result<_>>()?
        }
    };

    let has_more = items.len() as u32 > PAGE_SIZE;
    items.truncate(PAGE_SIZE as usize);
    Ok(json!({ "items": items, "hasMore": has_more }))
}

/// Delete transcripts older than `days` days. Returns the number removed.
pub fn purge_older_than(conn: &Connection, days: u32) -> rusqlite::Result<usize> {
    let cutoff = now_ms() - (days as i64) * 86_400_000;
    conn.execute(
        "DELETE FROM transcripts WHERE created_at < ?1",
        params![cutoff],
    )
}

/// Honor the retention setting; no-op when retention is off or the DB is
/// unavailable. Called at startup and daily.
pub fn run_retention_purge(app: &AppHandle) {
    let days = {
        let state = app.state::<SettingsState>();
        let settings = state.0.lock().unwrap();
        if !settings.history.enabled {
            return;
        }
        settings.history.retention_days
    };
    let Some(days) = days else { return };
    let Some(db) = app.try_state::<Db>() else {
        return;
    };
    let conn = db.0.lock().unwrap();
    match purge_older_than(&conn, days) {
        Ok(n) if n > 0 => tracing::info!("retention purge removed {n} transcripts"),
        Err(e) => tracing::warn!("retention purge failed: {e}"),
        _ => {}
    }
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM segments WHERE transcript_id = ?1", params![id])?;
    conn.execute("DELETE FROM transcripts WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM segments", [])?;
    conn.execute("DELETE FROM transcripts", [])?;
    Ok(())
}

/// Timestamped (and later speaker-labeled) pieces of a file/meeting transcript.
pub fn insert_segments(
    conn: &Connection,
    transcript_id: i64,
    segments: &[speakly_engine_types::Segment],
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO segments (transcript_id, idx, start_ms, end_ms, speaker, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (idx, s) in segments.iter().enumerate() {
        stmt.execute(params![
            transcript_id,
            idx as i64,
            s.start_ms as i64,
            s.end_ms as i64,
            s.speaker,
            s.text,
        ])?;
    }
    Ok(())
}

/// Rename a speaker label across one transcript's segments.
pub fn rename_speaker(
    conn: &Connection,
    transcript_id: i64,
    from: &str,
    to: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE segments SET speaker = ?3 WHERE transcript_id = ?1 AND speaker = ?2",
        params![transcript_id, from, to],
    )
}

pub fn set_source_path(conn: &Connection, id: i64, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE transcripts SET source_path = ?2 WHERE id = ?1",
        params![id, path],
    )?;
    Ok(())
}

/// Timestamped segments of a file/meeting transcript, in order.
pub fn segments_for(conn: &Connection, transcript_id: i64) -> rusqlite::Result<Vec<Value>> {
    let mut stmt = conn.prepare(
        "SELECT start_ms, end_ms, speaker, text FROM segments
         WHERE transcript_id = ?1 ORDER BY idx",
    )?;
    let rows = stmt.query_map(params![transcript_id], |r| {
        Ok(json!({
            "startMs": r.get::<_, i64>(0)?,
            "endMs": r.get::<_, i64>(1)?,
            "speaker": r.get::<_, Option<String>>(2)?,
            "text": r.get::<_, String>(3)?,
        }))
    })?;
    rows.collect()
}

/// Persist a finished dictation off the calling thread. Honors the history
/// settings; silently no-ops when the DB failed to open.
pub fn persist_dictation(
    app: &AppHandle,
    profile_id: &str,
    text: &str,
    utterance_ms: u64,
    translated: Option<(String, String)>,
) {
    let (enabled, model, language) = {
        let state = app.state::<SettingsState>();
        let settings = state.0.lock().unwrap();
        let enabled = settings.history.enabled && settings.history.save_dictation;
        let (model, language) = settings
            .profile(profile_id)
            .map(|p| (p.model_id.clone(), p.language.clone()))
            .unwrap_or_default();
        (enabled, model, language)
    };
    if !enabled {
        return;
    }
    let app = app.clone();
    let profile_id = profile_id.to_string();
    let text = text.to_string();
    std::thread::spawn(move || {
        let Some(db) = app.try_state::<Db>() else {
            return;
        };
        let conn = db.0.lock().unwrap();
        if let Err(e) = insert_transcript(
            &conn,
            "dictation",
            Some(&profile_id),
            Some(&model),
            Some(&language),
            utterance_ms as i64,
            &text,
            translated.as_ref().map(|(t, p)| (t.as_str(), p.as_str())),
        ) {
            tracing::warn!("history insert failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn add(conn: &Connection, text: &str) -> i64 {
        insert_transcript(
            conn,
            "dictation",
            Some("he"),
            Some("he-turbo"),
            Some("he"),
            1200,
            text,
            None,
        )
        .unwrap()
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = mem();
        migrate(&conn).unwrap();
        let v: String = conn
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "2");
    }

    #[test]
    fn segments_round_trip_and_cascade_on_delete() {
        let conn = mem();
        let id = add(&conn, "with segments");
        let segs = vec![
            speakly_engine_types::Segment {
                start_ms: 0,
                end_ms: 1500,
                speaker: None,
                text: "שלום".into(),
            },
            speakly_engine_types::Segment {
                start_ms: 1500,
                end_ms: 3000,
                speaker: None,
                text: "עולם".into(),
            },
        ];
        insert_segments(&conn, id, &segs).unwrap();
        set_source_path(&conn, id, "/tmp/a.mp3").unwrap();
        let fetched = segments_for(&conn, id).unwrap();
        assert_eq!(fetched.len(), 2);
        assert_eq!(fetched[0]["text"], "שלום");
        assert_eq!(fetched[1]["startMs"], 1500);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        let sp: String = conn
            .query_row(
                "SELECT source_path FROM transcripts WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sp, "/tmp/a.mp3");
        delete(&conn, id).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM segments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn search_filters_by_kind() {
        let conn = mem();
        add(&conn, "רשומת הכתבה");
        insert_transcript(
            &conn,
            "file",
            None,
            None,
            Some("he"),
            9000,
            "תמלול קובץ",
            None,
        )
        .unwrap();

        let all = search(&conn, None, None, 0).unwrap();
        assert_eq!(all["items"].as_array().unwrap().len(), 2);

        let files = search(&conn, None, Some("file"), 0).unwrap();
        let items = files["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"], "file");

        // Kind filter composes with FTS.
        let hits = search(&conn, Some("תמלול"), Some("dictation"), 0).unwrap();
        assert_eq!(hits["items"].as_array().unwrap().len(), 0);
        let hits = search(&conn, Some("תמלול"), Some("file"), 0).unwrap();
        assert_eq!(hits["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn hebrew_search_matches_prefix() {
        let conn = mem();
        add(&conn, "שלום עולם, זוהי בדיקה");
        add(&conn, "an english entry");
        let out = search(&conn, Some("שלום"), None, 0).unwrap();
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0]["text"].as_str().unwrap().contains("שלום עולם"));
        // Prefix of a longer word also matches.
        let out = search(&conn, Some("בדיק"), None, 0).unwrap();
        assert_eq!(out["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn pagination_reports_has_more() {
        let conn = mem();
        for i in 0..(PAGE_SIZE + 10) {
            add(&conn, &format!("entry number {i}"));
        }
        let page0 = search(&conn, None, None, 0).unwrap();
        assert_eq!(page0["items"].as_array().unwrap().len(), PAGE_SIZE as usize);
        assert!(page0["hasMore"].as_bool().unwrap());
        let page1 = search(&conn, None, None, 1).unwrap();
        assert_eq!(page1["items"].as_array().unwrap().len(), 10);
        assert!(!page1["hasMore"].as_bool().unwrap());
    }

    #[test]
    fn delete_and_clear_remove_from_fts() {
        let conn = mem();
        let id = add(&conn, "מחיקה ראשונה");
        add(&conn, "מחיקה שניה");
        delete(&conn, id).unwrap();
        let out = search(&conn, Some("מחיקה"), None, 0).unwrap();
        assert_eq!(out["items"].as_array().unwrap().len(), 1);
        clear(&conn).unwrap();
        let out = search(&conn, Some("מחיקה"), None, 0).unwrap();
        assert_eq!(out["items"].as_array().unwrap().len(), 0);
        let out = search(&conn, None, None, 0).unwrap();
        assert_eq!(out["items"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn purge_removes_only_old_rows() {
        let conn = mem();
        let old_id = add(&conn, "ישן מאוד");
        add(&conn, "טרי לגמרי");
        // Backdate one row by 10 days.
        conn.execute(
            "UPDATE transcripts SET created_at = created_at - 864000000 WHERE id = ?1",
            params![old_id],
        )
        .unwrap();
        let removed = purge_older_than(&conn, 7).unwrap();
        assert_eq!(removed, 1);
        let out = search(&conn, None, None, 0).unwrap();
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0]["text"].as_str().unwrap().contains("טרי"));
    }

    #[test]
    fn fts_expr_escapes_quotes() {
        assert_eq!(
            fts_expr("he\"llo world").unwrap(),
            "\"he\"\"llo\"* \"world\"*"
        );
        assert!(fts_expr("   ").is_none());
    }
}
