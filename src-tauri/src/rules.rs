use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const INTENDS: &[&str] = &["general", "design", "implementation"];
pub const HOOKS: &[&str] = &["run.start", "task.start", "task.end", "run.end"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct Rule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct InjectionRule {
    pub id: i64,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct NewRule {
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub intend: String,
    pub hook: String,
    pub prompt: String,
}

pub fn load_rules(db: &Connection) -> Result<Vec<Rule>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, name, enabled, intend, hook, prompt
             FROM rules
             ORDER BY
                CASE hook
                    WHEN 'run.start' THEN 1
                    WHEN 'task.start' THEN 2
                    WHEN 'task.end' THEN 3
                    WHEN 'run.end' THEN 4
                    ELSE 5
                END,
                intend,
                name,
                id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Rule {
                id: row.get(0)?,
                name: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                intend: row.get(3)?,
                hook: row.get(4)?,
                prompt: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

pub fn load_rule_injections(
    db: &Connection,
    intend: &str,
    hook: &str,
) -> Result<Vec<InjectionRule>, String> {
    validate_intend(intend)?;
    validate_hook(hook)?;

    let mut statement = db
        .prepare(
            "SELECT id, enabled, intend, hook, prompt
             FROM rules
             WHERE enabled = 1 AND intend = ?1 AND hook = ?2
             ORDER BY id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![intend, hook], |row| {
            Ok(InjectionRule {
                id: row.get(0)?,
                enabled: row.get::<_, i64>(1)? != 0,
                intend: row.get(2)?,
                hook: row.get(3)?,
                prompt: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

pub fn create_rule(db: &Connection, project_id: i64, input: NewRule) -> Result<i64, String> {
    validate_intend(&input.intend)?;
    validate_hook(&input.hook)?;

    let name = input.name.trim();
    if name.is_empty() {
        return Err("Rule name is required".to_string());
    }

    db.execute(
        "INSERT INTO rules(project_id, name, enabled, intend, hook, prompt)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project_id,
            name,
            if input.enabled { 1 } else { 0 },
            input.intend,
            input.hook,
            input.prompt
        ],
    )
    .map_err(|err| err.to_string())?;

    Ok(db.last_insert_rowid())
}

pub fn update_rule(db: &Connection, input: UpdateRule) -> Result<(), String> {
    validate_intend(&input.intend)?;
    validate_hook(&input.hook)?;

    let name = input.name.trim();
    if name.is_empty() {
        return Err("Rule name is required".to_string());
    }

    let affected = db
        .execute(
            "UPDATE rules
             SET name = ?1,
                 enabled = ?2,
                 intend = ?3,
                 hook = ?4,
                 prompt = ?5,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?6",
            params![
                name,
                if input.enabled { 1 } else { 0 },
                input.intend,
                input.hook,
                input.prompt,
                input.id
            ],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        Err(format!("Unknown rule id: {}", input.id))
    } else {
        Ok(())
    }
}

pub fn delete_rule(db: &Connection, rule_id: i64) -> Result<(), String> {
    let affected = db
        .execute("DELETE FROM rules WHERE id = ?1", params![rule_id])
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        Err(format!("Unknown rule id: {rule_id}"))
    } else {
        Ok(())
    }
}

fn validate_intend(intend: &str) -> Result<(), String> {
    if INTENDS.contains(&intend) {
        Ok(())
    } else {
        Err(format!(
            "Invalid intend '{intend}'. Expected one of: {}",
            INTENDS.join(", ")
        ))
    }
}

fn validate_hook(hook: &str) -> Result<(), String> {
    if HOOKS.contains(&hook) {
        Ok(())
    } else {
        Err(format!(
            "Invalid hook '{hook}'. Expected one of: {}",
            HOOKS.join(", ")
        ))
    }
}
