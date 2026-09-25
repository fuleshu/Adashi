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

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::storage::sqlite::health::*;
#[cfg(test)]
use rusqlite::{params, Connection};

pub use adashi_storage_api::health::*;

/// What the model says, as the scan needs it.
pub(crate) struct Model {
    /// `(external_id, name, element_type, parent_external_id)`
    pub(crate) elements: Vec<(String, String, String, Option<String>)>,
    /// `(design_external_id, target_type, target)`
    pub(crate) bindings: Vec<(String, String, String)>,
    /// Every external id named by at least one relationship, either end, plus every id that holds
    /// children. A top-level element is reached through its children rather than through a parent,
    /// and a relationship that happens to name nothing at the top must not orphan it.
    pub(crate) connected: BTreeSet<String>,
}

/// Runs the scan against a project folder and records the result.

pub fn scan_snapshot(
    snapshot: &dyn adashi_storage_api::ReadSnapshot,
    project_folder: &Path,
) -> Result<DesignHealthResult, String> {
    use adashi_storage_api::{HealthCacheKey, LocalDerivedCache};
    let content = snapshot.design_inventory().map_err(|e| e.to_string())?;
    let mut connected = BTreeSet::new();
    for relation in &content.relationships {
        connected.insert(relation.value.source_external_id.clone());
        connected.insert(relation.value.destination_external_id.clone());
    }
    for element in &content.elements {
        if let Some(parent) = &element.value.parent_external_id {
            connected.insert(parent.clone());
        }
    }
    let mut model = Model {
        elements: content
            .elements
            .into_iter()
            .map(|v| {
                (
                    v.value.external_id,
                    v.value.name,
                    v.value.element_type,
                    v.value.parent_external_id,
                )
            })
            .collect(),
        bindings: content
            .bindings
            .into_iter()
            .map(|v| (v.design_external_id, v.target_type, v.target))
            .collect(),
        connected,
    };
    // A Markdown design is project-owned even without architecture associations.
    // Validate claimed code bindings without inventing a C4 parent requirement.
    for document in content.markdown {
        model.connected.insert(document.external_id.clone());
        model.elements.push((document.external_id, document.title, "Markdown".into(), None));
    }
    let result = scan(project_folder, model)?;
    let key = HealthCacheKey {
        project_id: snapshot.metadata().identity.id.clone(),
        computer_id: crate::computer::id()?.into(),
        cursor: snapshot.metadata().cursor.clone(),
    };
    let _ = crate::storage::local_cache::FileDerivedCache::for_checkout(project_folder)
        .record_health(&key, &result);
    Ok(result)
}

/// The pure scan: model plus files in, findings out. No database, no clock, no side effects.
pub(crate) fn scan(project_folder: &Path, model: Model) -> Result<DesignHealthResult, String> {
    let mut findings = Vec::new();

    for (external_id, name, element_type, parent_external_id) in &model.elements {
        let attached =
            parent_external_id.is_some() || model.connected.contains(external_id.as_str());

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
        } else if element_type == "Markdown" && file_targets.is_empty() && symbol_targets.is_empty() {
            (ElementHealth::Resolved, "Project-level Markdown design; no code binding is claimed.".into())
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

    let broken_bindings = findings
        .iter()
        .map(|finding| finding.broken.len() as u32)
        .sum();
    let counts = HealthCounts {
        orphaned: findings
            .iter()
            .filter(|f| f.state == ElementHealth::Orphaned)
            .count() as u32,
        unmapped: findings
            .iter()
            .filter(|f| f.state == ElementHealth::Unmapped)
            .count() as u32,
        broken: findings
            .iter()
            .filter(|f| f.state == ElementHealth::Broken)
            .count() as u32,
        resolved: findings
            .iter()
            .filter(|f| f.state == ElementHealth::Resolved)
            .count() as u32,
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
    let canonical_root = project_folder
        .canonicalize()
        .unwrap_or_else(|_| project_folder.to_path_buf());
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
            && extension
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
    })
}

/// Writes the scan result, replacing the previous verdict for this project.

/// The recorded state of every element, without touching the filesystem.
///
/// Returns the state and the files the bindings resolved to, so a client can show what an element
/// owns without a scan.

/// The recorded counts, without touching the filesystem.

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

/// The recorded state of one element, with the files its bindings resolved to.
#[allow(dead_code)]

/// The findings that were reviewed and knowingly kept, newest first.
#[allow(dead_code)]

/// Records a finding that was reviewed and knowingly kept. The reason is required: keeping a
/// finding without one is a silence, and silence is what made the drift invisible.
#[allow(dead_code)]
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
                    (
                        (*id).to_string(),
                        (*kind).to_string(),
                        (*target).to_string(),
                    )
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
            model(&[("a", "A", "Component", Some("root"))], &[], &[]),
        )
        .unwrap();
        let element = finding(&result, "a");
        assert_eq!(element.state, ElementHealth::Unmapped);
        assert!(
            element
                .detail
                .contains("nothing is claimed about where its code lives"),
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
                &[
                    ("a", "file", "src/a.rs"),
                    ("a", "symbol", "src/a.rs::Thing"),
                ],
                &[],
            ),
        )
        .unwrap();
        assert_eq!(finding(&resolved, "a").state, ElementHealth::Resolved);

        let absent = scan(
            &root,
            model(
                &[("a", "A", "Component", Some("root"))],
                &[
                    ("a", "file", "src/a.rs"),
                    ("a", "symbol", "src/a.rs::Absent"),
                ],
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
                &[
                    ("a", "file", "src/a.rs"),
                    ("a", "symbol", "src/a.rs::Absent"),
                ],
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
            recorded
                .values()
                .filter(|(_, files)| !files.is_empty())
                .count()
        );
        for (external_id, (_, files)) in recorded
            .iter()
            .filter(|(_, (_, files))| !files.is_empty())
            .take(5)
        {
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
        println!(
            "resolved elements: {} ({resolved_with_files} with files)",
            resolved.len()
        );
        for element in resolved.iter().take(5) {
            println!("  {} owns {:?}", element.design_external_id, element.files);
        }
        // The recorded files are what the design view shows, so an empty file list means an
        // element appears to own nothing even though its bindings resolved.
        let recorded = recorded_states(&db, project_id).unwrap();
        let recorded_with_files = recorded
            .values()
            .filter(|(_, files)| !files.is_empty())
            .count();
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
                println!(
                    "         broken: {} {}",
                    binding.target_type, binding.target
                );
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
