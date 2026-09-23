//! Shared QA process runner. Storage commits reserve work and persist evidence;
//! shell commands never execute inside a database transaction.
use adashi_storage_api::{qa::*, *};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const OUTPUT_LIMIT: usize = 200_000;

pub(crate) fn run(
    store: &mut dyn ProjectStorage,
    folder: &str,
    operation: &str,
    query: QaJobQuery,
    trigger: &str,
) -> StorageResult<QaRun> {
    let query_json = serde_json::to_string(&query).map_err(StorageError::backend)?;
    {
        let snapshot = store.snapshot()?;
        if let Some(receipt) = snapshot.receipt(operation)? {
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
        operation_id: operation.into(),
        changes: vec![Change::Qa(QaWrite::StartRun {
            query,
            trigger_source: trigger.into(),
            jobs: plans,
        })],
    })?;
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
    for reserved in &run.job_runs {
        let claim = store.commit(Mutation {
            operation_id: format!("{operation}:claim:{}:{worker}", reserved.id),
            changes: vec![Change::Qa(QaWrite::ClaimJob {
                job_run_id: reserved.id,
                expected_version: 1,
            })],
        });
        match claim {
            Ok(_) => {}
            Err(StorageError::Conflict(_)) => return store.snapshot()?.qa_run(run.id),
            Err(e) => return Err(e),
        }
        let job = jobs
            .iter()
            .find(|job| job.id == reserved.qa_job_id)
            .ok_or_else(|| StorageError::Backend("Reserved QA definition is missing".into()))?;
        let evidence = execute_job(job, folder);
        let outcome = match evidence.status.as_str() {
            "passed" => QaJobOutcome::Passed,
            "timed_out" => QaJobOutcome::TimedOut,
            _ => QaJobOutcome::Failed,
        };
        store.commit(Mutation {
            operation_id: format!("{operation}:evidence:{}", reserved.id),
            changes: vec![Change::Qa(QaWrite::CompleteJob {
                job_run_id: reserved.id,
                expected_version: 2,
                evidence: QaEvidence {
                    outcome,
                    exit_code: evidence.exit_code,
                    duration_ms: evidence.duration_ms.unwrap_or(0),
                    output: evidence.output,
                },
            })],
        })?;
    }
    let result = store.commit(Mutation {
        operation_id: format!("{operation}:complete"),
        changes: vec![Change::Qa(QaWrite::CompleteRun {
            run_id: run.id,
            expected_version: 1,
        })],
    })?;
    let Some(ChangeOutcome::QaRun(run)) = result.outcomes.into_iter().next() else {
        return Err(StorageError::Backend(
            "QA completion returned an unexpected outcome".into(),
        ));
    };
    Ok(run)
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
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
        let mut end = OUTPUT_LIMIT;
        while !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
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
