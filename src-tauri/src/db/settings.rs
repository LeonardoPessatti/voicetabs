use serde::{de::DeserializeOwned, Serialize};

use super::Db;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn get_raw(db: &Db, key: &str) -> Result<Option<String>, SettingsError> {
    let value: Option<String> = db.with(|c| {
        c.query_row(
            "SELECT value FROM settings WHERE key = ?",
            [key],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })?;
    Ok(value)
}

pub fn set_raw(db: &Db, key: &str, value: &str) -> Result<(), SettingsError> {
    db.with(|c| {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )
    })?;
    Ok(())
}

pub fn get<T: DeserializeOwned>(db: &Db, key: &str) -> Result<Option<T>, SettingsError> {
    match get_raw(db, key)? {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

pub fn set<T: Serialize>(db: &Db, key: &str, value: &T) -> Result<(), SettingsError> {
    let encoded = serde_json::to_string(value)?;
    set_raw(db, key, &encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial_schema.sql"))
            .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap();
        Db::from_connection(conn)
    }

    #[test]
    fn get_missing_returns_none() {
        let db = mem_db();
        assert!(get::<String>(&db, "missing").unwrap().is_none());
    }

    #[test]
    fn set_then_get_roundtrips_a_string() {
        let db = mem_db();
        set(&db, "ui_locale", &"pt-BR".to_string()).unwrap();
        let got: Option<String> = get(&db, "ui_locale").unwrap();
        assert_eq!(got.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn set_overwrites_existing_value() {
        let db = mem_db();
        set(&db, "ui_locale", &"pt-BR".to_string()).unwrap();
        set(&db, "ui_locale", &"en".to_string()).unwrap();
        let got: Option<String> = get(&db, "ui_locale").unwrap();
        assert_eq!(got.as_deref(), Some("en"));
    }

    #[test]
    fn set_then_get_roundtrips_structured_value() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Active { tab_id: i64 }
        let db = mem_db();
        set(&db, "active_tab", &Active { tab_id: 42 }).unwrap();
        let got: Active = get(&db, "active_tab").unwrap().unwrap();
        assert_eq!(got, Active { tab_id: 42 });
    }
}
