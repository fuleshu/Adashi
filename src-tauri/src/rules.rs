use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::settings::RuleTemplate;

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

pub fn load_rule(db: &Connection, rule_id: i64) -> Result<Rule, String> {
    db.query_row(
        "SELECT id, name, enabled, intend, hook, prompt
         FROM rules
         WHERE id = ?1",
        params![rule_id],
        |row| {
            Ok(Rule {
                id: row.get(0)?,
                name: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                intend: row.get(3)?,
                hook: row.get(4)?,
                prompt: row.get(5)?,
            })
        },
    )
    .map_err(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => format!("Unknown rule id: {rule_id}"),
        _ => err.to_string(),
    })
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

pub fn create_rule_from_template(
    db: &Connection,
    project_id: i64,
    template: &RuleTemplate,
) -> Result<i64, String> {
    create_rule(
        db,
        project_id,
        NewRule {
            name: template.name.clone(),
            enabled: template.enabled,
            intend: template.intend.clone(),
            hook: template.hook.clone(),
            prompt: template.prompt.clone(),
        },
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_rule_from_template_inserts_normal_project_rule() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
                intend TEXT NOT NULL CHECK(intend IN ('general', 'design', 'implementation')),
                hook TEXT NOT NULL CHECK(hook IN ('run.start', 'task.start', 'task.end', 'run.end')),
                prompt TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .unwrap();
        let template = RuleTemplate {
            id: "template-1".to_string(),
            name: "Implementation Hook".to_string(),
            enabled: false,
            intend: "implementation".to_string(),
            hook: "run.start".to_string(),
            prompt: "Follow the design.".to_string(),
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
        };

        let rule_id = create_rule_from_template(&db, 7, &template).unwrap();
        let rule = load_rule(&db, rule_id).unwrap();

        assert_eq!(rule.name, template.name);
        assert_eq!(rule.enabled, template.enabled);
        assert_eq!(rule.intend, template.intend);
        assert_eq!(rule.hook, template.hook);
        assert_eq!(rule.prompt, template.prompt);
    }
}
