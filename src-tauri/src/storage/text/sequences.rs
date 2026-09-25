//! Preserve SQL AUTOINCREMENT behavior across projections and backend conversion.
use super::*;
use rusqlite::{params, Connection};

fn numeric(table: &engine::Table) -> bool {
    let primary = table.primary();
    primary.len() == 1 && primary[0].name == "id"
}

fn live_max(db: &Connection, table: &engine::Table) -> StorageResult<i64> {
    db.query_row(
        &format!("SELECT COALESCE(MAX(id),0) FROM {}", table.name),
        [],
        |row| row.get(0),
    )
    .map_err(StorageError::backend)
}

fn deleted_max(records: &BTreeMap<String, codec::Record>, name: &str) -> i64 {
    records
        .values()
        .filter(|r| r.deleted && r.collection == name)
        .filter_map(|r| r.data.get("id").and_then(Value::as_i64))
        .max()
        .unwrap_or(0)
}

/// Restore each numeric collection without random gaps or reusing deleted IDs.
pub(super) fn restore(
    db: &Connection,
    tables: &[engine::Table],
    records: &BTreeMap<String, codec::Record>,
) -> StorageResult<()> {
    for table in tables.iter().filter(|t| numeric(t)) {
        let high = live_max(db, table)?.max(deleted_max(records, &table.name));
        db.execute("DELETE FROM sqlite_sequence WHERE name=?1", [&table.name])
            .map_err(StorageError::backend)?;
        db.execute(
            "INSERT INTO sqlite_sequence(name,seq) VALUES(?1,?2)",
            params![table.name, high],
        )
        .map_err(StorageError::backend)?;
    }
    Ok(())
}

/// An upsert can consume an ID without adding a row. Persist that high-water mark
/// as an existing-format tombstone, just as conversions preserve deleted SQL IDs.
pub(super) fn retain(
    db: &Connection,
    tables: &[engine::Table],
    records: &mut BTreeMap<String, codec::Record>,
) -> StorageResult<()> {
    let slug: String = db
        .query_row("SELECT slug FROM projects", [], |r| r.get(0))
        .map_err(StorageError::backend)?;
    for table in tables.iter().filter(|t| numeric(t)) {
        let high: i64 = db
            .query_row(
                "SELECT COALESCE(MAX(seq),0) FROM sqlite_sequence WHERE name=?1",
                [&table.name],
                |r| r.get(0),
            )
            .map_err(StorageError::backend)?;
        if high > MAX_SAFE {
            return Err(invalid(&table.name, "integer alias space exhausted"));
        }
        if high <= live_max(db, table)?.max(deleted_max(records, &table.name)) {
            continue;
        }
        // A row deleted by this transaction already becomes a tombstone on export.
        if records.values().any(|r| {
            r.collection == table.name && r.data.get("id").and_then(Value::as_i64) == Some(high)
        }) {
            continue;
        }
        let identity = codec::deterministic_identity(&format!("{slug}:{}:[{high}]", table.name));
        records.entry(identity.clone()).or_insert(codec::Record {
            schema_version: 1,
            identity,
            collection: table.name.clone(),
            deleted: true,
            data: BTreeMap::from([("id".into(), high.into())]),
        });
    }
    Ok(())
}
