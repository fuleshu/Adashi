//! Generated in-tree architecture projection.
//!
//! The design database is canonical. This module renders bounded, deterministic instruction-file
//! blocks from it, so the model is visible where an agent already reads: the project root and the
//! folders that contain bound files. Nothing here is a source of truth — every block is a
//! projection, marked as generated and overwritten on regeneration.

use crate::state;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Managed-block markers. Text outside them is never rewritten.
pub const BLOCK_BEGIN: &str = "<!-- adashi:architecture:begin -->";
pub const BLOCK_END: &str = "<!-- adashi:architecture:end -->";
/// Carries the design revision a block was rendered from, for freshness and drift checks.
const REVISION_PREFIX: &str = "<!-- adashi:generated revision=";

/// Byte budgets. Nested instruction files compete for a harness budget that deletes on overflow,
/// so over-budget content is dropped deterministically and reported, never silently clipped.
const ROOT_BUDGET: usize = 2_000;
const FOLDER_BUDGET: usize = 1_000;
/// Caps that keep generated files from growing without bound in a large repository.
const MAX_FOLDERS: usize = 200;
const MAX_WALK_DIRECTORIES: usize = 5_000;
const ROOT_DESCRIPTION_CHARS: usize = 150;
/// Body budget held back for the Boundaries section, so a root block cannot spend everything on
/// responsibilities and show no architecture shape at all.
const ROOT_BOUNDARIES_RESERVE: usize = 220;
/// Folder blocks trade description depth for coverage: naming every element bound to a folder
/// is what stops duplication, so more short lines beat fewer long ones.
const FOLDER_DESCRIPTION_CHARS: usize = 90;

/// Directories that never receive a projection block. `public` is a served static-asset
/// directory, so a generated instruction file there would ship in the bundle instead of
/// guiding anyone.
const EXCLUDED_DIRECTORIES: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    "public",
    ".git",
    ".adashi",
];

/// Worst-case bytes reserved for the truncation notice, so a block that truncates still fits
/// the documented cap.
const ROOT_NOTICE_RESERVE: usize = 160;
/// A folder block can carry one merged truncation notice.
const FOLDER_NOTICE_RESERVE: usize = 170;
/// Room kept for the compact line that names elements whose descriptions did not fit.
const COMPACT_NAMES_RESERVE: usize = 150;
const COMPACT_NAMES_CHARS: usize = 110;
/// Placeholder sized like the real summary line, so budgeting accounts for it before the
/// accurate counts are known.
const ROOT_SUMMARY_PLACEHOLDER: &str =
    "Top layer: 00 of 000 elements, 00 of 0000 relationships. Deeper detail: the adashi_design \
     get_scope and get_bindings operations.";
const ROOT_SUMMARY_INDEX: usize = 4;

/// What one regeneration changed on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionReport {
    /// Project-relative paths written or refreshed.
    pub written: Vec<String>,
    /// Paths whose existing block had been edited outside Adashi and was restored.
    pub repaired: Vec<String>,
    /// Paths whose block was dropped because they no longer carry bound design.
    pub removed: Vec<String>,
}

/// Freshness state of one projected file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionFileStatus {
    /// Project-relative path.
    pub path: String,
    /// `current`, `stale`, `drifted` or `missing`.
    pub state: String,
}

/// Read-only projection status for the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionStatus {
    pub enabled: bool,
    pub file_name: String,
    pub revision: i64,
    /// Set when the last regeneration attempt failed, so a failure is visible rather than
    /// silently leaving stale blocks in place.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub files: Vec<ProjectionFileStatus>,
}

#[derive(Debug, Clone)]
struct Element {
    external_id: String,
    element_type: String,
    name: String,
    description: String,
    parent_external_id: Option<String>,
}

#[derive(Debug, Clone)]
struct Relationship {
    external_id: String,
    source: String,
    destination: String,
    description: String,
}

#[derive(Debug, Clone)]
struct Binding {
    design_external_id: String,
    target_type: String,
    target: String,
}

#[derive(Debug, Default)]
struct DesignSnapshot {
    elements: Vec<Element>,
    relationships: Vec<Relationship>,
    bindings: Vec<Binding>,
}

impl DesignSnapshot {
    fn element(&self, external_id: &str) -> Option<&Element> {
        self.elements
            .iter()
            .find(|element| element.external_id == external_id)
    }

    /// External ids of the top layer: the software systems and the containers they contain.
    fn top_layer_ids(&self) -> BTreeSet<String> {
        let system_ids = self
            .elements
            .iter()
            .filter(|element| element.element_type == "Software System")
            .map(|element| element.external_id.clone())
            .collect::<BTreeSet<_>>();

        self.elements
            .iter()
            .filter(|element| {
                element.element_type == "Software System"
                    || element
                        .parent_external_id
                        .as_deref()
                        .is_some_and(|parent| system_ids.contains(parent))
            })
            .map(|element| element.external_id.clone())
            .collect()
    }
}

fn load_snapshot(db: &Connection, project_row_id: i64) -> Result<DesignSnapshot, String> {
    let mut snapshot = DesignSnapshot::default();

    let mut statement = db
        .prepare(
            "SELECT e.external_id, e.element_type, e.name, e.description, e.parent_external_id
             FROM c4_elements e
             JOIN design_workspaces w ON w.id = e.workspace_id
             WHERE w.project_id = ?1
             ORDER BY e.id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([project_row_id], |row| {
            Ok(Element {
                external_id: row.get(0)?,
                element_type: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                parent_external_id: row.get(4)?,
            })
        })
        .map_err(|error| error.to_string())?;
    snapshot.elements = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;

    let mut statement = db
        .prepare(
            "SELECT r.external_id, r.source_external_id, r.destination_external_id, r.description
             FROM c4_relationships r
             JOIN design_workspaces w ON w.id = r.workspace_id
             WHERE w.project_id = ?1
             ORDER BY r.id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([project_row_id], |row| {
            Ok(Relationship {
                external_id: row.get(0)?,
                source: row.get(1)?,
                destination: row.get(2)?,
                description: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?;
    snapshot.relationships = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;

    let mut statement = db
        .prepare(
            "SELECT b.design_external_id, b.target_type, b.target
             FROM design_bindings b
             JOIN design_workspaces w ON w.id = b.workspace_id
             WHERE w.project_id = ?1
             ORDER BY b.design_external_id, b.target_type, b.target",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([project_row_id], |row| {
            Ok(Binding {
                design_external_id: row.get(0)?,
                target_type: row.get(1)?,
                target: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;
    snapshot.bindings = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;

    Ok(snapshot)
}

/// Orders elements most specific first, so a folder block leads with the components that own
/// its code rather than with a broad container that happens to be bound there too.
fn specificity_rank(element_type: &str) -> i32 {
    match element_type {
        "Component" => 0,
        "Container" => 1,
        "Software System" => 2,
        _ => 3,
    }
}

/// Collapses whitespace and clamps to `max_chars`, so one description cannot dominate a block.
fn one_line(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let truncated = collapsed.chars().take(max_chars).collect::<String>();
    format!("{}…", truncated.trim_end())
}

/// Bytes a vector of lines occupies when joined with newlines.
fn body_bytes(lines: &[String]) -> usize {
    lines.iter().map(|line| line.len() + 1).sum()
}

/// Trims `lines` to `budget` bytes by dropping from the end, and reports how many were dropped.
fn fit_budget(lines: &mut Vec<String>, budget: usize) -> usize {
    let mut dropped = 0;
    while !lines.is_empty() {
        if body_bytes(lines) <= budget {
            break;
        }
        lines.pop();
        dropped += 1;
    }
    dropped
}

/// Appends a titled section when at least one of its lines fits, and reports dropped lines.
///
/// A section whose title would stand without content is never emitted, so truncation cannot
/// leave a dangling heading.
fn push_section(body: &mut Vec<String>, title: &str, lines: &[String], budget: usize) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let used = body_bytes(body) + title.len() + 2;
    if used >= budget {
        return lines.len();
    }
    let mut kept = lines.to_vec();
    let dropped = fit_budget(&mut kept, budget - used);
    if kept.is_empty() {
        return lines.len();
    }
    body.push(String::new());
    body.push(title.to_string());
    body.extend(kept);
    dropped
}

fn revision_marker(revision: i64) -> String {
    format!("{REVISION_PREFIX}{revision} -->")
}

fn generated_notice() -> &'static str {
    "Generated from the Adashi design model; do not edit, change the model."
}

fn render_root_block(snapshot: &DesignSnapshot, revision: i64) -> String {
    let top_layer = snapshot.top_layer_ids();
    let mut element_lines: Vec<String> = Vec::new();

    for element in &snapshot.elements {
        if element.element_type != "Software System" {
            continue;
        }
        let purpose = one_line(&element.description, ROOT_DESCRIPTION_CHARS);
        element_lines.push(format!("- **{}** (Software System) — {purpose}", element.name));
    }
    for element in &snapshot.elements {
        if element.element_type == "Software System" || !top_layer.contains(&element.external_id) {
            continue;
        }
        let responsibility = one_line(&element.description, ROOT_DESCRIPTION_CHARS);
        element_lines.push(format!(
            "- **{}** ({}) — {responsibility}",
            element.name, element.element_type
        ));
    }

    let mut relationship_lines = Vec::new();
    for relationship in &snapshot.relationships {
        if !top_layer.contains(&relationship.source)
            || !top_layer.contains(&relationship.destination)
        {
            continue;
        }
        let (Some(source), Some(destination)) = (
            snapshot.element(&relationship.source),
            snapshot.element(&relationship.destination),
        ) else {
            continue;
        };
        relationship_lines.push(format!(
            "- {} -> {}: {}",
            source.name,
            destination.name,
            one_line(&relationship.description, ROOT_DESCRIPTION_CHARS)
        ));
    }

    let header = vec![
        BLOCK_BEGIN.to_string(),
        revision_marker(revision),
        "# Architecture (generated)".to_string(),
        generated_notice().to_string(),
        ROOT_SUMMARY_PLACEHOLDER.to_string(),
        String::new(),
        "These responsibilities are already owned: extend them, do not duplicate.".to_string(),
        String::new(),
    ];
    let footer = vec![BLOCK_END.to_string()];

    // Budget covers the whole block, so the documented cap is the file size a harness sees.
    let overhead = body_bytes(&header) + body_bytes(&footer) + ROOT_NOTICE_RESERVE;
    let budget = ROOT_BUDGET.saturating_sub(overhead);

    // The body is fitted first so the summary line can state what actually survived. Elements
    // are fitted against a reduced budget whenever boundaries exist, so responsibilities cannot
    // consume the whole block and leave the architecture's shape unstated.
    let element_budget = if relationship_lines.is_empty() {
        budget
    } else {
        budget.saturating_sub(ROOT_BOUNDARIES_RESERVE)
    };
    let mut body = element_lines;
    let dropped_elements = fit_budget(&mut body, element_budget);
    let dropped_relationships =
        push_section(&mut body, "Boundaries:", &relationship_lines, budget);

    let mut block = header;
    block[ROOT_SUMMARY_INDEX] = format!(
        "Top layer: {top_layer_count} of {total_elements} elements, {} of {total_relationships} \
         relationships. Deeper detail: the adashi_design get_scope and get_bindings operations.",
        relationship_lines.len() - dropped_relationships,
        total_elements = snapshot.elements.len(),
        total_relationships = snapshot.relationships.len(),
        top_layer_count = top_layer.len(),
    );
    block.extend(body);
    if dropped_elements > 0 || dropped_relationships > 0 {
        block.push(String::new());
        block.push(format!(
            "[Dropped {dropped_elements} element line(s) and {dropped_relationships} relationship \
             line(s) to fit the projection budget; retrieve them by id.]"
        ));
    }
    block.extend(footer);
    block.join("\n")
}

/// Renders the per-folder block for one folder, or `None` when the folder owns no design.
fn render_folder_block(
    folder: &str,
    owned: &[&Element],
    relationships: &[&Relationship],
    snapshot: &DesignSnapshot,
    bindings: &[&Binding],
    revision: i64,
) -> Option<String> {
    if owned.is_empty() {
        return None;
    }

    // Kept as (name, line) pairs so elements that do not fit can still be named compactly:
    // knowing which elements own a folder is what stops duplication.
    let element_entries = owned
        .iter()
        .map(|element| {
            (
                element.name.clone(),
                format!(
                    "- **{}** ({}) — {}",
                    element.name,
                    element.element_type,
                    one_line(&element.description, FOLDER_DESCRIPTION_CHARS)
                ),
            )
        })
        .collect::<Vec<_>>();

    let mut relationship_lines = Vec::new();
    for relationship in relationships {
        let (Some(source), Some(destination)) = (
            snapshot.element(&relationship.source),
            snapshot.element(&relationship.destination),
        ) else {
            continue;
        };
        relationship_lines.push(format!(
            "- {} -> {}: {}",
            source.name,
            destination.name,
            one_line(&relationship.description, FOLDER_DESCRIPTION_CHARS)
        ));
    }

    // One line per distinct bound target: several elements in a folder can bind the same file.
    let binding_lines = bindings
        .iter()
        .map(|binding| format!("- {} `{}`", binding.target_type, binding.target))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let header = vec![
        BLOCK_BEGIN.to_string(),
        revision_marker(revision),
        format!("# Architecture — `{folder}` (generated)"),
        generated_notice().to_string(),
        "These responsibilities are already owned here: extend them, do not duplicate.".to_string(),
        String::new(),
    ];
    let footer = vec![BLOCK_END.to_string()];
    let overhead = body_bytes(&header) + body_bytes(&footer) + FOLDER_NOTICE_RESERVE;
    let budget = FOLDER_BUDGET.saturating_sub(overhead);

    let mut kept_elements = element_entries
        .iter()
        .map(|(_, line)| line.clone())
        .collect::<Vec<_>>();
    if fit_budget(&mut kept_elements, budget) > 0 {
        // Elements were dropped, so reserve room to name them: coverage of which elements own
        // this folder is what stops duplication, even when their descriptions must be cut.
        let mut refit = element_entries
            .iter()
            .map(|(_, line)| line.clone())
            .collect::<Vec<_>>();
        fit_budget(&mut refit, budget.saturating_sub(COMPACT_NAMES_RESERVE));
        kept_elements = refit;
    }
    let dropped_names = element_entries[kept_elements.len()..]
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let dropped_elements = dropped_names.len();

    let mut body = kept_elements;
    if !dropped_names.is_empty() {
        let mut compact = String::new();
        let mut included = 0;
        for name in &dropped_names {
            let candidate = if compact.is_empty() {
                name.clone()
            } else {
                format!("{compact}, {name}")
            };
            if candidate.len() > COMPACT_NAMES_CHARS {
                break;
            }
            compact = candidate;
            included += 1;
        }
        if included < dropped_names.len() {
            compact.push_str(", …");
        } else {
            compact.push('.');
        }
        push_section(&mut body, "Also bound here:", &[compact], budget);
    }
    let dropped_relationships = push_section(
        &mut body,
        "Boundaries crossing this folder:",
        &relationship_lines,
        budget,
    );
    let dropped_bindings = push_section(&mut body, "Bound here:", &binding_lines, budget);
    let dropped_other = dropped_relationships + dropped_bindings;

    let mut block = header;
    block.extend(body);
    if dropped_elements > 0 || dropped_other > 0 {
        block.push(String::new());
        block.push(format!(
            "[Showing {} of {} design element(s) bound here, {dropped_other} further line(s) \
             dropped. Retrieve the rest with the adashi_design get_scope operation.]",
            owned.len() - dropped_elements,
            owned.len()
        ));
    }
    block.extend(footer);
    Some(block.join("\n"))
}

/// Normalises a bound path to a project-relative path, or `None` when it escapes the project.
fn relative_target(project_folder: &Path, target: &str) -> Option<String> {
    let trimmed = target.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return None;
    }

    let relative = if Path::new(&trimmed).is_absolute() {
        let absolute = Path::new(target).to_path_buf();
        absolute
            .strip_prefix(project_folder)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        trimmed.trim_start_matches("./").to_string()
    };

    if relative.is_empty()
        || relative.starts_with('/')
        || Path::new(&relative)
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return None;
    }
    Some(relative)
}

fn folder_of(relative_path: &str) -> String {
    match relative_path.rsplit_once('/') {
        Some((folder, _)) if !folder.is_empty() => folder.to_string(),
        _ => String::new(),
    }
}

fn directory_is_excluded(folder: &str) -> bool {
    folder
        .split('/')
        .any(|part| part.starts_with('.') || EXCLUDED_DIRECTORIES.contains(&part))
}

/// Computes the exact block content for every projected path, including the root.
fn expected_blocks(
    snapshot: &DesignSnapshot,
    project_folder: &Path,
    revision: i64,
) -> BTreeMap<String, String> {
    let mut blocks = BTreeMap::new();
    blocks.insert(String::new(), render_root_block(snapshot, revision));

    let mut by_folder: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, binding) in snapshot.bindings.iter().enumerate() {
        if binding.target_type != "file" {
            continue;
        }
        let Some(relative) = relative_target(project_folder, &binding.target) else {
            continue;
        };
        let folder = folder_of(&relative);
        if directory_is_excluded(&folder) {
            continue;
        }
        by_folder.entry(folder).or_default().push(index);
    }

    for (folder, binding_indexes) in by_folder {
        if blocks.len() > MAX_FOLDERS {
            break;
        }
        let owned_ids = binding_indexes
            .iter()
            .map(|index| snapshot.bindings[*index].design_external_id.clone())
            .collect::<BTreeSet<_>>();
        let mut owned = owned_ids
            .iter()
            .filter_map(|id| snapshot.element(id))
            .collect::<Vec<_>>();
        owned.sort_by(|left, right| {
            specificity_rank(&left.element_type)
                .cmp(&specificity_rank(&right.element_type))
                .then_with(|| left.external_id.cmp(&right.external_id))
        });
        if owned.is_empty() {
            continue;
        }

        let mut relationships = snapshot
            .relationships
            .iter()
            .filter(|relationship| {
                owned_ids.contains(&relationship.source)
                    || owned_ids.contains(&relationship.destination)
            })
            .collect::<Vec<_>>();
        relationships.sort_by(|left, right| left.external_id.cmp(&right.external_id));

        let mut bindings = binding_indexes
            .iter()
            .map(|index| &snapshot.bindings[*index])
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| left.target.cmp(&right.target));

        if let Some(block) =
            render_folder_block(&folder, &owned, &relationships, snapshot, &bindings, revision)
        {
            blocks.insert(folder, block);
        }
    }

    blocks
}

/// Extracts the managed block from file content, if present.
fn extract_block(content: &str) -> Option<(usize, usize)> {
    let start = content.find(BLOCK_BEGIN)?;
    let end = content[start..].find(BLOCK_END)? + start + BLOCK_END.len();
    Some((start, end))
}

fn block_revision(content: &str) -> Option<i64> {
    let (start, end) = extract_block(content)?;
    let block = &content[start..end];
    let marker = block.find(REVISION_PREFIX)? + REVISION_PREFIX.len();
    let rest = &block[marker..];
    let digits = rest
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

/// Splices `block` into existing content, replacing an existing managed block when present.
fn splice_block(existing: &str, block: &str) -> String {
    match extract_block(existing) {
        Some((start, end)) => {
            let mut spliced = String::with_capacity(existing.len() + block.len());
            spliced.push_str(&existing[..start]);
            spliced.push_str(block);
            spliced.push_str(&existing[end..]);
            spliced
        }
        None => {
            if existing.trim().is_empty() {
                return format!("{block}\n");
            }
            let mut appended = existing.trim_end().to_string();
            appended.push_str("\n\n");
            appended.push_str(block);
            appended.push('\n');
            appended
        }
    }
}

/// Removes the managed block, returning the remaining content.
fn strip_block(existing: &str) -> String {
    match extract_block(existing) {
        Some((start, end)) => {
            let mut stripped = String::with_capacity(existing.len());
            stripped.push_str(&existing[..start]);
            stripped.push_str(&existing[end..]);
            let trimmed = stripped.trim_end();
            if trimmed.is_empty() {
                String::new()
            } else {
                format!("{trimmed}\n")
            }
        }
        None => existing.to_string(),
    }
}

/// Every instruction file under `root` carrying the configured name, breadth-first and bounded.
fn walk_instruction_files(root: &Path, file_name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    let mut visited = 0;

    while let Some(directory) = queue.pop() {
        if visited >= MAX_WALK_DIRECTORIES {
            break;
        }
        visited += 1;

        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if path.is_dir() {
                if name.starts_with('.') || EXCLUDED_DIRECTORIES.contains(&name) {
                    continue;
                }
                queue.push(path);
            } else if name == file_name {
                found.push(path);
            }
        }
    }

    found.sort();
    found
}

fn relative_display(project_folder: &Path, path: &Path) -> String {
    path.strip_prefix(project_folder)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Writes one managed block, creating the file when absent.
///
/// Returns `(relative_path, repaired)`, where `repaired` reports that a previously written block
/// had been edited outside Adashi and was restored.
fn write_block(
    project_folder: &Path,
    relative_folder: &str,
    file_name: &str,
    block: &str,
) -> Result<(String, bool), String> {
    let mut directory = project_folder.to_path_buf();
    if !relative_folder.is_empty() {
        for part in relative_folder.split('/') {
            directory.push(part);
        }
    }
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(file_name);

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let repaired = match extract_block(&existing) {
        Some((start, end)) => existing[start..end] != *block && block_revision(&existing).is_some(),
        None => false,
    };

    let next = splice_block(&existing, block);
    if next != existing {
        fs::write(&path, next).map_err(|error| error.to_string())?;
    }
    Ok((relative_display(project_folder, &path), repaired))
}

/// Removes the managed block from one file, deleting the file when nothing else remains.
fn remove_block(path: &Path) -> Result<bool, String> {
    let Ok(existing) = fs::read_to_string(path) else {
        return Ok(false);
    };
    if extract_block(&existing).is_none() {
        return Ok(false);
    }
    let stripped = strip_block(&existing);
    if stripped.is_empty() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    } else {
        fs::write(path, stripped).map_err(|error| error.to_string())?;
    }
    Ok(true)
}

/// Regenerates every projection block for one project.
pub fn regenerate(
    db: &Connection,
    project_row_id: i64,
    project_folder: &Path,
    file_name: &str,
    enabled: bool,
) -> Result<ProjectionReport, String> {
    let mut report = ProjectionReport::default();
    if !enabled {
        return Ok(report);
    }

    let snapshot = load_snapshot(db, project_row_id)?;
    let revision = state::load_project_revision(db, project_row_id)?.revision;
    let blocks = expected_blocks(&snapshot, project_folder, revision);

    let mut expected_paths = BTreeSet::new();
    for (folder, block) in &blocks {
        let (path, repaired) = write_block(project_folder, folder, file_name, block)?;
        if repaired {
            report.repaired.push(path.clone());
        }
        expected_paths.insert(path.clone());
        report.written.push(path);
    }

    // Drop blocks that no longer correspond to bound design, including stale ones left by an
    // earlier configuration.
    for path in walk_instruction_files(project_folder, file_name) {
        let relative = relative_display(project_folder, &path);
        if expected_paths.contains(&relative) {
            continue;
        }
        if remove_block(&path)? {
            report.removed.push(relative);
        }
    }

    report.written.sort();
    report.repaired.sort();
    report.removed.sort();
    Ok(report)
}

/// Removes every managed block Adashi wrote under `project_folder` for one file name.
///
/// Used when projection is disabled or the configured file name changes, so an orphaned block
/// cannot keep being injected.
pub fn remove_managed_blocks(
    project_folder: &Path,
    file_name: &str,
) -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    for path in walk_instruction_files(project_folder, file_name) {
        if remove_block(&path)? {
            removed.push(relative_display(project_folder, &path));
        }
    }
    removed.sort();
    Ok(removed)
}

/// Reports per-file freshness without modifying anything.
pub fn status(
    db: &Connection,
    project_row_id: i64,
    project_folder: &Path,
    file_name: &str,
    enabled: bool,
) -> Result<ProjectionStatus, String> {
    let revision = state::load_project_revision(db, project_row_id)?.revision;
    let mut files = Vec::new();

    if enabled {
        let snapshot = load_snapshot(db, project_row_id)?;
        let blocks = expected_blocks(&snapshot, project_folder, revision);
        for (folder, expected) in &blocks {
            let mut path = project_folder.to_path_buf();
            if !folder.is_empty() {
                for part in folder.split('/') {
                    path.push(part);
                }
            }
            path.push(file_name);
            let relative_path = relative_display(project_folder, &path);

            let state = match fs::read_to_string(&path) {
                Err(_) => "missing",
                Ok(content) => match extract_block(&content) {
                    None => "missing",
                    Some((start, end)) if content[start..end] == *expected => "current",
                    Some(_) => match block_revision(&content) {
                        Some(block) if block == revision => "drifted",
                        _ => "stale",
                    },
                },
            };
            files.push(ProjectionFileStatus {
                path: relative_path,
                state: state.to_string(),
            });
        }
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ProjectionStatus {
        enabled,
        file_name: file_name.to_string(),
        revision,
        error: None,
        files,
    })
}

/// Drops per-project projection overrides for projects that no longer exist.
pub(crate) fn forget_project(settings: &mut crate::settings::AppSettings, project_id: &str) {
    settings
        .architecture_projection
        .enabled_project_ids
        .retain(|id| id != project_id);
    settings
        .architecture_projection
        .project_file_names
        .remove(project_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(id: &str, kind: &str, name: &str, description: &str, parent: Option<&str>) -> Element {
        Element {
            external_id: id.to_string(),
            element_type: kind.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            parent_external_id: parent.map(str::to_string),
        }
    }

    fn snapshot() -> DesignSnapshot {
        DesignSnapshot {
            elements: vec![
                element("1", "Software System", "Adashi", "Local context layer.", None),
                element("5", "Container", "MCP Server", "Passive stdio server.", Some("1")),
                element("6", "Container", "Data Store", "Project-local SQLite.", Some("1")),
                element("9", "Component", "Deep", "Below the top layer.", Some("5")),
                element("2", "Person", "Developer", "Human actor.", None),
            ],
            relationships: vec![
                Relationship {
                    external_id: "r1".into(),
                    source: "5".into(),
                    destination: "6".into(),
                    description: "Reads and writes project-local resources.".into(),
                },
                Relationship {
                    external_id: "r2".into(),
                    source: "5".into(),
                    destination: "9".into(),
                    description: "Internal detail below the top layer.".into(),
                },
            ],
            bindings: Vec::new(),
        }
    }

    #[test]
    fn root_block_carries_responsibilities_and_top_layer_boundaries_only() {
        let block = render_root_block(&snapshot(), 7);
        assert!(block.contains("**Adashi** (Software System) — Local context layer."));
        assert!(block.contains("**MCP Server** (Container) — Passive stdio server."));
        assert!(block.contains("MCP Server -> Data Store: Reads and writes project-local"));
        // Deeper elements and non-top-layer relationships stay out of the root projection.
        assert!(!block.contains("**Deep**"));
        assert!(!block.contains("Internal detail below the top layer."));
        // Actors are not implementable surface.
        assert!(!block.contains("Developer"));
        assert!(block.starts_with(BLOCK_BEGIN));
        assert!(block.ends_with(BLOCK_END));
        assert!(block.contains("revision=7"));
    }

    #[test]
    fn rendering_is_byte_stable_across_runs() {
        assert_eq!(render_root_block(&snapshot(), 3), render_root_block(&snapshot(), 3));
    }

    #[test]
    fn folder_block_lists_owned_elements_boundaries_and_bindings() {
        let snapshot = snapshot();
        let owned = [&snapshot.elements[1]];
        let relationships = [&snapshot.relationships[0], &snapshot.relationships[1]];
        let bindings = [&Binding {
            design_external_id: "5".into(),
            target_type: "file".into(),
            target: "src/mcp.rs".into(),
        }];
        let block =
            render_folder_block("src", &owned, &relationships, &snapshot, &bindings, 4).unwrap();
        assert!(block.contains("**MCP Server** (Container)"));
        assert!(block.contains("Boundaries crossing this folder:"));
        assert!(block.contains("Bound here:"));
        assert!(block.contains("src/mcp.rs"));
        assert!(block.contains("revision=4"));
    }

    #[test]
    fn a_folder_with_no_owned_elements_is_not_projected() {
        let snapshot = snapshot();
        assert_eq!(
            render_folder_block("src", &[], &[], &snapshot, &[], 1),
            None
        );
    }

    #[test]
    fn splice_replaces_only_the_managed_block() {
        let other = format!("# Hand written\n\nKeep me.\n\n{}\nold\n{}", BLOCK_BEGIN, BLOCK_END);
        let spliced = splice_block(&other, &format!("{BLOCK_BEGIN}\nnew\n{BLOCK_END}"));
        assert!(spliced.contains("Keep me."));
        assert!(spliced.contains("new"));
        assert!(!spliced.contains("old"));
    }

    #[test]
    fn splice_appends_without_disturbing_existing_content() {
        let spliced = splice_block("# Hand written\n", &format!("{BLOCK_BEGIN}\nx\n{BLOCK_END}"));
        assert!(spliced.starts_with("# Hand written\n"));
        assert!(spliced.contains(BLOCK_BEGIN));
    }

    #[test]
    fn strip_removes_the_block_and_leaves_other_content() {
        let content = format!("# Keep\n\n{BLOCK_BEGIN}\nx\n{BLOCK_END}\n");
        assert_eq!(strip_block(&content), "# Keep\n");
    }

    #[test]
    fn strip_reports_empty_when_only_the_block_existed() {
        let content = format!("{BLOCK_BEGIN}\nx\n{BLOCK_END}\n");
        assert!(strip_block(&content).is_empty());
    }

    #[test]
    fn bound_paths_that_escape_the_project_are_rejected() {
        let root = Path::new(r"C:\src\Adashi");
        assert_eq!(relative_target(root, "src/mcp.rs").as_deref(), Some("src/mcp.rs"));
        assert_eq!(
            relative_target(root, r"C:\src\Adashi\src\mcp.rs").as_deref(),
            Some("src/mcp.rs")
        );
        assert_eq!(relative_target(root, "../escape.rs"), None);
        assert_eq!(relative_target(root, r"D:\elsewhere\file.rs"), None);
        assert_eq!(relative_target(root, "   "), None);
    }

    #[test]
    fn folders_are_derived_and_excluded_directories_skipped() {
        assert_eq!(folder_of("src/mcp.rs"), "src");
        assert_eq!(folder_of("top.rs"), "");
        assert!(directory_is_excluded("src/target/debug"));
        assert!(directory_is_excluded("node_modules/pkg"));
        assert!(!directory_is_excluded("src/api"));
    }

    #[test]
    fn budget_truncation_drops_from_the_end_and_is_reported() {
        let mut lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        // Two one-character lines plus their newlines fit exactly; the third is dropped.
        assert_eq!(fit_budget(&mut lines, 4), 1);
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);

        let mut lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(fit_budget(&mut lines, 0), 3);
        assert!(lines.is_empty());
    }

    #[test]
    fn description_collapsing_is_single_line_and_bounded() {
        assert_eq!(one_line("a\n  b\tc", 40), "a b c");
        let long = one_line(&"x".repeat(50), 10);
        assert_eq!(long.chars().count(), 11);
        assert!(long.ends_with('…'));
    }

    /// End-to-end lifecycle over a real project database: write, stay byte-stable, repair a
    /// hand edit, then clean up completely when the projection is disabled.
    #[test]
    fn projection_lifecycle_writes_repairs_and_cleans_up() {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-projection-{suffix}"));
        let project_folder = root.join("project");
        fs::create_dir_all(&project_folder).unwrap();
        let project = crate::settings::ProjectSettings {
            id: "projection-test".into(),
            name: "Projection Test".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        let db = crate::open_project_database(&project).unwrap();
        let project_row_id: i64 = db
            .query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let workspace_id: i64 = db
            .query_row("SELECT id FROM design_workspaces ORDER BY id LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();

        db.execute_batch(
            "DELETE FROM design_bindings; DELETE FROM c4_relationships; DELETE FROM c4_elements;",
        )
        .unwrap();
        for (external_id, parent, kind, name, description) in [
            ("sys", None, "Software System", "Adashi", "Local context layer."),
            ("mcp", Some("sys"), "Container", "MCP Server", "Passive stdio server."),
            ("store", Some("sys"), "Container", "Data Store", "Project-local SQLite."),
            ("deep", Some("mcp"), "Component", "Internal", "Below the top layer."),
        ] {
            db.execute(
                "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![workspace_id, external_id, parent, kind, name, description],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO c4_relationships(workspace_id, external_id, source_external_id, destination_external_id, description)
             VALUES(?1, 'r1', 'mcp', 'store', 'Reads and writes project resources.')",
            [workspace_id],
        )
        .unwrap();
        for (target_type, target) in [("file", "src/mcp.rs"), ("file", "src/store.rs")] {
            db.execute(
                "INSERT INTO design_bindings(workspace_id, design_external_id, target_type, target)
                 VALUES(?1, ?2, ?3, ?4)",
                rusqlite::params![
                    workspace_id,
                    if target.ends_with("mcp.rs") { "mcp" } else { "store" },
                    target_type,
                    target
                ],
            )
            .unwrap();
        }

        let report = regenerate(&db, project_row_id, &project_folder, "AGENTS.md", true).unwrap();
        assert_eq!(report.written, vec!["AGENTS.md", "src/AGENTS.md"]);
        assert!(report.repaired.is_empty() && report.removed.is_empty());

        let root_file = project_folder.join("AGENTS.md");
        let folder_file = project_folder.join("src").join("AGENTS.md");
        let root_block = fs::read_to_string(&root_file).unwrap();
        let folder_block = fs::read_to_string(&folder_file).unwrap();
        assert!(root_block.contains("**MCP Server** (Container) — Passive stdio server."));
        assert!(root_block.contains("MCP Server -> Data Store: Reads and writes project resources."));
        assert!(!root_block.contains("**Internal**"));
        assert!(folder_block.contains("Bound here:"));
        assert!(folder_block.contains("src/mcp.rs"));

        // A second identical run writes nothing and keeps the bytes identical.
        let before = fs::read_to_string(&root_file).unwrap();
        let report = regenerate(&db, project_row_id, &project_folder, "AGENTS.md", true).unwrap();
        assert!(report.repaired.is_empty());
        assert_eq!(fs::read_to_string(&root_file).unwrap(), before);

        // A hand edit inside the managed block is detected and restored.
        let edited = before.replace("Passive stdio server.", "Someone edited this.");
        fs::write(&root_file, &edited).unwrap();
        let report = regenerate(&db, project_row_id, &project_folder, "AGENTS.md", true).unwrap();
        assert_eq!(report.repaired, vec!["AGENTS.md"]);
        assert_eq!(fs::read_to_string(&root_file).unwrap(), before);

        // Hand-written content outside the block survives untouched.
        fs::write(&root_file, format!("# House rules\n\n{before}")).unwrap();
        regenerate(&db, project_row_id, &project_folder, "AGENTS.md", true).unwrap();
        assert!(fs::read_to_string(&root_file).unwrap().starts_with("# House rules"));

        // Disabling removes every managed block and deletes files that held nothing else.
        let removed = remove_managed_blocks(&project_folder, "AGENTS.md").unwrap();
        assert_eq!(removed, vec!["AGENTS.md".to_string(), "src/AGENTS.md".to_string()]);
        assert!(!folder_file.exists());
        assert!(root_file.exists());
        assert!(fs::read_to_string(&root_file).unwrap().starts_with("# House rules"));

        let status = status(&db, project_row_id, &project_folder, "AGENTS.md", true).unwrap();
        assert_eq!(status.files.len(), 2);
        assert!(status.files.iter().all(|file| file.state == "missing"));

        // The connection must close before Windows will release the project database file.
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "manual verification against this workspace's real project database"]
    fn verify_against_real_workspace_project() {
        let source = Path::new(r"C:\src\Adashi\.adashi\adashi.sqlite3");
        assert!(source.is_file(), "real project database not available");

        let root = std::env::temp_dir().join("adashi-projection-real");
        let project_folder = root.join("project");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(project_folder.join(".adashi")).unwrap();
        fs::copy(source, project_folder.join(".adashi/adashi.sqlite3")).unwrap();

        let project = crate::settings::ProjectSettings {
            id: "adashi".into(),
            name: "Adashi".into(),
            folder: project_folder.to_string_lossy().into_owned(),
        };
        let db = crate::open_project_database(&project).unwrap();
        let project_row_id: i64 = db
            .query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();

        let report = regenerate(&db, project_row_id, &project_folder, "AGENTS.md", true).unwrap();
        println!("--- written files ({}) ---", report.written.len());
        for path in &report.written {
            let full = project_folder.join(path);
            let bytes = fs::metadata(&full).map(|meta| meta.len()).unwrap_or(0);
            println!("  {bytes:>6} bytes  {path}");
        }

        let root_block = fs::read_to_string(project_folder.join("AGENTS.md")).unwrap();
        println!("\n--- root block ---\n{root_block}\n--- end root block ---");
        assert!(root_block.len() <= ROOT_BUDGET + 600);
        for entry in report.written.iter().filter(|path| path.as_str() != "AGENTS.md") {
            let block = fs::read_to_string(project_folder.join(entry)).unwrap();
            assert!(block.len() <= FOLDER_BUDGET + 600, "{entry} exceeded its budget");
            println!("\n--- {entry} ---\n{block}");
        }

        drop(db);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn disabled_projection_writes_nothing() {
        let root = std::env::temp_dir().join("adashi-projection-disabled");
        let _ = fs::create_dir_all(&root);
        let db = Connection::open_in_memory().unwrap();
        let report = regenerate(&db, 1, &root, "AGENTS.md", false).unwrap();
        assert_eq!(report, ProjectionReport::default());
        assert!(!root.join("AGENTS.md").exists());
        let _ = fs::remove_dir_all(root);
    }
}
