//! Design-to-code correspondence: a deterministic sensor for the formal design.
//!
//! The design is a second description of the system. Nothing has ever connected it to the first
//! one, so it could drift silently and therefore drifted freely. This module answers two questions
//! that need no model, no reader and no language knowledge:
//!
//! 1. **Is the element attached to the model?** A parent holds it, a relationship names it as an
//!    endpoint, or it holds children. An element with none of those is floating in the hierarchy.
//! 2. **Do the files it claims exist?** Every binding resolves to a real file inside the project
//!    folder.
//!
//! From those two facts, four states — and each one has a different next action, which is what
//! makes the states worth having:
//!
//! | Attached | File links | State | Next action |
//! | --- | --- | --- | --- |
//! | no | — | `orphaned` | attach it to the model |
//! | yes | none | `unmapped` | bind it to the code that implements it |
//! | yes | any broken | `broken` | fix the binding |
//! | yes | all resolve | `resolved` | nothing |
//!
//! Three deliberate limits, stated so the field is not over-read:
//!
//! - **`unmapped` is not "not implemented".** It means nothing is *claimed* about where the code
//!   lives. An unbound element may be fully built, and reading it as absent work invites building
//!   what already exists.
//! - **A resolved pointer is not a correct implementation.** These checks prove a pointer, never a
//!   correspondence between the code and the responsibility. Establishing that needs either
//!   language-aware analysis or a reader, and neither is here.
//! - **No language is parsed.** A symbol binding is verified by finding its name as a whole word in
//!   its file, so no language can be misjudged — and a name that is only data, in a string or a
//!   comment, does not count as a definition.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ElementHealth {
    /// Attached to the model, but no binding: its code cannot be located.
    Unmapped,
    /// No parent and no relationship names it: floating in the hierarchy.
    Orphaned,
    /// Attached, with at least one binding that does not resolve.
    Broken,
    /// Attached, with bindings that all resolve.
    Resolved,
}

impl ElementHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unmapped => "unmapped",
            Self::Orphaned => "orphaned",
            Self::Broken => "broken",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unmapped" => Some(Self::Unmapped),
            "orphaned" => Some(Self::Orphaned),
            "broken" => Some(Self::Broken),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }

    /// Whether the model's claim is currently unsupported by the code. These are the states a task
    /// close has to address or knowingly keep.
    #[allow(dead_code)]
    pub fn needs_attention(self) -> bool {
        !matches!(self, Self::Resolved)
    }

    /// Most actionable first, so a listing reads as a work queue.
    fn weight(self) -> u8 {
        match self {
            Self::Broken => 0,
            Self::Orphaned => 1,
            Self::Unmapped => 2,
            Self::Resolved => 3,
        }
    }

    /// The next action, in words, so the state does not need a legend.
    pub fn next_action(self) -> &'static str {
        match self {
            Self::Orphaned => "Attach it to the model: give it a parent or a relationship.",
            Self::Unmapped => "Bind it to the file that implements it.",
            Self::Broken => "Fix the binding: what it names is not there.",
            Self::Resolved => "Nothing to do.",
        }
    }
}

#[derive(Clone, Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrokenBinding {
    pub design_external_id: String,
    pub target_type: String,
    pub target: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementFinding {
    pub design_external_id: String,
    pub name: String,
    pub element_type: String,
    pub state: ElementHealth,
    /// What to do about it, in one line.
    pub next_action: String,
    /// Files the element's bindings resolve to.
    pub files: Vec<String>,
    /// Bindings that name something that is not there.
    pub broken: Vec<BrokenBinding>,
    /// Human-readable summary of why the element is in this state.
    pub detail: String,
    /// Findings reviewed and knowingly kept, with their reasons.
    pub waivers: Vec<HealthWaiver>,
}

#[derive(Clone, Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthWaiver {
    pub id: i64,
    pub state: String,
    pub reason: String,
    pub task_id: Option<i64>,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthCounts {
    pub orphaned: u32,
    pub unmapped: u32,
    pub broken: u32,
    pub resolved: u32,
    /// Individual bindings that do not resolve, which can exceed the number of broken elements.
    pub broken_bindings: u32,
    pub waivers: u32,
    pub elements: u32,
}

impl HealthCounts {
    /// Elements whose claim is not currently supported by the code. The close gate reads this;
    /// until the gate lands, it is exercised by the tests.
    #[allow(dead_code)]
    pub fn needs_attention(&self) -> u32 {
        self.orphaned + self.unmapped + self.broken
    }
}

#[derive(Clone, Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignHealthResult {
    pub counts: HealthCounts,
    /// Every element, most actionable first, so a client can list or filter by state without a
    /// second call.
    pub elements: Vec<ElementFinding>,
    /// One line stating what the numbers mean, so a reader does not have to infer it.
    pub summary: String,
    /// What these checks do not establish, stated in the result itself.
    pub not_checked: String,
}

/// What the model says, as the scan needs it.
struct Model {
    /// `(external_id, name, element_type, parent_external_id)`
    elements: Vec<(String, String, String, Option<String>)>,
    /// `(design_external_id, target_type, target)`
    bindings: Vec<(String, String, String)>,
    /// Every external id named by at least one relationship, either end, plus every id that holds
    /// children. A top-level element is reached through its children rather than through a parent,
    /// and a relationship that happens to name nothing at the top must not orphan it.
    connected: BTreeSet<String>,
}

fn collect_model(db: &Connection) -> Result<Model, String> {
    let mut element_statement = db
        .prepare(
            "SELECT e.external_id, e.name, e.element_type, e.parent_external_id
             FROM c4_elements e
             JOIN design_workspaces w ON w.id = e.workspace_id
             ORDER BY e.id",
        )
        .map_err(|err| err.to_string())?;
    let elements = element_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    let mut binding_statement = db
        .prepare(
            "SELECT b.design_external_id, b.target_type, b.target
             FROM design_bindings b
             JOIN design_workspaces w ON w.id = b.workspace_id
             ORDER BY b.target_type, b.target",
        )
        .map_err(|err| err.to_string())?;
    let bindings = binding_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    let mut connected = BTreeSet::new();
    let mut relationship_statement = db
        .prepare(
            "SELECT r.source_external_id, r.destination_external_id
             FROM c4_relationships r
             JOIN design_workspaces w ON w.id = r.workspace_id",
        )
        .map_err(|err| err.to_string())?;
    let relationships = relationship_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;
    for (source, destination) in relationships {
        connected.insert(source);
        connected.insert(destination);
    }
    // An element that holds children is attached: it is reached from above by descent, which is
    // how a top-level Software System is placed when no relationship names it.
    for (_, _, _, parent) in &elements {
        if let Some(parent) = parent {
            connected.insert(parent.clone());
        }
    }

    Ok(Model {
        elements,
        bindings,
        connected,
    })
}

/// Runs the scan against a project folder and records the result.
pub fn scan_and_record(
    db: &Connection,
    project_id: i64,
    project_folder: &Path,
) -> Result<DesignHealthResult, String> {
    let result = scan(project_folder, collect_model(db)?)?;
    record(db, project_id, &result)?;
    Ok(result)
}

/// The pure scan: model plus files in, findings out. No database, no clock, no side effects.
fn scan(project_folder: &Path, model: Model) -> Result<DesignHealthResult, String> {
    let mut findings = Vec::new();

    for (external_id, name, element_type, parent_external_id) in &model.elements {
        let attached = parent_external_id.is_some() || model.connected.contains(external_id.as_str());

        let own_bindings = model
            .bindings
            .iter()
            .filter(|(design_external_id, _, _)| design_external_id == external_id)
            .collect::<Vec<_>>();

        let own_targets = |target_type: &str| -> Vec<String> {
            own_bindings
                .iter()
                .filter(|(_, kind, _)| kind == target_type)
                .map(|(_, _, target)| target.trim().to_string())
                .collect()
        };
        let file_targets = own_targets("file");
        let symbol_targets = own_targets("symbol");

        let mut files = Vec::new();
        let mut broken = Vec::new();
        for target in &file_targets {
            match resolve_file(project_folder, target) {
                Some(path) => files.push(relative_display(project_folder, &path)),
                None => broken.push(BrokenBinding {
                    design_external_id: external_id.clone(),
                    target_type: "file".to_string(),
                    target: target.clone(),
                    detail: "file not found inside the project folder".to_string(),
                }),
            }
        }

        for target in &symbol_targets {
            if !symbol_resolves(project_folder, target, &file_targets, &files) {
                broken.push(BrokenBinding {
                    design_external_id: external_id.clone(),
                    target_type: "symbol".to_string(),
                    target: target.clone(),
                    detail: "symbol not found in its file".to_string(),
                });
            }
        }

        let (state, detail) = if !attached {
            (
                ElementHealth::Orphaned,
                "No parent and no relationship names it, so the model cannot place it.".to_string(),
            )
        } else if file_targets.is_empty() && symbol_targets.is_empty() {
            (
                ElementHealth::Unmapped,
                "No file or symbol binding, so nothing is claimed about where its code lives."
                    .to_string(),
            )
        } else if !broken.is_empty() {
            (
                ElementHealth::Broken,
                format!(
                    "{} binding(s) do not resolve: {}",
                    broken.len(),
                    broken
                        .iter()
                        .map(|binding| binding.target.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        } else {
            (
                ElementHealth::Resolved,
                "Attached, and every binding resolves.".to_string(),
            )
        };

        findings.push(ElementFinding {
            design_external_id: external_id.clone(),
            name: name.clone(),
            element_type: element_type.clone(),
            state,
            next_action: state.next_action().to_string(),
            files,
            broken,
            detail,
            waivers: Vec::new(),
        });
    }

    findings.sort_by(|left, right| {
        left.state
            .weight()
            .cmp(&right.state.weight())
            .then(left.design_external_id.cmp(&right.design_external_id))
    });

    let broken_bindings = findings.iter().map(|finding| finding.broken.len() as u32).sum();
    let counts = HealthCounts {
        orphaned: findings.iter().filter(|f| f.state == ElementHealth::Orphaned).count() as u32,
        unmapped: findings.iter().filter(|f| f.state == ElementHealth::Unmapped).count() as u32,
        broken: findings.iter().filter(|f| f.state == ElementHealth::Broken).count() as u32,
        resolved: findings.iter().filter(|f| f.state == ElementHealth::Resolved).count() as u32,
        broken_bindings,
        waivers: 0,
        elements: findings.len() as u32,
    };

    Ok(DesignHealthResult {
        summary: format!(
            "{} element(s): {} resolved, {} unmapped, {} orphaned, {} broken ({} broken binding(s)).",
            counts.elements,
            counts.resolved,
            counts.unmapped,
            counts.orphaned,
            counts.broken,
            counts.broken_bindings
        ),
        counts,
        elements: findings,
        not_checked:
            "These checks prove that an element is attached and that what it binds to exists. They \
             do not check whether a bound file is a correct implementation of the element's \
             responsibility."
                .to_string(),
    })
}

/// A binding target resolves to a file inside the project folder, or it does not.
fn resolve_file(project_folder: &Path, target: &str) -> Option<PathBuf> {
    if target.trim().is_empty() {
        return None;
    }
    let candidate = PathBuf::from(target.trim().replace('\\', "/"));
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        project_folder.join(candidate)
    };
    let normalized = candidate.canonicalize().ok()?;
    let root = project_folder.canonicalize().ok()?;
    // A binding that escapes the folder is not a path the scan may follow.
    normalized.starts_with(&root).then_some(normalized)
}

/// The display form of a resolved file: relative to the project folder, with forward slashes.
///
/// The resolved path is canonical, which on Windows carries a `\\?\` prefix the configured folder
/// does not, so the strip is attempted against both forms rather than assuming they match.
fn relative_display(project_folder: &Path, path: &Path) -> String {
    let canonical_root = project_folder.canonicalize().unwrap_or_else(|_| project_folder.to_path_buf());
    path.strip_prefix(&canonical_root)
        .or_else(|_| path.strip_prefix(project_folder))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A symbol binding resolves when its file exists and holds the name as a whole word.
///
/// Deliberately not language-aware: it looks for the name, not for a declaration in any particular
/// syntax, so no language can be misjudged. A binding that names its file (`path/to/file.rs::sym`)
/// is checked against that file; a bare name is checked against the element's resolved files.
fn symbol_resolves(
    project_folder: &Path,
    target: &str,
    file_targets: &[String],
    resolved_files: &[String],
) -> bool {
    let (file_hint, symbol) = split_symbol(target);
    let symbol = symbol.rsplit("::").next().unwrap_or(symbol.as_str());
    if symbol.is_empty() {
        return false;
    }

    let candidates: Vec<String> = match file_hint {
        Some(file) => vec![file.to_string()],
        None => {
            if file_targets.is_empty() {
                return false;
            }
            resolved_files.to_vec()
        }
    };

    candidates.iter().any(|file| {
        resolve_file(project_folder, file)
            .and_then(|path| fs::read_to_string(path).ok())
            .map(|contents| contains_word(&contents, symbol))
            .unwrap_or(false)
    })
}

/// A whole-word occurrence, skipping occurrences inside string literals and comments so a file that
/// legitimately holds a name as data is not mistaken for one that defines it.
fn contains_word(contents: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut search_from = 0;
    while let Some(found) = contents[search_from..].find(needle) {
        let start = search_from + found;
        let end = start + needle.len();
        let before_ok = contents[..start]
            .chars()
            .next_back()
            .map(|character| !character.is_alphanumeric() && character != '_')
            .unwrap_or(true);
        let after_ok = contents[end..]
            .chars()
            .next()
            .map(|character| !character.is_alphanumeric() && character != '_')
            .unwrap_or(true);
        if before_ok && after_ok && !inside_literal_or_comment(contents, start) {
            return true;
        }
        search_from = end;
        if search_from >= contents.len() {
            break;
        }
    }
    false
}

/// Whether the offset sits inside a string literal or a comment.
fn inside_literal_or_comment(contents: &str, offset: usize) -> bool {
    let mut quote: Option<char> = None;
    let mut line_comment = false;
    let mut block_depth = 0usize;
    let mut characters = contents[..offset.min(contents.len())].chars().peekable();

    while let Some(character) = characters.next() {
        if line_comment {
            if character == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_depth > 0 {
            if character == '*' && characters.peek() == Some(&'/') {
                characters.next();
                block_depth -= 1;
            } else if character == '/' && characters.peek() == Some(&'*') {
                characters.next();
                block_depth += 1;
            }
            continue;
        }
        if let Some(active) = quote {
            if character == '\\' {
                characters.next();
            } else if character == active {
                quote = None;
            }
            continue;
        }
        match character {
            '/' if characters.peek() == Some(&'/') => {
                characters.next();
                line_comment = true;
            }
            '/' if characters.peek() == Some(&'*') => {
                characters.next();
                block_depth += 1;
            }
            '"' | '\'' | '`' => quote = Some(character),
            _ => {}
        }
    }
    quote.is_some() || line_comment || block_depth > 0
}

/// Splits `path::symbol` into an optional file hint and the symbol name.
fn split_symbol(target: &str) -> (Option<&str>, String) {
    let target = target.trim();
    match target.rsplit_once("::") {
        Some((file, symbol)) if !symbol.trim().is_empty() && looks_like_path(file) => {
            (Some(file.trim()), symbol.trim().to_string())
        }
        _ => (None, target.to_string()),
    }
}

fn looks_like_path(value: &str) -> bool {
    let value = value.trim();
    if value.contains('/') || value.contains('\\') {
        return true;
    }
    value.rsplit_once('.').is_some_and(|(_, extension)| {
        !extension.is_empty()
            && extension.len() <= 4
            && extension.chars().all(|character| character.is_ascii_alphanumeric())
    })
}

/// Writes the scan result, replacing the previous verdict for this project.
fn record(db: &Connection, project_id: i64, result: &DesignHealthResult) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|err| err.to_string())?;
    tx.execute(
        "DELETE FROM design_binding_checks WHERE project_id = ?1",
        params![project_id],
    )
    .map_err(|err| err.to_string())?;
    tx.execute(
        "DELETE FROM design_element_checks WHERE project_id = ?1",
        params![project_id],
    )
    .map_err(|err| err.to_string())?;

    for finding in &result.elements {
        for binding in &finding.broken {
            tx.execute(
                "INSERT INTO design_binding_checks(project_id, design_external_id, target_type, target, state, detail)
                 VALUES(?1, ?2, ?3, ?4, 'broken', ?5)",
                params![
                    project_id,
                    binding.design_external_id,
                    binding.target_type,
                    binding.target,
                    binding.detail
                ],
            )
            .map_err(|err| err.to_string())?;
        }
        tx.execute(
            "INSERT INTO design_element_checks(project_id, design_external_id, state, detail, files, checked_at)
             VALUES(?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)",
            params![
                project_id,
                finding.design_external_id,
                finding.state.as_str(),
                finding.detail,
                serde_json::to_string(&finding.files).unwrap_or_else(|_| "[]".to_string())
            ],
        )
        .map_err(|err| err.to_string())?;
    }
    tx.commit().map_err(|err| err.to_string())
}

/// The recorded state of every element, without touching the filesystem.
///
/// Returns the state and the files the bindings resolved to, so a client can show what an element
/// owns without a scan.
pub fn recorded_states(
    db: &Connection,
    project_id: i64,
) -> Result<HashMap<String, (ElementHealth, Vec<String>)>, String> {
    let mut statement = db
        .prepare(
            "SELECT design_external_id, state, files
             FROM design_element_checks WHERE project_id=?1",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?;
    let mut states = HashMap::new();
    for row in rows {
        let (external_id, state, files) = row.map_err(|err| err.to_string())?;
        if let Some(state) = ElementHealth::parse(&state) {
            let files = serde_json::from_str::<Vec<String>>(&files).unwrap_or_default();
            states.insert(external_id, (state, files));
        }
    }
    Ok(states)
}

/// The recorded counts, without touching the filesystem.
pub fn recorded_counts(db: &Connection, project_id: i64) -> Result<HealthCounts, String> {
    let mut statement = db
        .prepare("SELECT state, COUNT(*) FROM design_element_checks WHERE project_id=?1 GROUP BY state")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|err| err.to_string())?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())?;

    let mut counts = HealthCounts::default();
    for (state, count) in rows {
        let count = count as u32;
        match state.as_str() {
            "orphaned" => counts.orphaned = count,
            "unmapped" => counts.unmapped = count,
            "broken" => counts.broken = count,
            "resolved" => counts.resolved = count,
            _ => {}
        }
        counts.elements += count;
    }
    counts.broken_bindings = db
        .query_row(
            "SELECT COUNT(*) FROM design_binding_checks WHERE project_id=?1 AND state='broken'",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())? as u32;
    counts.waivers = db
        .query_row(
            "SELECT COUNT(*) FROM design_health_waivers WHERE project_id=?1",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())? as u32;
    Ok(counts)
}

/// One line describing the recorded state, so a reader does not have to interpret numbers.
pub fn recorded_summary(counts: &HealthCounts) -> String {
    if counts.elements == 0 {
        return "Design health has not been scanned for this project yet.".to_string();
    }
    format!(
        "{} resolved, {} unmapped, {} orphaned, {} broken ({} broken binding(s)).",
        counts.resolved, counts.unmapped, counts.orphaned, counts.broken, counts.broken_bindings
    )
}

/// The recorded state of one element, without a filesystem scan.
#[allow(dead_code)]
pub fn recorded_state(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<Option<ElementHealth>, String> {
    let state: Option<String> = db
        .query_row(
            "SELECT state FROM design_element_checks WHERE project_id=?1 AND design_external_id=?2",
            params![project_id, external_id],
            |row| row.get(0),
        )
        .ok();
    Ok(state.and_then(|state| ElementHealth::parse(&state)))
}

/// The recorded state of one element, with the files its bindings resolved to.
#[allow(dead_code)]
pub fn recorded_state_with_files(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<Option<(ElementHealth, Vec<String>)>, String> {
    Ok(recorded_states(db, project_id)?.remove(external_id))
}

/// The findings that were reviewed and knowingly kept, newest first.
#[allow(dead_code)]
pub fn load_waivers(
    db: &Connection,
    project_id: i64,
    external_id: &str,
) -> Result<Vec<HealthWaiver>, String> {
    let mut statement = db
        .prepare(
            "SELECT id, state, reason, task_id, created_by, created_at
             FROM design_health_waivers
             WHERE project_id=?1 AND design_external_id=?2
             ORDER BY id DESC",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(params![project_id, external_id], |row| {
            Ok(HealthWaiver {
                id: row.get(0)?,
                state: row.get(1)?,
                reason: row.get(2)?,
                task_id: row.get(3)?,
                created_by: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|err| err.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|err| err.to_string())
}

/// Records a finding that was reviewed and knowingly kept. The reason is required: keeping a
/// finding without one is a silence, and silence is what made the drift invisible.
#[allow(dead_code)]
pub fn record_waiver(
    db: &Connection,
    project_id: i64,
    external_id: &str,
    state: ElementHealth,
    reason: &str,
    task_id: Option<i64>,
) -> Result<i64, String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("A kept finding needs a reason. Without one it is a silence.".to_string());
    }
    db.execute(
        "INSERT INTO design_health_waivers(project_id, design_external_id, state, reason, task_id)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![project_id, external_id, state.as_str(), reason, task_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(db.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-health-{label}-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn model(
        elements: &[(&str, &str, &str, Option<&str>)],
        bindings: &[(&str, &str, &str)],
        relationships: &[(&str, &str)],
    ) -> Model {
        let mut connected = BTreeSet::new();
        for (source, destination) in relationships {
            connected.insert((*source).to_string());
            connected.insert((*destination).to_string());
        }
        // Mirror what collect_model does: holding children attaches an element.
        for (_, _, _, parent) in elements {
            if let Some(parent) = parent {
                connected.insert((*parent).to_string());
            }
        }
        Model {
            elements: elements
                .iter()
                .map(|(id, name, kind, parent)| {
                    (
                        (*id).to_string(),
                        (*name).to_string(),
                        (*kind).to_string(),
                        parent.map(str::to_string),
                    )
                })
                .collect(),
            bindings: bindings
                .iter()
                .map(|(id, kind, target)| {
                    ((*id).to_string(), (*kind).to_string(), (*target).to_string())
                })
                .collect(),
            connected,
        }
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn finding<'a>(result: &'a DesignHealthResult, id: &str) -> &'a ElementFinding {
        result
            .elements
            .iter()
            .find(|element| element.design_external_id == id)
            .unwrap_or_else(|| panic!("{id} missing"))
    }

    #[test]
    fn an_element_with_no_parent_and_no_relationship_is_orphaned() {
        let root = scratch("orphaned");
        write(&root, "src/a.rs", "fn thing() {}\n");
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", None)],
                &[("a", "file", "src/a.rs")],
                &[],
            ),
        )
        .unwrap();
        let element = finding(&result, "a");
        assert_eq!(element.state, ElementHealth::Orphaned);
        // Its binding still resolves; the problem is the model, not the code.
        assert_eq!(element.files, vec!["src/a.rs".to_string()]);
        assert!(element.next_action.contains("Attach it to the model"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_parent_or_a_relationship_is_enough_to_be_attached() {
        let root = scratch("attached");
        write(&root, "src/a.rs", "fn thing() {}\n");
        write(&root, "src/b.rs", "fn other() {}\n");
        write(&root, "src/root.rs", "fn root() {}\n");
        let result = scan(
            &root,
            model(
                &[
                    ("root", "Root", "Software System", None),
                    ("child", "Child", "Container", Some("root")),
                    ("peer", "Peer", "Component", None),
                ],
                &[
                    ("root", "file", "src/root.rs"),
                    ("child", "file", "src/a.rs"),
                    ("peer", "file", "src/b.rs"),
                ],
                &[("root", "peer")],
            ),
        )
        .unwrap();
        for id in ["root", "child", "peer"] {
            assert_eq!(finding(&result, id).state, ElementHealth::Resolved, "{id}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_attached_element_without_bindings_is_unmapped_not_unimplemented() {
        let root = scratch("unmapped");
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[],
                &[],
            ),
        )
        .unwrap();
        let element = finding(&result, "a");
        assert_eq!(element.state, ElementHealth::Unmapped);
        assert!(
            element.detail.contains("nothing is claimed about where its code lives"),
            "{}",
            element.detail
        );
        assert!(
            !element.detail.contains("not implemented"),
            "the state must not read as absent work"
        );
        assert!(element.next_action.contains("Bind it to the file"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_binding_that_names_a_missing_file_is_broken() {
        let root = scratch("broken");
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "src/gone.rs")],
                &[],
            ),
        )
        .unwrap();
        let element = finding(&result, "a");
        assert_eq!(element.state, ElementHealth::Broken);
        assert_eq!(element.broken.len(), 1);
        assert!(element.broken[0].detail.contains("file not found"));
        assert_eq!(result.counts.broken, 1);
        assert_eq!(result.counts.broken_bindings, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn one_broken_binding_among_good_ones_is_still_broken() {
        let root = scratch("partly-broken");
        write(&root, "src/a.rs", "fn thing() {}\n");
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "src/a.rs"), ("a", "file", "src/gone.rs")],
                &[],
            ),
        )
        .unwrap();
        let element = finding(&result, "a");
        assert_eq!(element.state, ElementHealth::Broken);
        assert_eq!(element.files, vec!["src/a.rs".to_string()]);
        assert_eq!(element.broken.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_symbol_binding_resolves_against_its_named_file() {
        let root = scratch("symbol");
        write(&root, "src/a.rs", "pub struct Thing;\n");
        let resolved = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "src/a.rs"), ("a", "symbol", "src/a.rs::Thing")],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(finding(&resolved, "a").state, ElementHealth::Resolved);

        let absent = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "src/a.rs"), ("a", "symbol", "src/a.rs::Absent")],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(finding(&absent, "a").state, ElementHealth::Broken);
        assert!(finding(&absent, "a").broken[0]
            .detail
            .contains("symbol not found"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_bare_symbol_name_is_checked_against_the_elements_files() {
        let root = scratch("bare-symbol");
        write(&root, "src/a.rs", "fn helper() {}\n");
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "src/a.rs"), ("a", "symbol", "helper")],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(finding(&result, "a").state, ElementHealth::Resolved);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_symbol_named_only_as_data_is_not_a_definition() {
        // The prompt-repair table holds retired names as string literals. A file that lists a name
        // is not a file that defines it.
        let root = scratch("symbol-data");
        write(
            &root,
            "src/a.rs",
            "const RETIRED: &[&str] = &[\"Absent\"];\n// Absent in a comment too\n",
        );
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "src/a.rs"), ("a", "symbol", "src/a.rs::Absent")],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(finding(&result, "a").state, ElementHealth::Broken);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_binding_cannot_escape_the_project_folder() {
        let root = scratch("escape");
        let outside = root.parent().unwrap().join("outside-health-secret.rs");
        fs::write(&outside, "fn secret() {}\n").unwrap();
        let result = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[("a", "file", "../outside-health-secret.rs")],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(finding(&result, "a").state, ElementHealth::Broken);
        let _ = fs::remove_file(outside);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn findings_are_ordered_most_actionable_first() {
        let root = scratch("ordering");
        write(&root, "src/ok.rs", "fn ok() {}\n");
        let result = scan(
            &root,
            model(
                &[
                    ("root", "Root", "Software System", None),
                    ("ok", "Ok", "Component", Some("root")),
                    ("unmapped", "Unmapped", "Component", Some("root")),
                    ("dead", "Dead", "Component", Some("root")),
                    ("float", "Float", "Component", None),
                ],
                &[("ok", "file", "src/ok.rs"), ("dead", "file", "src/nope.rs")],
                &[],
            ),
        )
        .unwrap();
        let order = result
            .elements
            .iter()
            .map(|element| (element.design_external_id.as_str(), element.state))
            .collect::<Vec<_>>();
        // `root` holds children, so it is attached and merely unmapped; `float` holds nothing
        // and is named by nothing, so it is the orphaned one. Order within a state is by id.
        assert_eq!(
            order,
            vec![
                ("dead", ElementHealth::Broken),
                ("float", ElementHealth::Orphaned),
                ("root", ElementHealth::Unmapped),
                ("unmapped", ElementHealth::Unmapped),
                ("ok", ElementHealth::Resolved),
            ]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_result_states_what_it_does_not_check() {
        let root = scratch("limits");
        let result = scan(&root, model(&[], &[], &[])).unwrap();
        assert!(result.not_checked.contains("do not check"));
        assert!(result.summary.contains("0 element(s)"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_recorded_scan_is_readable_without_touching_the_filesystem() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
            .unwrap();
        let root = scratch("recorded");
        write(&root, "src/ok.rs", "fn ok() {}\n");
        let result = scan(
            &root,
            model(
                &[
                    ("root", "Root", "Software System", Some("parent")),
                    ("ok", "Ok", "Component", Some("root")),
                    ("dead", "Dead", "Component", Some("root")),
                    ("bare", "Bare", "Component", Some("root")),
                    ("float", "Float", "Component", None),
                ],
                &[("ok", "file", "src/ok.rs"), ("dead", "file", "gone.rs")],
                &[],
            ),
        )
        .unwrap();
        record(&db, 1, &result).unwrap();

        let states = recorded_states(&db, 1).unwrap();
        let state_of = |id: &str| states.get(id).map(|(state, _)| *state);
        assert_eq!(state_of("ok"), Some(ElementHealth::Resolved));
        assert_eq!(state_of("dead"), Some(ElementHealth::Broken));
        assert_eq!(state_of("bare"), Some(ElementHealth::Unmapped));
        assert_eq!(state_of("float"), Some(ElementHealth::Orphaned));

        // The resolved files survive the round trip. A resolved element with no visible files is
        // indistinguishable from a broken one in the design view, so this is not cosmetic.
        assert_eq!(
            states.get("ok").map(|(_, files)| files.clone()),
            Some(vec!["src/ok.rs".to_string()])
        );
        assert_eq!(
            states.get("dead").map(|(_, files)| files.clone()),
            Some(Vec::new()),
            "a broken element must not claim files it could not resolve"
        );

        // Every one of the four counts has to survive the round trip. Asserting only some of them
        // is what let a dashboard read three zeros next to a correct total, so all four are pinned.
        // `root` is unmapped too: it names a parent that is not in this fixture, and having a
        // parent is what attaches it.
        let counts = recorded_counts(&db, 1).unwrap();
        assert_eq!(
            counts,
            HealthCounts {
                orphaned: 1,
                unmapped: 2,
                broken: 1,
                resolved: 1,
                broken_bindings: 1,
                waivers: 0,
                elements: 5,
            },
            "the recorded counts must agree with the scan state for state"
        );
        assert_eq!(counts.needs_attention(), 4);
        assert_eq!(
            recorded_summary(&counts),
            "1 resolved, 2 unmapped, 1 orphaned, 1 broken (1 broken binding(s))."
        );

        // A second scan replaces the first rather than accumulating.
        record(&db, 1, &result).unwrap();
        let again = recorded_counts(&db, 1).unwrap();
        assert_eq!(again, counts);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_kept_finding_requires_a_reason() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut db).unwrap();
        db.execute("INSERT INTO projects(name,slug) VALUES('P','p')", [])
            .unwrap();
        let error = record_waiver(&db, 1, "a", ElementHealth::Unmapped, "   ", None).unwrap_err();
        assert!(error.contains("needs a reason"), "{error}");
        record_waiver(
            &db,
            1,
            "a",
            ElementHealth::Unmapped,
            "specified but deliberately unbound for now",
            None,
        )
        .unwrap();
        let waivers = load_waivers(&db, 1, "a").unwrap();
        assert_eq!(waivers.len(), 1);
        assert_eq!(waivers[0].state, "unmapped");
        assert_eq!(recorded_counts(&db, 1).unwrap().waivers, 1);
    }

    /// Refreshes this workspace's real database in place, so the design view shows the file lists
    /// without waiting for a manual rescan.
    ///
    /// Run explicitly:
    /// `cargo test --lib --no-default-features design_health::tests::refresh_live -- --ignored --nocapture`
    #[test]
    #[ignore = "writes the recorded check for this workspace's real project database"]
    fn refresh_live_project_health() {
        // Read the settings text rather than going through load_or_init, which normalises and
        // writes the user's settings file back. This uses the folder out of it and writes only
        // inside the workspace.
        let settings_path = crate::settings::settings_path();
        let settings: crate::settings::AppSettings =
            serde_json::from_str(&fs::read_to_string(&settings_path).expect("settings readable"))
                .expect("settings parse");
        let project = crate::project::resolve_project_from_settings(&settings, Some("adashi"))
            .expect("the workspace project must be configured");
        let mut db = crate::project::open_project_database(&project).unwrap();
        // Run the real migrations, which is also what adds the files column to a database that
        // predates it.
        crate::schema::migrate(&mut db).unwrap();
        let project_id: i64 = db
            .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
            .unwrap();

        let result =
            scan_and_record(&db, project_id, std::path::Path::new(&project.folder)).unwrap();
        println!("{}", result.summary);
        let recorded = recorded_states(&db, project_id).unwrap();
        println!(
            "recorded elements with files: {}",
            recorded.values().filter(|(_, files)| !files.is_empty()).count()
        );
        for (external_id, (_, files)) in recorded.iter().filter(|(_, (_, files))| !files.is_empty()).take(5) {
            println!("  {external_id} owns {files:?}");
        }
    }

    /// Runs the scan against this workspace's real design and real source tree, on a copy of the
    /// live database, and prints what it finds.
    ///
    /// Run with:
    /// `cargo test --lib --no-default-features design_health::tests::real_project -- --ignored --nocapture`
    #[test]
    #[ignore = "manual verification against this workspace's real database"]
    fn real_project_scan_reports_attach_and_link_findings() {
        let live_database = Path::new(r"C:\src\Adashi\.adashi\adashi.sqlite3");
        let live_folder = Path::new(r"C:\src\Adashi");
        if !live_database.exists() {
            eprintln!("skipping: no live database at {}", live_database.display());
            return;
        }
        let root = scratch("real");
        let copy = root.join("adashi.sqlite3");
        fs::copy(live_database, &copy).unwrap();
        let mut db = Connection::open(&copy).unwrap();
        crate::schema::migrate(&mut db).unwrap();
        let project_id: i64 = db
            .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
            .unwrap();

        let result = scan_and_record(&db, project_id, live_folder).unwrap();
        println!("{}", result.summary);
        println!("not checked: {}", result.not_checked);
        println!();

        let resolved: Vec<&ElementFinding> = result
            .elements
            .iter()
            .filter(|element| element.state == ElementHealth::Resolved)
            .collect();
        let resolved_with_files = resolved
            .iter()
            .filter(|element| !element.files.is_empty())
            .count();
        println!("resolved elements: {} ({resolved_with_files} with files)", resolved.len());
        for element in resolved.iter().take(5) {
            println!("  {} owns {:?}", element.design_external_id, element.files);
        }
        // The recorded files are what the design view shows, so an empty file list means an
        // element appears to own nothing even though its bindings resolved.
        let recorded = recorded_states(&db, project_id).unwrap();
        let recorded_with_files = recorded.values().filter(|(_, files)| !files.is_empty()).count();
        // A broken element can still own files: it has some bindings that resolve and some that do
        // not. So the recorded count is compared against every element that owns something, not
        // against the resolved ones alone.
        let scanned_with_files = result
            .elements
            .iter()
            .filter(|element| !element.files.is_empty())
            .count();
        println!("recorded elements with files: {recorded_with_files}");
        assert!(
            resolved_with_files > 0,
            "a resolved element must own at least one file, or the view has nothing to show"
        );
        assert_eq!(
            recorded_with_files, scanned_with_files,
            "every scanned file list must survive into the recorded rows the view reads"
        );
        for element in result
            .elements
            .iter()
            .filter(|element| element.state != ElementHealth::Resolved)
        {
            println!(
                "{:8} {} ({}) — {}",
                element.state.as_str(),
                element.design_external_id,
                element.name,
                element.detail
            );
            for binding in &element.broken {
                println!("         broken: {} {}", binding.target_type, binding.target);
            }
        }
        println!();
        println!("resolved: {}", result.counts.resolved);

        // The scan is reproducible: identical inputs produce identical verdicts.
        let again = scan_and_record(&db, project_id, live_folder).unwrap();
        assert_eq!(again.counts, result.counts);
        let counts = recorded_counts(&db, project_id).unwrap();
        assert_eq!(counts, result.counts);
        println!("recorded: {}", recorded_summary(&counts));

        let _ = fs::remove_dir_all(root);
    }
}
