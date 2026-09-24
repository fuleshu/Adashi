//! Schema-14 record vocabulary and disposable relational projection.
use super::*;
use rusqlite::{
    types::{Value as SqlValue, ValueRef},
    Connection,
};

pub(super) const COLLECTIONS: &[&str] = &[
    "projects",
    "design_workspaces",
    "diagrams",
    "c4_elements",
    "c4_relationships",
    "design_bindings",
    "agent_tasks",
    "task_design_specification_links",
    "task_qa_entries",
    "coding_guidelines",
    "post_task_commands",
    "qa_jobs",
    "qa_job_design_links",
    "qa_job_task_links",
    "qa_job_tags",
    "qa_runs",
    "qa_job_runs",
    "qa_checks",
    "ui_mockups",
    "ui_mockup_edit_operations",
    "ui_mockup_annotations",
    "ui_mockup_proposals",
    "rules",
    "fixed_hook_prompts",
    "project_memory",
    "resource_versions",
    "mutation_operations", // SQL projection and version-1 import only; never exported.
    "project_memory_notes",
    "project_memory_note_resolutions",
    "design_health_waivers",
];
pub(super) const NON_CANONICAL: &[&str] = &[
    "project_state",
    "project_computers",
    "resource_intents",
    "schema_migrations",
    "sqlite_sequence",
    "ui_mockup_preview_cache",
    "design_binding_checks",
    "design_element_checks",
];
pub(super) type Data = BTreeMap<String, Value>;
pub(super) type Rows = BTreeMap<(String, String), Data>;

#[derive(Clone, Debug)]
pub(super) struct Column {
    pub name: String,
    pub kind: String,
    pub pk: i64,
    pub nullable: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Foreign {
    pub table: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
}
#[derive(Clone, Debug)]
pub(super) struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub foreign: Vec<Foreign>,
}
impl Table {
    pub fn primary(&self) -> Vec<&Column> {
        let mut c = self.columns.iter().filter(|c| c.pk > 0).collect::<Vec<_>>();
        c.sort_by_key(|c| c.pk);
        c
    }
    pub fn key(&self, data: &Data) -> StorageResult<String> {
        serde_json::to_string(
            &self
                .primary()
                .iter()
                .map(|c| data.get(&c.name).cloned().unwrap_or(Value::Null))
                .collect::<Vec<_>>(),
        )
        .map_err(StorageError::backend)
    }
}

pub(super) fn empty() -> StorageResult<Connection> {
    let mut db = Connection::open_in_memory().map_err(StorageError::backend)?;
    sqlite::schema::migrate(&mut db).map_err(StorageError::backend)?;
    Ok(db)
}

pub(super) fn tables(db: &Connection) -> StorageResult<Vec<Table>> {
    let mut stmt = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(StorageError::backend)?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(StorageError::backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::backend)?;
    for name in &names {
        if !COLLECTIONS.contains(&name.as_str()) && !NON_CANONICAL.contains(&name.as_str()) {
            return Err(invalid(
                name,
                "unclassified database table; conversion would be lossy",
            ));
        }
    }
    COLLECTIONS
        .iter()
        .map(|name| {
            if !names.iter().any(|n| n == name) {
                return Err(invalid(name, "missing required schema-14 table"));
            }
            let columns = db
                .prepare(&format!("PRAGMA table_info({name})"))
                .map_err(StorageError::backend)?
                .query_map([], |r| {
                    Ok(Column {
                        name: r.get(1)?,
                        kind: r.get(2)?,
                        nullable: r.get::<_, i64>(3)? == 0,
                        pk: r.get(5)?,
                    })
                })
                .map_err(StorageError::backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StorageError::backend)?;
            let mut foreign = BTreeMap::<i64, Foreign>::new();
            let mut s = db
                .prepare(&format!("PRAGMA foreign_key_list({name})"))
                .map_err(StorageError::backend)?;
            let entries = s
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                })
                .map_err(StorageError::backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StorageError::backend)?;
            let mut entries = entries;
            entries.sort_by_key(|e| (e.0, e.1));
            for (id, _, target, from, to) in entries {
                let f = foreign.entry(id).or_insert(Foreign {
                    table: target,
                    from: vec![],
                    to: vec![],
                });
                f.from.push(from);
                f.to.push(to);
            }
            Ok(Table {
                name: (*name).into(),
                columns,
                foreign: foreign.into_values().collect(),
            })
        })
        .collect()
}

/// All SQL identifiers originate in the closed schema, never in a record.
pub(super) fn dump(db: &Connection, tables: &[Table]) -> StorageResult<Rows> {
    let mut rows = Rows::new();
    for table in tables {
        let mut stmt = db
            .prepare(&format!("SELECT * FROM {}", table.name))
            .map_err(StorageError::backend)?;
        let mut query = stmt.query([]).map_err(StorageError::backend)?;
        while let Some(row) = query.next().map_err(StorageError::backend)? {
            let mut data = Data::new();
            for (i, c) in table.columns.iter().enumerate() {
                let value = match row.get_ref(i).map_err(StorageError::backend)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => n.into(),
                    ValueRef::Real(n) => n.into(),
                    ValueRef::Text(s) => String::from_utf8(s.to_vec())
                        .map_err(StorageError::backend)?
                        .into(),
                    ValueRef::Blob(_) => {
                        return Err(invalid(&table.name, "unexpected authoritative binary data"))
                    }
                };
                data.insert(c.name.clone(), value);
            }
            // Shared files contain no machine-specific registration or generated renderer output.
            if table.name == "projects" {
                data.insert("repository_path".into(), Value::Null);
            }
            if table.name == "design_workspaces" {
                data.insert("structurizr_dsl".into(), "".into());
                data.insert("structurizr_json".into(), "".into());
                data.insert("updated_at".into(), data["created_at"].clone());
            }
            if table.name == "diagrams" && data["kind"] == "structurizr" {
                data.insert("source".into(), "".into());
                data.insert("updated_at".into(), data["created_at"].clone());
            }
            if table.name == "resource_versions" && data["resource_kind"] == "computer" {
                continue;
            }
            let key = table.key(&data)?;
            rows.insert((table.name.clone(), key), data);
        }
    }
    Ok(rows)
}

pub(super) fn import(db: &Connection, tables: &[Table], rows: &Rows) -> StorageResult<()> {
    db.pragma_update(None, "foreign_keys", false)
        .map_err(StorageError::backend)?;
    for table in tables {
        db.execute(&format!("DELETE FROM {}", table.name), [])
            .map_err(StorageError::backend)?;
    }
    for ((name, _), data) in rows {
        let table = tables
            .iter()
            .find(|t| t.name == *name)
            .ok_or_else(|| invalid(name, "unknown collection"))?;
        let columns = table
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let values = table
            .columns
            .iter()
            .map(|c| to_sql(&data[&c.name]))
            .collect::<StorageResult<Vec<_>>>()?;
        let params = vec!["?"; values.len()].join(",");
        db.execute(
            &format!("INSERT INTO {name}({columns}) VALUES({params})"),
            rusqlite::params_from_iter(values),
        )
        .map_err(|e| invalid(name, &format!("constraint conflict: {e}")))?;
    }
    db.pragma_update(None, "foreign_keys", true)
        .map_err(StorageError::backend)?;
    let mut check = db
        .prepare("PRAGMA foreign_key_check")
        .map_err(StorageError::backend)?;
    if let Some(row) = check
        .query([])
        .map_err(StorageError::backend)?
        .next()
        .map_err(StorageError::backend)?
    {
        let name: String = row.get(0).map_err(StorageError::backend)?;
        return Err(invalid(&name, "invalid foreign-key reference after merge"));
    }
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
        .map_err(StorageError::backend)?;
    let workspaces: i64 = db
        .query_row("SELECT COUNT(*) FROM design_workspaces", [], |r| r.get(0))
        .map_err(StorageError::backend)?;
    if count != 1 || workspaces != 1 {
        return Err(invalid(
            "text",
            "exactly one project and workspace are required",
        ));
    }
    sqlite::state::ensure_project_state(db).map_err(StorageError::backend)?;
    Ok(())
}

fn to_sql(v: &Value) -> StorageResult<SqlValue> {
    Ok(match v {
        Value::Null => SqlValue::Null,
        Value::String(s) => SqlValue::Text(s.clone()),
        Value::Number(n) if n.is_i64() => SqlValue::Integer(n.as_i64().unwrap()),
        Value::Number(n) => SqlValue::Real(
            n.as_f64()
                .ok_or_else(|| invalid("record", "invalid number"))?,
        ),
        _ => return Err(invalid("record", "invalid scalar")),
    })
}


pub(super) use super::records::{parse_records, rows_from_records, export};
