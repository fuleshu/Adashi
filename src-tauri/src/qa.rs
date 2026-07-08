use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_TIMEOUT_SECONDS: i64 = 120;
const DEFAULT_SHELL: &str = "powershell";
const OUTPUT_LIMIT: usize = 200_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaJob {
    pub id: i64,
    pub number: i64,
    pub name: String,
    pub description: String,
    pub command: String,
    pub working_directory: String,
    pub shell: String,
    pub timeout_seconds: i64,
    pub enabled: bool,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub derived_state: String,
    pub design_specification_links: Vec<QaJobDesignLink>,
    pub task_links: Vec<QaJobTaskLink>,
    pub tags: Vec<String>,
    pub latest_run: Option<QaJobRun>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaJobDesignLink {
    pub id: i64,
    pub qa_job_id: i64,
    pub sort_order: i64,
    pub target_type: String,
    pub design_external_id: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaJobTaskLink {
    pub id: i64,
    pub qa_job_id: i64,
    pub task_id: i64,
    pub sort_order: i64,
    pub title: String,
    pub number: i64,
    pub state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaJobRun {
    pub id: i64,
    pub qa_run_id: i64,
    pub qa_job_id: i64,
    pub command_snapshot: String,
    pub status: String,
    pub exit_code: Option<i64>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub output: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaRun {
    pub id: i64,
    pub trigger_source: String,
    pub query_snapshot: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub summary: String,
    pub job_runs: Vec<QaJobRun>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaJobQuery {
    pub job_ids: Option<Vec<i64>>,
    pub states: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub task_ids: Option<Vec<i64>>,
    pub design_external_ids: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct QaDesignLinkInput {
    pub target_type: Option<String>,
    pub design_external_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct NewQaJob {
    pub name: String,
    pub description: Option<String>,
    pub command: String,
    pub working_directory: Option<String>,
    pub shell: Option<String>,
    pub timeout_seconds: Option<i64>,
    pub enabled: Option<bool>,
    pub created_by: Option<String>,
    pub design_specification_links: Option<Vec<QaDesignLinkInput>>,
    pub task_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(rmcp::schemars::JsonSchema)]
pub struct UpdateQaJob {
    pub qa_job_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub command: Option<String>,
    pub working_directory: Option<String>,
    pub shell: Option<String>,
    pub timeout_seconds: Option<i64>,
    pub enabled: Option<bool>,
    pub design_specification_links: Option<Vec<QaDesignLinkInput>>,
    pub task_ids: Option<Vec<i64>>,
    pub tags: Option<Vec<String>>,
}

pub fn load_jobs(
    db: &Connection,
    project_id: i64,
    query: Option<&QaJobQuery>,
) -> Result<Vec<QaJob>, String> {
    let rows = load_job_rows(db, project_id)?;
    let mut jobs = rows
        .into_iter()
        .map(|row| hydrate_job(db, project_id, row))
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(query) = query {
        jobs.retain(|job| matches_query(job, query));
    }

    jobs.sort_by(|left, right| {
        state_rank(&left.derived_state)
            .cmp(&state_rank(&right.derived_state))
            .then_with(|| left.number.cmp(&right.number))
    });
    Ok(jobs)
}

pub fn load_job(db: &Connection, project_id: i64, qa_job_id: i64) -> Result<QaJob, String> {
    hydrate_job(db, project_id, load_job_row(db, project_id, qa_job_id)?)
}

pub fn create_job(db: &Connection, project_id: i64, input: NewQaJob) -> Result<QaJob, String> {
    let name = required_trimmed(&input.name, "QA job name")?;
    let command = required_trimmed(&input.command, "QA command")?;
    let timeout_seconds =
        validate_timeout(input.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS))?;
    let shell = normalize_shell(input.shell.as_deref());
    let number = next_job_number(db, project_id)?;

    db.execute(
        "INSERT INTO qa_jobs(
            project_id, number, name, description, command, working_directory,
            shell, timeout_seconds, enabled, created_by
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            project_id,
            number,
            name,
            input.description.unwrap_or_default().trim(),
            command,
            input.working_directory.unwrap_or_default().trim(),
            shell,
            timeout_seconds,
            input.enabled.unwrap_or(true) as i64,
            input
                .created_by
                .unwrap_or_else(|| "user".to_string())
                .trim()
        ],
    )
    .map_err(|err| err.to_string())?;

    let qa_job_id = db.last_insert_rowid();
    replace_design_links(
        db,
        qa_job_id,
        input.design_specification_links.unwrap_or_default(),
    )?;
    replace_task_links(
        db,
        project_id,
        qa_job_id,
        input.task_ids.unwrap_or_default(),
    )?;
    replace_tags(db, qa_job_id, input.tags.unwrap_or_default())?;
    load_job(db, project_id, qa_job_id)
}

pub fn update_job(db: &Connection, project_id: i64, input: UpdateQaJob) -> Result<QaJob, String> {
    let current = load_job(db, project_id, input.qa_job_id)?;
    let name = input.name.unwrap_or(current.name).trim().to_string();
    if name.is_empty() {
        return Err("QA job name is required".to_string());
    }

    let command = input.command.unwrap_or(current.command).trim().to_string();
    if command.is_empty() {
        return Err("QA command is required".to_string());
    }

    let timeout_seconds =
        validate_timeout(input.timeout_seconds.unwrap_or(current.timeout_seconds))?;
    let shell = normalize_shell(Some(input.shell.as_deref().unwrap_or(&current.shell)));
    let description = input
        .description
        .unwrap_or(current.description)
        .trim()
        .to_string();
    let working_directory = input
        .working_directory
        .unwrap_or(current.working_directory)
        .trim()
        .to_string();

    let affected = db
        .execute(
            "UPDATE qa_jobs
             SET name = ?1,
                 description = ?2,
                 command = ?3,
                 working_directory = ?4,
                 shell = ?5,
                 timeout_seconds = ?6,
                 enabled = ?7,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?8 AND project_id = ?9",
            params![
                name,
                description,
                command,
                working_directory,
                shell,
                timeout_seconds,
                input.enabled.unwrap_or(current.enabled) as i64,
                input.qa_job_id,
                project_id
            ],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown QA job id: {}", input.qa_job_id));
    }

    if let Some(links) = input.design_specification_links {
        replace_design_links(db, input.qa_job_id, links)?;
    }
    if let Some(task_ids) = input.task_ids {
        replace_task_links(db, project_id, input.qa_job_id, task_ids)?;
    }
    if let Some(tags) = input.tags {
        replace_tags(db, input.qa_job_id, tags)?;
    }

    load_job(db, project_id, input.qa_job_id)
}

pub fn delete_job(db: &Connection, project_id: i64, qa_job_id: i64) -> Result<(), String> {
    let affected = db
        .execute(
            "DELETE FROM qa_jobs WHERE id = ?1 AND project_id = ?2",
            params![qa_job_id, project_id],
        )
        .map_err(|err| err.to_string())?;

    if affected == 0 {
        return Err(format!("Unknown QA job id: {qa_job_id}"));
    }

    Ok(())
}

pub fn run_jobs(
    db: &Connection,
    project_id: i64,
    project_folder: &str,
    query: QaJobQuery,
    trigger_source: &str,
) -> Result<QaRun, String> {
    let mut jobs = load_jobs(db, project_id, Some(&query))?
        .into_iter()
        .filter(|job| job.enabled)
        .collect::<Vec<_>>();
    jobs.sort_by_key(|job| job.number);

    if jobs.is_empty() {
        return Err("No enabled QA jobs matched the run request".to_string());
    }

    let query_snapshot = serde_json::to_string(&query).map_err(|err| err.to_string())?;
    db.execute(
        "INSERT INTO qa_runs(project_id, trigger_source, query_snapshot, status)
         VALUES (?1, ?2, ?3, 'running')",
        params![project_id, trigger_source.trim(), query_snapshot],
    )
    .map_err(|err| err.to_string())?;
    let qa_run_id = db.last_insert_rowid();

    let mut passed = 0;
    let mut failed = 0;
    let mut timed_out = 0;

    for job in jobs {
        let command_snapshot = command_snapshot(&job)?;
        db.execute(
            "INSERT INTO qa_job_runs(
                qa_run_id, qa_job_id, command_snapshot, status, output
             )
             VALUES (?1, ?2, ?3, 'running', '')",
            params![qa_run_id, job.id, command_snapshot],
        )
        .map_err(|err| err.to_string())?;
        let job_run_id = db.last_insert_rowid();

        let evidence = execute_job(&job, project_folder);
        match evidence.status.as_str() {
            "passed" => passed += 1,
            "timed_out" => timed_out += 1,
            _ => failed += 1,
        }

        db.execute(
            "UPDATE qa_job_runs
             SET status = ?1,
                 exit_code = ?2,
                 finished_at = CURRENT_TIMESTAMP,
                 duration_ms = ?3,
                 output = ?4
             WHERE id = ?5",
            params![
                evidence.status,
                evidence.exit_code,
                evidence.duration_ms,
                evidence.output,
                job_run_id
            ],
        )
        .map_err(|err| err.to_string())?;
    }

    let status = if failed == 0 && timed_out == 0 {
        "passed"
    } else {
        "failed"
    };
    let summary = format!("{passed} passed, {failed} failed, {timed_out} timed out");
    db.execute(
        "UPDATE qa_runs
         SET status = ?1,
             finished_at = CURRENT_TIMESTAMP,
             summary = ?2
         WHERE id = ?3",
        params![status, summary, qa_run_id],
    )
    .map_err(|err| err.to_string())?;

    load_run(db, project_id, qa_run_id)
}

pub fn load_runs(
    db: &Connection,
    project_id: i64,
    limit: Option<i64>,
) -> Result<Vec<QaRun>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let mut statement = db
        .prepare(
            "SELECT id, trigger_source, query_snapshot, status, started_at, finished_at, summary
             FROM qa_runs
             WHERE project_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id, limit], read_run_row)
        .map_err(|err| err.to_string())?;

    rows.map(|row| hydrate_run(db, row.map_err(|err| err.to_string())?))
        .collect()
}

pub fn load_run(db: &Connection, project_id: i64, qa_run_id: i64) -> Result<QaRun, String> {
    let row = db
        .query_row(
            "SELECT id, trigger_source, query_snapshot, status, started_at, finished_at, summary
             FROM qa_runs
             WHERE id = ?1 AND project_id = ?2",
            params![qa_run_id, project_id],
            read_run_row,
        )
        .map_err(|err| err.to_string())?;
    hydrate_run(db, row)
}

fn hydrate_job(db: &Connection, _project_id: i64, row: QaJobRow) -> Result<QaJob, String> {
    let latest_run = load_latest_job_run(db, row.id)?;
    let design_specification_links = load_design_links(db, row.id)?;
    let task_links = load_task_links(db, row.id)?;
    let tags = load_tags(db, row.id)?;
    let derived_state = derive_state(db, &row, latest_run.as_ref())?;

    Ok(QaJob {
        id: row.id,
        number: row.number,
        name: row.name,
        description: row.description,
        command: row.command,
        working_directory: row.working_directory,
        shell: row.shell,
        timeout_seconds: row.timeout_seconds,
        enabled: row.enabled,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        derived_state,
        design_specification_links,
        task_links,
        tags,
        latest_run,
    })
}

fn hydrate_run(db: &Connection, row: QaRunRow) -> Result<QaRun, String> {
    Ok(QaRun {
        id: row.id,
        trigger_source: row.trigger_source,
        query_snapshot: row.query_snapshot,
        status: row.status,
        started_at: row.started_at,
        finished_at: row.finished_at,
        summary: row.summary,
        job_runs: load_job_runs_for_run(db, row.id)?,
    })
}

fn derive_state(
    db: &Connection,
    job: &QaJobRow,
    latest_run: Option<&QaJobRun>,
) -> Result<String, String> {
    let Some(latest_run) = latest_run else {
        return Ok("needs-rerun".to_string());
    };

    if latest_run.status == "running" {
        return Ok("running".to_string());
    }

    if latest_run
        .finished_at
        .as_deref()
        .map(|finished_at| finished_at < job.updated_at.as_str())
        .unwrap_or(true)
    {
        return Ok("needs-rerun".to_string());
    }

    let stale_task_link = db
        .query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM qa_job_task_links l
                JOIN agent_tasks t ON t.id = l.task_id
                WHERE l.qa_job_id = ?1
                  AND t.updated_at > ?2
            )",
            params![job.id, latest_run.finished_at.as_deref().unwrap_or("")],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())?
        != 0;
    if stale_task_link {
        return Ok("needs-rerun".to_string());
    }

    match latest_run.status.as_str() {
        "passed" => Ok("green".to_string()),
        "failed" | "timed_out" => Ok("red".to_string()),
        _ => Ok("needs-rerun".to_string()),
    }
}

fn matches_query(job: &QaJob, query: &QaJobQuery) -> bool {
    if let Some(job_ids) = &query.job_ids {
        if !job_ids.contains(&job.id) {
            return false;
        }
    }

    if let Some(enabled) = query.enabled {
        if job.enabled != enabled {
            return false;
        }
    }

    if let Some(states) = &query.states {
        let states = states
            .iter()
            .map(|state| state.trim().to_ascii_lowercase())
            .filter(|state| !state.is_empty())
            .collect::<Vec<_>>();
        if !states.is_empty() && !states.contains(&job.derived_state) {
            return false;
        }
    }

    if let Some(tags) = &query.tags {
        let wanted = tags
            .iter()
            .map(|tag| tag.trim().to_ascii_lowercase())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        if !wanted.is_empty()
            && !wanted.iter().all(|tag| {
                job.tags
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(tag))
            })
        {
            return false;
        }
    }

    if let Some(task_ids) = &query.task_ids {
        if !task_ids.is_empty()
            && !task_ids
                .iter()
                .all(|task_id| job.task_links.iter().any(|link| link.task_id == *task_id))
        {
            return false;
        }
    }

    if let Some(design_external_ids) = &query.design_external_ids {
        let wanted = design_external_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if !wanted.is_empty()
            && !wanted.iter().all(|design_id| {
                job.design_specification_links
                    .iter()
                    .any(|link| link.design_external_id == *design_id)
            })
        {
            return false;
        }
    }

    true
}

fn execute_job(job: &QaJob, project_folder: &str) -> JobEvidence {
    let start = Instant::now();
    let output_path = std::env::temp_dir().join(format!(
        "adashi-qa-{}-{}.log",
        job.id,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    ));

    let output_file = match fs::File::create(&output_path) {
        Ok(file) => file,
        Err(err) => {
            return JobEvidence {
                status: "failed".to_string(),
                exit_code: None,
                duration_ms: Some(start.elapsed().as_millis() as i64),
                output: format!("Failed to create QA output capture file: {err}"),
            };
        }
    };

    let mut command = shell_command(job);
    command.current_dir(resolve_working_directory(
        project_folder,
        &job.working_directory,
    ));
    command.stdout(Stdio::from(output_file));
    if let Ok(stderr_file) = fs::OpenOptions::new().append(true).open(&output_path) {
        command.stderr(Stdio::from(stderr_file));
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let _ = fs::remove_file(&output_path);
            return JobEvidence {
                status: "failed".to_string(),
                exit_code: None,
                duration_ms: Some(start.elapsed().as_millis() as i64),
                output: format!("Failed to start QA command: {err}"),
            };
        }
    };

    let timeout = Duration::from_secs(job.timeout_seconds.max(1) as u64);
    let mut timed_out = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() >= timeout => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    let mut output = fs::read_to_string(&output_path).unwrap_or_default();
    let _ = fs::remove_file(&output_path);
    if output.len() > OUTPUT_LIMIT {
        output.truncate(OUTPUT_LIMIT);
        output.push_str("\n\n[Output truncated by Adashi QA capture]");
    }

    let exit_code = exit_status.and_then(|status| status.code().map(i64::from));
    let status = if timed_out {
        "timed_out"
    } else if exit_status.map(|status| status.success()).unwrap_or(false) {
        "passed"
    } else {
        "failed"
    };

    JobEvidence {
        status: status.to_string(),
        exit_code,
        duration_ms: Some(start.elapsed().as_millis() as i64),
        output,
    }
}

fn shell_command(job: &QaJob) -> Command {
    let shell = job.shell.trim().to_ascii_lowercase();
    match shell.as_str() {
        "cmd" | "cmd.exe" => {
            let mut command = Command::new("cmd.exe");
            command.arg("/C").arg(&job.command);
            command
        }
        "pwsh" | "pwsh.exe" => {
            let mut command = Command::new("pwsh.exe");
            command.arg("-NoProfile").arg("-Command").arg(&job.command);
            command
        }
        "sh" | "bash" => {
            let mut command = Command::new(shell);
            command.arg("-c").arg(&job.command);
            command
        }
        _ => {
            let mut command = Command::new("powershell.exe");
            command.arg("-NoProfile").arg("-Command").arg(&job.command);
            command
        }
    }
}

fn resolve_working_directory(project_folder: &str, working_directory: &str) -> PathBuf {
    let trimmed = working_directory.trim();
    if trimmed.is_empty() {
        return PathBuf::from(project_folder);
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        PathBuf::from(project_folder).join(path)
    }
}

fn command_snapshot(job: &QaJob) -> Result<String, String> {
    serde_json::to_string(&json!({
        "name": job.name,
        "command": job.command,
        "workingDirectory": job.working_directory,
        "shell": job.shell,
        "timeoutSeconds": job.timeout_seconds,
        "enabled": job.enabled,
        "tags": job.tags,
        "designSpecificationLinks": job.design_specification_links,
        "taskLinks": job.task_links,
    }))
    .map_err(|err| err.to_string())
}

fn load_job_rows(db: &Connection, project_id: i64) -> Result<Vec<QaJobRow>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, number, name, description, command, working_directory, shell,
                    timeout_seconds, enabled, created_by, created_at, updated_at
             FROM qa_jobs
             WHERE project_id = ?1
             ORDER BY number",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], read_job_row)
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn load_job_row(db: &Connection, project_id: i64, qa_job_id: i64) -> Result<QaJobRow, String> {
    db.query_row(
        "SELECT id, number, name, description, command, working_directory, shell,
                timeout_seconds, enabled, created_by, created_at, updated_at
         FROM qa_jobs
         WHERE id = ?1 AND project_id = ?2",
        params![qa_job_id, project_id],
        read_job_row,
    )
    .map_err(|err| err.to_string())
}

fn read_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<QaJobRow> {
    Ok(QaJobRow {
        id: row.get(0)?,
        number: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        command: row.get(4)?,
        working_directory: row.get(5)?,
        shell: row.get(6)?,
        timeout_seconds: row.get(7)?,
        enabled: row.get::<_, i64>(8)? != 0,
        created_by: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn load_design_links(db: &Connection, qa_job_id: i64) -> Result<Vec<QaJobDesignLink>, String> {
    let mut statement = db
        .prepare(
            "SELECT
                l.id,
                l.qa_job_id,
                l.sort_order,
                l.target_type,
                l.design_external_id,
                COALESCE(e.name, r.description, d.title, l.design_external_id) AS title
             FROM qa_job_design_links l
             LEFT JOIN c4_elements e ON e.external_id = l.design_external_id
             LEFT JOIN c4_relationships r ON r.external_id = l.design_external_id
             LEFT JOIN diagrams d ON d.key = l.design_external_id
             WHERE l.qa_job_id = ?1
             ORDER BY l.sort_order, l.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![qa_job_id], |row| {
            Ok(QaJobDesignLink {
                id: row.get(0)?,
                qa_job_id: row.get(1)?,
                sort_order: row.get(2)?,
                target_type: row.get(3)?,
                design_external_id: row.get(4)?,
                title: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn replace_design_links(
    db: &Connection,
    qa_job_id: i64,
    links: Vec<QaDesignLinkInput>,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM qa_job_design_links WHERE qa_job_id = ?1",
        params![qa_job_id],
    )
    .map_err(|err| err.to_string())?;

    for (index, link) in links.into_iter().enumerate() {
        let design_external_id = required_trimmed(&link.design_external_id, "Design link id")?;
        let target_type = match link.target_type {
            Some(target_type) if !target_type.trim().is_empty() => target_type.trim().to_string(),
            _ => infer_design_target_type(db, design_external_id)?,
        };
        validate_design_target_type(&target_type)?;

        db.execute(
            "INSERT INTO qa_job_design_links(
                qa_job_id, sort_order, target_type, design_external_id
             )
             VALUES (?1, ?2, ?3, ?4)",
            params![qa_job_id, index as i64, target_type, design_external_id],
        )
        .map_err(|err| err.to_string())?;
    }

    touch_job(db, qa_job_id)
}

fn load_task_links(db: &Connection, qa_job_id: i64) -> Result<Vec<QaJobTaskLink>, String> {
    let mut statement = db
        .prepare(
            "SELECT l.id, l.qa_job_id, l.task_id, l.sort_order, t.title, t.number, t.state
             FROM qa_job_task_links l
             JOIN agent_tasks t ON t.id = l.task_id
             WHERE l.qa_job_id = ?1
             ORDER BY l.sort_order, l.id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![qa_job_id], |row| {
            Ok(QaJobTaskLink {
                id: row.get(0)?,
                qa_job_id: row.get(1)?,
                task_id: row.get(2)?,
                sort_order: row.get(3)?,
                title: row.get(4)?,
                number: row.get(5)?,
                state: row.get(6)?,
            })
        })
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn replace_task_links(
    db: &Connection,
    project_id: i64,
    qa_job_id: i64,
    task_ids: Vec<i64>,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM qa_job_task_links WHERE qa_job_id = ?1",
        params![qa_job_id],
    )
    .map_err(|err| err.to_string())?;

    for (index, task_id) in task_ids.into_iter().enumerate() {
        let exists = db
            .query_row(
                "SELECT 1 FROM agent_tasks WHERE id = ?1 AND project_id = ?2 LIMIT 1",
                params![task_id, project_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|err| err.to_string())?
            .is_some();
        if !exists {
            return Err(format!("Unknown task id for QA link: {task_id}"));
        }

        db.execute(
            "INSERT OR IGNORE INTO qa_job_task_links(qa_job_id, task_id, sort_order)
             VALUES (?1, ?2, ?3)",
            params![qa_job_id, task_id, index as i64],
        )
        .map_err(|err| err.to_string())?;
    }

    touch_job(db, qa_job_id)
}

fn load_tags(db: &Connection, qa_job_id: i64) -> Result<Vec<String>, String> {
    let mut statement = db
        .prepare("SELECT tag FROM qa_job_tags WHERE qa_job_id = ?1 ORDER BY tag")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![qa_job_id], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn replace_tags(db: &Connection, qa_job_id: i64, tags: Vec<String>) -> Result<(), String> {
    db.execute(
        "DELETE FROM qa_job_tags WHERE qa_job_id = ?1",
        params![qa_job_id],
    )
    .map_err(|err| err.to_string())?;

    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }

        db.execute(
            "INSERT OR IGNORE INTO qa_job_tags(qa_job_id, tag) VALUES (?1, ?2)",
            params![qa_job_id, tag],
        )
        .map_err(|err| err.to_string())?;
    }

    touch_job(db, qa_job_id)
}

fn load_latest_job_run(db: &Connection, qa_job_id: i64) -> Result<Option<QaJobRun>, String> {
    db.query_row(
        "SELECT id, qa_run_id, qa_job_id, command_snapshot, status, exit_code,
                started_at, finished_at, duration_ms, output
         FROM qa_job_runs
         WHERE qa_job_id = ?1
         ORDER BY id DESC
         LIMIT 1",
        params![qa_job_id],
        read_job_run,
    )
    .optional()
    .map_err(|err| err.to_string())
}

fn load_job_runs_for_run(db: &Connection, qa_run_id: i64) -> Result<Vec<QaJobRun>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, qa_run_id, qa_job_id, command_snapshot, status, exit_code,
                    started_at, finished_at, duration_ms, output
             FROM qa_job_runs
             WHERE qa_run_id = ?1
             ORDER BY id",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![qa_run_id], read_job_run)
        .map_err(|err| err.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

fn read_job_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<QaJobRun> {
    Ok(QaJobRun {
        id: row.get(0)?,
        qa_run_id: row.get(1)?,
        qa_job_id: row.get(2)?,
        command_snapshot: row.get(3)?,
        status: row.get(4)?,
        exit_code: row.get(5)?,
        started_at: row.get(6)?,
        finished_at: row.get(7)?,
        duration_ms: row.get(8)?,
        output: row.get(9)?,
    })
}

fn read_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<QaRunRow> {
    Ok(QaRunRow {
        id: row.get(0)?,
        trigger_source: row.get(1)?,
        query_snapshot: row.get(2)?,
        status: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        summary: row.get(6)?,
    })
}

fn infer_design_target_type(db: &Connection, design_external_id: &str) -> Result<String, String> {
    let element_exists = db
        .query_row(
            "SELECT 1 FROM c4_elements WHERE external_id = ?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if element_exists {
        return Ok("element".to_string());
    }

    let relationship_exists = db
        .query_row(
            "SELECT 1 FROM c4_relationships WHERE external_id = ?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if relationship_exists {
        return Ok("relationship".to_string());
    }

    let diagram_exists = db
        .query_row(
            "SELECT 1 FROM diagrams WHERE key = ?1 LIMIT 1",
            params![design_external_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .is_some();
    if diagram_exists {
        return Ok("uml".to_string());
    }

    Err(format!(
        "Unknown design specification id: {design_external_id}"
    ))
}

fn validate_design_target_type(target_type: &str) -> Result<(), String> {
    const TARGET_TYPES: &[&str] = &["element", "relationship", "uml"];
    if TARGET_TYPES.contains(&target_type) {
        Ok(())
    } else {
        Err(format!(
            "Invalid design link target type '{target_type}'. Expected one of: {}",
            TARGET_TYPES.join(", ")
        ))
    }
}

fn validate_timeout(timeout_seconds: i64) -> Result<i64, String> {
    if (1..=86_400).contains(&timeout_seconds) {
        Ok(timeout_seconds)
    } else {
        Err("QA timeout must be between 1 and 86400 seconds".to_string())
    }
}

fn normalize_shell(shell: Option<&str>) -> String {
    let shell = shell.unwrap_or(DEFAULT_SHELL).trim();
    if shell.is_empty() {
        DEFAULT_SHELL.to_string()
    } else {
        shell.to_string()
    }
}

fn required_trimmed<'a>(value: &'a str, label: &str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(format!("{label} is required"))
    } else {
        Ok(trimmed)
    }
}

fn next_job_number(db: &Connection, project_id: i64) -> Result<i64, String> {
    db.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM qa_jobs WHERE project_id = ?1",
        params![project_id],
        |row| row.get(0),
    )
    .map_err(|err| err.to_string())
}

fn touch_job(db: &Connection, qa_job_id: i64) -> Result<(), String> {
    db.execute(
        "UPDATE qa_jobs SET updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![qa_job_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn state_rank(state: &str) -> i32 {
    match state {
        "running" => 0,
        "red" => 1,
        "needs-rerun" => 2,
        "green" => 3,
        _ => 4,
    }
}

struct JobEvidence {
    status: String,
    exit_code: Option<i64>,
    duration_ms: Option<i64>,
    output: String,
}

struct QaJobRow {
    id: i64,
    number: i64,
    name: String,
    description: String,
    command: String,
    working_directory: String,
    shell: String,
    timeout_seconds: i64,
    enabled: bool,
    created_by: String,
    created_at: String,
    updated_at: String,
}

struct QaRunRow {
    id: i64,
    trigger_source: String,
    query_snapshot: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    summary: String,
}
