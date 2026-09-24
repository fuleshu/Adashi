//! Checkout-only runtime state, including request retry results. None of it is
//! project history; the shared journal publishes local and project files atomically.
use super::*;
use rusqlite::{params, Connection};

pub(super) const PATH: &str = "$local/state.json";
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct LocalState {
    computers: Vec<api::ComputerCheckout>,
    versions: Vec<api::coordination::ResourceVersion>,
    intents: Vec<api::coordination::ResourceIntent>,
    receipts: BTreeMap<String, Value>,
    pub owned_runs: BTreeSet<i64>,
}
impl LocalState {
    pub fn retry_results(&self) -> &BTreeMap<String, Value> {
        &self.receipts
    }
    pub fn change_fingerprint(&self) -> StorageResult<Vec<u8>> {
        codec::bytes(&(&self.computers, &self.versions))
    }
    pub fn read(files: &Files) -> StorageResult<Self> {
        files
            .get(PATH)
            .map(|b| codec::parse(b, PATH))
            .transpose()
            .map(|v| v.unwrap_or_default())
    }
    pub fn install(&self, db: &Connection, project: i64) -> StorageResult<()> {
        for c in &self.computers {
            db.execute("INSERT OR REPLACE INTO project_computers(project_id,computer_id,repository_path) VALUES(?1,?2,?3)",params![project,c.computer_id,c.repository_path]).map_err(StorageError::backend)?;
        }
        for v in &self.versions {
            if v.resource_kind != "computer" {
                return Err(invalid(PATH, "only computer versions are local"));
            }
            db.execute("INSERT OR REPLACE INTO resource_versions(project_id,resource_kind,resource_id,version) VALUES(?1,?2,?3,?4)",params![project,v.resource_kind,v.resource_id,v.version]).map_err(StorageError::backend)?;
        }
        for (id, result) in &self.receipts {
            db.execute("INSERT OR REPLACE INTO mutation_operations(project_id,operation_id,result_json) VALUES(?1,?2,?3)",params![project,id,serde_json::to_string(result).map_err(StorageError::backend)?]).map_err(StorageError::backend)?;
        }
        for intent in &self.intents {
            db.execute("INSERT OR REPLACE INTO resource_intents(project_id,agent_run_id,resource_kind,resource_id,expires_at) VALUES(?1,?2,?3,?4,?5)",params![project,intent.agent_run_id,intent.resource_kind,intent.resource_id,intent.expires_at]).map_err(StorageError::backend)?;
        }
        Ok(())
    }
    pub fn capture(&mut self, db: &Connection, project: i64) -> StorageResult<()> {
        let mut stmt=db.prepare("SELECT computer_id,repository_path FROM project_computers WHERE project_id=?1 ORDER BY computer_id").map_err(StorageError::backend)?;
        self.computers = stmt
            .query_map([project], |r| {
                Ok(api::ComputerCheckout {
                    computer_id: r.get(0)?,
                    repository_path: r.get(1)?,
                })
            })
            .map_err(StorageError::backend)?
            .collect::<rusqlite::Result<_>>()
            .map_err(StorageError::backend)?;
        self.versions = sqlite::snapshot::all_versions(db, project)?
            .into_iter()
            .filter(|v| v.resource_kind == "computer")
            .collect();
        self.intents =
            sqlite::concurrency::load_live_intents(db, project).map_err(StorageError::backend)?;
        let mut stmt = db
            .prepare("SELECT operation_id,result_json FROM mutation_operations WHERE project_id=?1")
            .map_err(StorageError::backend)?;
        self.receipts.clear();
        for item in stmt
            .query_map([project], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(StorageError::backend)?
        {
            let (id, text) = item.map_err(StorageError::backend)?;
            let value: Value = serde_json::from_str(&text).map_err(StorageError::backend)?;
            self.receipts.insert(id, value);
        }
        Ok(())
    }
    pub fn add_to(&self, files: &mut Files) -> StorageResult<()> {
        files.insert(PATH.into(), codec::bytes(self)?);
        Ok(())
    }
    pub fn guard_rows(&self, rows: &engine::Rows, project: i64) -> engine::Rows {
        let mut rows = rows.clone();
        for c in &self.computers {
            rows.insert(
                ("project_computers".into(), c.computer_id.clone()),
                BTreeMap::from([
                    ("project_id".into(), project.into()),
                    ("computer_id".into(), c.computer_id.clone().into()),
                    ("repository_path".into(), c.repository_path.clone().into()),
                ]),
            );
        }
        for v in &self.versions {
            rows.insert(
                (
                    "resource_versions".into(),
                    format!("computer:{}", v.resource_id),
                ),
                BTreeMap::from([
                    ("resource_kind".into(), v.resource_kind.clone().into()),
                    ("resource_id".into(), v.resource_id.clone().into()),
                    ("version".into(), v.version.into()),
                ]),
            );
        }
        rows
    }
    pub fn require_owned_claims(
        &self,
        db: &Connection,
        mutation: &api::Mutation,
    ) -> StorageResult<()> {
        for change in &mutation.changes {
            if let api::Change::Qa(
                api::QaWrite::ClaimJob { job_run_id, .. }
                | api::QaWrite::CompleteJob { job_run_id, .. },
            ) = change
            {
                let run: i64 = db
                    .query_row(
                        "SELECT qa_run_id FROM qa_job_runs WHERE id=?1",
                        [job_run_id],
                        |r| r.get(0),
                    )
                    .map_err(StorageError::backend)?;
                if !self.owned_runs.contains(&run) {
                    return Err(invalid("QA reservation","this checkout does not own execution of the copied reservation; create a new run"));
                }
            }
        }
        Ok(())
    }
}
