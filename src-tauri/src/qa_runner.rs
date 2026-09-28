//! Shared QA process runner. Storage commits reserve work and persist evidence;
//! shell commands never execute inside a database transaction.
//!
//! Enforcement lives here, not in a transport, so the desktop and MCP paths
//! cannot diverge: a run must name what it selects, stays inside a wall-clock
//! budget, reserves at most `QA_RUN_MAX_JOBS` jobs, skips already-green jobs
//! unless forced, and kills the whole process tree on timeout or cancellation.
use adashi_storage_api::{qa::*, *};
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const OUTPUT_LIMIT: usize = 200_000;

/// Options that shape one run.
pub(crate) struct RunRequest {
    pub operation: String,
    pub query: QaJobQuery,
    pub trigger: String,
    /// Permit a run whose selection is the all-default (every enabled job) query.
    pub allow_broad_run: bool,
    /// Run jobs even when their latest evidence is already green.
    pub force: bool,
    /// Wall-clock budget for the whole run; clamped to the server maximum.
    pub max_duration_seconds: Option<i64>,
}

pub(crate) fn run(
    store: &mut dyn ProjectStorage,
    folder: &str,
    request: RunRequest,
) -> StorageResult<QaRun> {
    let RunRequest {
        operation,
        query,
        trigger,
        allow_broad_run,
        force,
        max_duration_seconds,
    } = request;
    let query_json = serde_json::to_string(&query).map_err(StorageError::backend)?;
    {
        let snapshot = store.snapshot()?;
        if let Some(receipt) = snapshot.receipt(&operation)? {
            let OperationReceipt::V1 { result, .. } = receipt else {
                return Err(StorageError::OperationReused);
            };
            let Some(ChangeOutcome::QaRun(run)) = result.outcomes.first() else {
                return Err(StorageError::OperationReused);
            };
            if run.query_snapshot != query_json || run.trigger_source != trigger {
                return Err(StorageError::OperationReused);
            }
            return snapshot.qa_run(run.id);
        }
    }

    let mut jobs = store
        .snapshot()?
        .qa_jobs(&query)?
        .into_iter()
        .filter(|j| j.enabled)
        .collect::<Vec<_>>();
    jobs.sort_by_key(|j| j.number);

    // An all-default query is a project-wide run; require the caller to say so.
    if !query.requests_explicit_selection() && !allow_broad_run {
        return Err(StorageError::Validation(format!(
            "run_jobs needs an explicit selection (jobIds, states, tags, taskIds or \
             designExternalIds); the empty query matches {} enabled job(s). \
             Pass allowBroadRun=true only for a deliberate project-wide run.",
            jobs.len()
        )));
    }
    if jobs.len() > QA_RUN_MAX_JOBS {
        return Err(StorageError::Validation(format!(
            "A QA run may execute at most {QA_RUN_MAX_JOBS} jobs, got {}. Narrow the selection.",
            jobs.len()
        )));
    }
    if jobs.is_empty() {
        return Err(StorageError::Validation(
            "No enabled QA jobs matched the run request".into(),
        ));
    }

    // Already-green work is not re-executed unless the caller forces it. This is
    // what stops a release aggregate from re-running every gate it subsumes.
    let up_to_date = if force {
        0
    } else {
        let before = jobs.len();
        jobs.retain(|job| job.derived_state != "green");
        before - jobs.len()
    };
    if jobs.is_empty() {
        return Err(StorageError::Validation(format!(
            "All {up_to_date} selected QA job(s) are already green; pass force=true to run them again"
        )));
    }

    let budget = max_duration_seconds
        .unwrap_or(QA_RUN_DEFAULT_BUDGET_SECONDS)
        .clamp(1, QA_RUN_MAX_BUDGET_SECONDS);
    let deadline = Instant::now() + Duration::from_secs(budget as u64);

    let plans = jobs
        .iter()
        .map(|job| {
            Ok(QaExecutionPlan {
                job_id: job.id,
                expected_version: job.version,
                command_snapshot: command_snapshot(job).map_err(StorageError::Validation)?,
            })
        })
        .collect::<StorageResult<Vec<_>>>()?;
    let result = store.commit(Mutation {
        operation_id: operation.clone(),
        changes: vec![Change::Qa(QaWrite::StartRun {
            query,
            trigger_source: trigger.clone(),
            jobs: plans,
        })],
    })?;
    let reservation_versions = result.versions;
    let Some(ChangeOutcome::QaRun(run)) = result.outcomes.into_iter().next() else {
        return Err(StorageError::Backend(
            "QA reservation returned an unexpected outcome".into(),
        ));
    };
    let worker = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(StorageError::backend)?
            .as_nanos()
    );
    let cancel = register_cancel(run.id);
    let _cancel_guard = CancelGuard(run.id);

    for reserved in &run.job_runs {
        let expected_version = reservation_versions
            .iter()
            .find(|v| v.resource_kind == "qa.job-run" && v.resource_id == reserved.id.to_string())
            .map(|v| v.version)
            .ok_or_else(|| StorageError::Backend("Missing reservation version".into()))?;
        let job = jobs
            .iter()
            .find(|job| job.id == reserved.qa_job_id)
            .ok_or_else(|| StorageError::Backend("Reserved QA definition is missing".into()))?;

        let remaining = deadline.saturating_duration_since(Instant::now());
        if cancel.load(Ordering::SeqCst) || remaining < Duration::from_secs(1) {
            let (outcome, output) = if cancel.load(Ordering::SeqCst) {
                (
                    QaJobOutcome::Cancelled,
                    "[Adashi] run cancelled before this job started",
                )
            } else {
                (
                    QaJobOutcome::Skipped,
                    "[Adashi] skipped: run budget exhausted before this job started",
                )
            };
            finish_job(
                store,
                &operation,
                reserved.id,
                expected_version,
                outcome,
                None,
                0,
                output,
            )?;
            continue;
        }

        let claim = store.commit(Mutation {
            operation_id: format!("{operation}:claim:{}:{worker}", reserved.id),
            changes: vec![Change::Qa(QaWrite::ClaimJob {
                job_run_id: reserved.id,
                expected_version,
            })],
        });
        let claimed_version = match claim {
            Ok(result) => result
                .versions
                .iter()
                .find(|v| {
                    v.resource_kind == "qa.job-run" && v.resource_id == reserved.id.to_string()
                })
                .map(|v| v.version)
                .ok_or_else(|| StorageError::Backend("QA claim returned no version".into()))?,
            Err(StorageError::Conflict(_)) => return store.snapshot()?.qa_run(run.id),
            Err(e) => return Err(e),
        };

        // Cap the job's own timeout by what remains of the run budget so one slow
        // job cannot consume the whole run.
        let effective_timeout = (remaining.as_secs() as i64).clamp(1, job.timeout_seconds);
        let evidence = execute_job(job, folder, effective_timeout, &cancel);
        let outcome = match evidence.status.as_str() {
            "passed" => QaJobOutcome::Passed,
            "timed_out" => QaJobOutcome::TimedOut,
            "cancelled" => QaJobOutcome::Cancelled,
            _ => QaJobOutcome::Failed,
        };
        finish_job(
            store,
            &operation,
            reserved.id,
            claimed_version,
            outcome,
            evidence.exit_code,
            evidence.duration_ms.unwrap_or(0),
            evidence.output,
        )?;
    }

    let expected_version = resource_version(store, "qa.run", run.id)?;
    let result = store.commit(Mutation {
        operation_id: format!("{operation}:complete"),
        changes: vec![Change::Qa(QaWrite::CompleteRun {
            run_id: run.id,
            expected_version,
        })],
    })?;
    let Some(ChangeOutcome::QaRun(run)) = result.outcomes.into_iter().next() else {
        return Err(StorageError::Backend(
            "QA completion returned an unexpected outcome".into(),
        ));
    };
    Ok(run)
}

#[allow(clippy::too_many_arguments)]
fn finish_job(
    store: &mut dyn ProjectStorage,
    operation: &str,
    job_run_id: i64,
    expected_version: i64,
    outcome: QaJobOutcome,
    exit_code: Option<i64>,
    duration_ms: i64,
    output: impl Into<String>,
) -> StorageResult<()> {
    store.commit(Mutation {
        operation_id: format!("{operation}:evidence:{job_run_id}"),
        changes: vec![Change::Qa(QaWrite::CompleteJob {
            job_run_id,
            expected_version,
            evidence: QaEvidence {
                outcome,
                exit_code,
                duration_ms,
                output: output.into(),
            },
        })],
    })?;
    Ok(())
}

fn resource_version(store: &mut dyn ProjectStorage, kind: &str, id: i64) -> StorageResult<i64> {
    store
        .snapshot()?
        .resource_versions(&[ResourceKey {
            kind: kind.into(),
            id: id.to_string(),
        }])?
        .first()
        .map(|v| v.version)
        .ok_or_else(|| StorageError::Backend("Missing QA resource version".into()))
}

/// Cancellation is advisory and in-process: a `cancel_run` call sets the flag,
/// and the worker's poll loop kills the process tree on its next tick.
fn cancel_registry() -> &'static Mutex<HashMap<i64, Arc<AtomicBool>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<i64, Arc<AtomicBool>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_cancel(run_id: i64) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    cancel_registry()
        .lock()
        .expect("qa cancel registry")
        .insert(run_id, flag.clone());
    flag
}

fn release_cancel(run_id: i64) {
    cancel_registry()
        .lock()
        .expect("qa cancel registry")
        .remove(&run_id);
}

/// Releases the run's cancel registration however the worker exits.
struct CancelGuard(i64);

impl Drop for CancelGuard {
    fn drop(&mut self) {
        release_cancel(self.0);
    }
}

/// Signals a live run in this process. Returns false when the run is not active
/// here (already finished, or owned by another process).
pub(crate) fn request_cancel(run_id: i64) -> bool {
    match cancel_registry()
        .lock()
        .expect("qa cancel registry")
        .get(&run_id)
    {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

fn execute_job(
    job: &QaJob,
    project_folder: &str,
    timeout_seconds: i64,
    cancel: &AtomicBool,
) -> JobEvidence {
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        // Give the child its own process group so a timeout can kill every
        // descendant (cargo/vite/tauri), not just the shell.
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
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

    let timeout = Duration::from_secs(timeout_seconds.max(1) as u64);
    let mut timed_out = false;
    let mut cancelled = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if cancel.load(Ordering::SeqCst) => {
                cancelled = true;
                kill_tree(&mut child);
                break child.wait().ok();
            }
            Ok(None) if start.elapsed() >= timeout => {
                timed_out = true;
                kill_tree(&mut child);
                break child.wait().ok();
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };

    let mut output = fs::read_to_string(&output_path).unwrap_or_default();
    let _ = fs::remove_file(&output_path);
    if output.len() > OUTPUT_LIMIT {
        let mut end = OUTPUT_LIMIT;
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        output.push_str("\n\n[Output truncated by Adashi QA capture]");
    }

    let exit_code = exit_status.and_then(|status| status.code().map(i64::from));
    let status = if cancelled {
        "cancelled"
    } else if timed_out {
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

/// Kills the child and every process in its group/tree.
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // The child leads its own process group, so the negative pid targets it
        // and all descendants.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
    let _ = child.kill();
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
            #[cfg(windows)]
            let executable = "pwsh.exe";
            #[cfg(not(windows))]
            let executable = "pwsh";
            let mut command = Command::new(executable);
            command.arg("-NoProfile").arg("-Command").arg(&job.command);
            command
        }
        "sh" | "bash" => {
            let mut command = Command::new(shell);
            command.arg("-c").arg(&job.command);
            command
        }
        "powershell" | "powershell.exe" => {
            #[cfg(windows)]
            let executable = "powershell.exe";
            #[cfg(not(windows))]
            let executable = "pwsh";
            let mut command = Command::new(executable);
            command.arg("-NoProfile").arg("-Command").arg(&job.command);
            command
        }
        _ if cfg!(windows) => {
            let mut command = Command::new("powershell.exe");
            command.arg("-NoProfile").arg("-Command").arg(&job.command);
            command
        }
        _ => {
            let mut command = Command::new("bash");
            command.arg("-c").arg(&job.command);
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

pub(crate) fn command_snapshot(job: &QaJob) -> Result<String, String> {
    serde_json::to_string(&json!({
        "name": job.name,
        "kind": job.kind,
        "scope": job.scope,
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

struct JobEvidence {
    status: String,
    exit_code: Option<i64>,
    duration_ms: Option<i64>,
    output: String,
}
