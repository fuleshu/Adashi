//! Canonical records, UUID references and tombstone conversion.
use super::engine::{Data, Rows, Table, COLLECTIONS};
use super::*;

pub(super) fn parse_records(files: &Files) -> StorageResult<BTreeMap<String, codec::Record>> {
    let format: codec::Format = codec::parse(
        files
            .get("format.json")
            .ok_or_else(|| invalid("format.json", "text project is not initialized"))?,
        "format.json",
    )?;
    if ![1, 2].contains(&format.schema_version)
        || ![14, sqlite::schema::SCHEMA_VERSION].contains(&format.relational_schema)
    {
        return Err(invalid(
            "format.json",
            "unsupported text or relational schema version",
        ));
    }
    let mut records = BTreeMap::new();
    for (path, bytes) in files
        .iter()
        .filter(|(p, _)| p.as_str() != "format.json" && !p.starts_with("$local/"))
    {
        let r: codec::Record = codec::parse(bytes, path)?;
        if format.relational_schema < 15 && r.collection.starts_with("markdown_design_") {
            return Err(invalid(path, "Markdown records require relational schema 15"));
        }
        if format.schema_version == 2 && r.collection == "mutation_operations" {
            return Err(invalid(
                path,
                "request history belongs in ignored local state, not text project records",
            ));
        }
        if r.schema_version != 1
            || !codec::valid_identity(&r.identity)
            || !COLLECTIONS.contains(&r.collection.as_str())
            || *path != format!("records/{}/{}.json", r.collection, r.identity)
        {
            return Err(invalid(
                path,
                "record version, identity or collection does not match its path",
            ));
        }
        if records.insert(r.identity.clone(), r).is_some() {
            return Err(invalid(path, "duplicate record identity"));
        }
    }
    Ok(records)
}

/// Resolve UUID references recursively, including composite primary keys.
fn field(
    records: &BTreeMap<String, codec::Record>,
    tables: &[Table],
    record: &codec::Record,
    column: &str,
    trail: &mut BTreeSet<(String, String)>,
) -> StorageResult<Value> {
    let path = format!("records/{}/{}.json", record.collection, record.identity);
    if !trail.insert((record.identity.clone(), column.into())) {
        return Err(invalid(&path, "cyclic foreign-key value"));
    }
    let value = record
        .data
        .get(column)
        .ok_or_else(|| invalid(&path, &format!("missing field {column}")))?;
    let table = tables.iter().find(|t| t.name == record.collection).unwrap();
    let reference = table
        .foreign
        .iter()
        .find_map(|f| f.from.iter().position(|c| c == column).map(|i| (f, i)));
    let result = if let Some((f, index)) = reference {
        if value.is_null() {
            Value::Null
        } else {
            let object = value
                .as_object()
                .ok_or_else(|| invalid(&path, &format!("{column} requires a UUID reference")))?;
            let id = object
                .get("ref")
                .and_then(Value::as_str)
                .filter(|_| object.len() == 1)
                .ok_or_else(|| invalid(&path, "invalid reference object"))?;
            let target = records
                .get(id)
                .filter(|r| !r.deleted && r.collection == f.table)
                .ok_or_else(|| {
                    invalid(
                        &path,
                        &format!(
                            "{column} references missing/deleted {} record {id}",
                            f.table
                        ),
                    )
                })?;
            field(records, tables, target, &f.to[index], trail)?
        }
    } else {
        codec::decode(value, &path)?
    };
    trail.remove(&(record.identity.clone(), column.into()));
    Ok(result)
}

pub(super) fn rows_from_records(
    records: &BTreeMap<String, codec::Record>,
    tables: &[Table],
) -> StorageResult<Rows> {
    let mut rows = Rows::new();
    let mut aliases = BTreeSet::new();
    for record in records.values() {
        let table = tables.iter().find(|t| t.name == record.collection).unwrap();
        let mut key_data = Data::new();
        for column in table.primary() {
            let value = if record.deleted {
                record
                    .data
                    .get(&column.name)
                    .cloned()
                    .ok_or_else(|| invalid(&record.identity, "missing tombstone primary key"))?
            } else {
                field(records, tables, record, &column.name, &mut BTreeSet::new())?
            };
            let valid = match column.kind.as_str() {
                "INTEGER" => value.as_i64().is_some(),
                "TEXT" => value.is_string(),
                _ => false,
            };
            if !valid {
                return Err(invalid(&record.identity, "invalid primary key"));
            }
            if column.name == "id" && !value.as_i64().is_some_and(|v| v > 0 && v <= MAX_SAFE) {
                return Err(invalid(
                    &record.identity,
                    "API alias must be a positive JavaScript-safe integer",
                ));
            }
            key_data.insert(column.name.clone(), value);
        }
        if record.deleted && record.data.len() != table.primary().len() {
            return Err(invalid(
                &record.identity,
                "tombstone may retain only its primary key",
            ));
        }
        if !aliases.insert((record.collection.clone(), table.key(&key_data)?)) {
            return Err(invalid(
                &record.identity,
                "duplicate live/deleted primary key or API alias",
            ));
        }
    }
    for record in records.values().filter(|r| !r.deleted) {
        let table = tables.iter().find(|t| t.name == record.collection).unwrap();
        let path = format!("records/{}/{}.json", record.collection, record.identity);
        if record.data.len() != table.columns.len() {
            return Err(invalid(&path, "missing or unknown record fields"));
        }
        let mut data = Data::new();
        for column in &table.columns {
            let v = field(records, tables, record, &column.name, &mut BTreeSet::new())?;
            let valid = if v.is_null() {
                column.nullable && column.pk == 0
            } else {
                match column.kind.as_str() {
                    "INTEGER" => v.as_i64().is_some(),
                    "TEXT" => v.is_string(),
                    "REAL" => v.is_number(),
                    _ => false,
                }
            };
            if !valid {
                return Err(invalid(
                    &path,
                    &format!(
                        "{} must be {}{}",
                        column.name,
                        column.kind,
                        if column.nullable { " or null" } else { "" }
                    ),
                ));
            }
            if let Some(text) = v.as_str() {
                if text.lines().any(|l| {
                    l.starts_with("<<<<<<< ") || l.starts_with(">>>>>>> ") || l == "======="
                }) {
                    return Err(invalid(&path, "unresolved conflict markers in content"));
                }
            }
            data.insert(column.name.clone(), v);
        }
        let key = table.key(&data)?;
        if rows.insert((table.name.clone(), key), data).is_some() {
            return Err(invalid(
                &path,
                "duplicate primary key/API alias after merge",
            ));
        }
    }
    Ok(rows)
}

pub(super) fn export(
    rows: &Rows,
    tables: &[Table],
    old: &BTreeMap<String, codec::Record>,
    old_rows: &Rows,
    initial: bool,
) -> StorageResult<Files> {
    let project = rows
        .iter()
        .find(|((t, _), _)| t == "projects")
        .map(|(_, d)| d["slug"].as_str().unwrap_or(""))
        .ok_or_else(|| invalid("projects", "missing project"))?;
    let mut identities = BTreeMap::<(String, String), String>::new();
    // Live old records have already been fully validated. Tombstone primary keys
    // are stored as decoded scalars, so deleted aliases remain reserved.
    for r in old.values() {
        let table = tables.iter().find(|t| t.name == r.collection).unwrap();
        let key = if r.deleted {
            table.key(&r.data)?
        } else {
            let mut d = Data::new();
            for c in table.primary() {
                d.insert(
                    c.name.clone(),
                    field(old, tables, r, &c.name, &mut BTreeSet::new())?,
                );
            }
            table.key(&d)?
        };
        if identities
            .insert((r.collection.clone(), key), r.identity.clone())
            .is_some()
        {
            return Err(invalid(&r.collection, "live/deleted alias collision"));
        }
    }
    for key in rows
        .keys()
        .filter(|(table, _)| table != "mutation_operations")
    {
        if let Some(id) = identities.get(key) {
            let table = tables.iter().find(|t| t.name == key.0).unwrap();
            let primary = table.primary();
            if old.get(id).is_some_and(|r| r.deleted)
                && primary.len() == 1
                && primary[0].name == "id"
            {
                return Err(invalid(
                    &key.0,
                    "a deleted primary-key alias cannot be reused",
                ));
            }
        }
        identities.entry(key.clone()).or_insert_with(|| {
            if initial {
                codec::deterministic_identity(&format!("{project}:{}:{}", key.0, key.1))
            } else {
                uuid::Uuid::new_v4().to_string()
            }
        });
    }
    let mut references = BTreeMap::new();
    for table in tables {
        for f in &table.foreign {
            let index_key = (f.table.clone(), f.to.clone());
            if references.contains_key(&index_key) {
                continue;
            }
            let mut index = BTreeMap::new();
            for (key, data) in rows.iter().filter(|((t, _), _)| t == &f.table) {
                let v = serde_json::to_string(&f.to.iter().map(|c| &data[c]).collect::<Vec<_>>())
                    .map_err(StorageError::backend)?;
                if index.insert(v, identities[key].clone()).is_some() {
                    return Err(invalid(&f.table, "ambiguous foreign-key target"));
                }
            }
            references.insert(index_key, index);
        }
    }
    let mut files = Files::new();
    files.insert(
        "format.json".into(),
        codec::bytes(&codec::Format {
            schema_version: 2,
            relational_schema: sqlite::schema::SCHEMA_VERSION,
        })?,
    );
    for (key, row) in rows
        .iter()
        .filter(|((table, _), _)| table != "mutation_operations")
    {
        let table = tables.iter().find(|t| t.name == key.0).unwrap();
        let mut data = row
            .iter()
            .map(|(k, v)| (k.clone(), codec::encode(v)))
            .collect::<Data>();
        for f in &table.foreign {
            if f.from.iter().any(|c| row[c].is_null()) {
                continue;
            }
            let lookup = serde_json::to_string(&f.from.iter().map(|c| &row[c]).collect::<Vec<_>>())
                .map_err(StorageError::backend)?;
            let id = references[&(f.table.clone(), f.to.clone())]
                .get(&lookup)
                .ok_or_else(|| invalid(&key.0, "missing referenced record"))?;
            for column in &f.from {
                data.insert(column.clone(), serde_json::json!({"ref":id}));
            }
        }
        let record = codec::Record {
            schema_version: 1,
            identity: identities[key].clone(),
            collection: key.0.clone(),
            deleted: false,
            data,
        };
        files.insert(
            format!("records/{}/{}.json", record.collection, record.identity),
            codec::bytes(&record)?,
        );
    }
    for r in old
        .values()
        .filter(|r| r.collection != "mutation_operations")
    {
        let path = format!("records/{}/{}.json", r.collection, r.identity);
        if files.contains_key(&path) {
            continue;
        }
        let mut tombstone = r.clone();
        tombstone.deleted = true;
        if !r.deleted {
            let table = tables.iter().find(|t| t.name == r.collection).unwrap();
            let original = old_rows
                .iter()
                .find(|(key, _)| identities.get(*key) == Some(&r.identity))
                .map(|(_, data)| data)
                .ok_or_else(|| invalid(&path, "missing deletion preimage"))?;
            tombstone.data = table
                .primary()
                .iter()
                .map(|c| (c.name.clone(), original[&c.name].clone()))
                .collect();
        }
        files.insert(path, codec::bytes(&tombstone)?);
    }
    Ok(files)
}
