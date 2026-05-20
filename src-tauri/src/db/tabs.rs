use serde::{Deserialize, Serialize};

use super::Db;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tab {
    pub id: i64,
    pub title: String,
    pub order_idx: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum TabsError {
    #[error("tab not found: {0}")]
    NotFound(i64),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

pub fn list(db: &Db) -> Result<Vec<Tab>, TabsError> {
    db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, title, order_idx, created_at, updated_at
             FROM tabs ORDER BY order_idx ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Tab {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    order_idx: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn create(db: &Db, title: &str, now_ms: i64) -> Result<Tab, TabsError> {
    db.with(|c| {
        let tx = c.transaction()?;
        let next_order: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(order_idx), -1) + 1 FROM tabs",
                [],
                |r| r.get(0),
            )?;
        tx.execute(
            "INSERT INTO tabs (title, order_idx, created_at, updated_at)
             VALUES (?, ?, ?, ?)",
            rusqlite::params![title, next_order, now_ms, now_ms],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Tab {
            id,
            title: title.to_string(),
            order_idx: next_order,
            created_at: now_ms,
            updated_at: now_ms,
        })
    })
}

pub fn rename(db: &Db, id: i64, new_title: &str, now_ms: i64) -> Result<(), TabsError> {
    let changed = db.with(|c| {
        c.execute(
            "UPDATE tabs SET title = ?, updated_at = ? WHERE id = ?",
            rusqlite::params![new_title, now_ms, id],
        )
    })?;
    if changed == 0 {
        return Err(TabsError::NotFound(id));
    }
    Ok(())
}

pub fn delete(db: &Db, id: i64) -> Result<(), TabsError> {
    let changed = db.with(|c| c.execute("DELETE FROM tabs WHERE id = ?", [id]))?;
    if changed == 0 {
        return Err(TabsError::NotFound(id));
    }
    Ok(())
}

pub fn reorder(db: &Db, ordered_ids: &[i64], now_ms: i64) -> Result<(), TabsError> {
    db.with(|c| {
        let tx = c.transaction()?;
        for (idx, id) in ordered_ids.iter().enumerate() {
            let changed = tx.execute(
                "UPDATE tabs SET order_idx = ?, updated_at = ? WHERE id = ?",
                rusqlite::params![idx as i64, now_ms, id],
            )?;
            if changed == 0 {
                return Err(TabsError::NotFound(*id));
            }
        }
        tx.commit()?;
        Ok(())
    })
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
    fn create_then_list_returns_in_order() {
        let db = mem_db();
        let t1 = create(&db, "A", 100).unwrap();
        let t2 = create(&db, "B", 101).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, t1.id);
        assert_eq!(listed[0].order_idx, 0);
        assert_eq!(listed[1].id, t2.id);
        assert_eq!(listed[1].order_idx, 1);
    }

    #[test]
    fn rename_updates_title_and_updated_at() {
        let db = mem_db();
        let t = create(&db, "old", 100).unwrap();
        rename(&db, t.id, "new", 200).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed[0].title, "new");
        assert_eq!(listed[0].updated_at, 200);
    }

    #[test]
    fn rename_unknown_returns_not_found() {
        let db = mem_db();
        let err = rename(&db, 999, "x", 100).unwrap_err();
        assert!(matches!(err, TabsError::NotFound(999)));
    }

    #[test]
    fn delete_removes_the_tab() {
        let db = mem_db();
        let t = create(&db, "x", 1).unwrap();
        delete(&db, t.id).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn reorder_assigns_order_idx_by_position_in_array() {
        let db = mem_db();
        let a = create(&db, "A", 1).unwrap();
        let b = create(&db, "B", 2).unwrap();
        let c = create(&db, "C", 3).unwrap();
        reorder(&db, &[c.id, a.id, b.id], 10).unwrap();
        let listed = list(&db).unwrap();
        assert_eq!(listed[0].id, c.id);
        assert_eq!(listed[1].id, a.id);
        assert_eq!(listed[2].id, b.id);
        for (i, t) in listed.iter().enumerate() {
            assert_eq!(t.order_idx, i as i64);
        }
    }

    #[test]
    fn reorder_unknown_id_returns_not_found() {
        let db = mem_db();
        let a = create(&db, "A", 1).unwrap();
        let err = reorder(&db, &[a.id, 999], 10).unwrap_err();
        assert!(matches!(err, TabsError::NotFound(999)));
    }
}
