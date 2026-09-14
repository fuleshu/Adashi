//! Grep-shaped search across all project content.
//!
//! Agents grep and read files habitually; they do not fish for opaque ids through narrow tool
//! calls. This module gives that habit a project-content target: one cross-domain search whose
//! locator prefixes are drillable addresses (`design:<external_id>` -> the `adashi_design`
//! get_scope operation, `task:<id>` -> the `adashi_tasks` get operation, `memory:<note_id>` ->
//! the `adashi_memory` get operation with its `noteId` filter).
//!
//! Search scope is project **content**. QA jobs and run evidence, lifecycle rules and the
//! memory protocol rule are project **tooling**: they describe how a project is operated, not
//! what it is, and QA run output is the one field that can detonate a context window. They are
//! excluded by construction rather than by a filter that could drift.
//!
//! Input is tolerant in shape and deterministic in meaning. There is no semantic inference and
//! no ranked relevance: every leniency rule is mechanical, and ordering is domain, then field
//! weight (name and title before description and body before bindings and file lists), then
//! locator. The same query always produces the same output.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::design::{self, DesignBindingRecord, DesignDiagramRecord, DesignElementRecord};
use crate::mockups::{self, MockupSummary};
use crate::tasks;

/// Budget for the whole rendered response. Grep output is read one line at a time, so this
/// bound is what stops a broad query from replacing the answer with a wall of text.
pub const GREP_REPLY_BUDGET: usize = 8 * 1024;
/// Smallest excerpt that still shows the matched text rather than only its neighbourhood.
const MIN_PREVIEW_BUDGET: usize = 40;
/// Characters of context kept on each side of the match inside an excerpt.
const EXCERPT_CONTEXT_CHARS: usize = 60;
/// Hard cap for a matched artifact source: it may be matched, never returned whole.
const WINDOW_CAP: usize = 160;
/// Boundaries shown by the empty-pattern overview before it summarises the rest.
const OVERVIEW_BOUNDARY_LINES: usize = 12;
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;
const MAX_PATTERN_CHARS: usize = 4_096;

/// Domain order is part of the contract: deterministic ordering starts here.
const ALL_DOMAINS: [GrepDomain; 3] = [GrepDomain::Design, GrepDomain::Tasks, GrepDomain::Memory];

// Field weights order matches inside a domain. These are fixed constants rather than scores,
// because ranking that cannot be explained cannot be learned.
const WEIGHT_NAME: u16 = 100;
const WEIGHT_DESCRIPTION: u16 = 200;
const WEIGHT_ARTIFACT: u16 = 250;
const WEIGHT_BINDING: u16 = 300;
const WEIGHT_DEFAULT: u16 = 400;
const WEIGHT_MEMORY_SUMMARY: u16 = 100;
const WEIGHT_MEMORY_BODY: u16 = 200;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum GrepDomain {
    Design,
    Tasks,
    Memory,
}

impl GrepDomain {
    fn as_str(self) -> &'static str {
        match self {
            Self::Design => "design",
            Self::Tasks => "tasks",
            Self::Memory => "memory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GrepScope {
    /// Every project-content domain.
    All,
    Design,
    Tasks,
    Memory,
}

impl GrepScope {
    fn domains(self) -> &'static [GrepDomain] {
        match self {
            Self::All => &ALL_DOMAINS,
            Self::Design => &[GrepDomain::Design],
            Self::Tasks => &[GrepDomain::Tasks],
            Self::Memory => &[GrepDomain::Memory],
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "all" => Some(Self::All),
            "design" => Some(Self::Design),
            "tasks" => Some(Self::Tasks),
            "memory" => Some(Self::Memory),
            _ => None,
        }
    }
}

/// Task state filter for a grep. The vocabulary is the task lifecycle's own, so a search and a
/// task listing can never disagree about what a state is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GrepTaskState {
    Todo,
    Active,
    Finished,
    Closed,
}

impl GrepTaskState {
    /// Task states are closed vocabulary, so an unrecognised state is reported as a typo with the
    /// accepted values rather than being silently searched as text.
    fn from_word(word: &str) -> Result<Self, String> {
        tasks::TaskState::parse(word).map(|state| match state {
            tasks::TaskState::Todo => Self::Todo,
            tasks::TaskState::Active => Self::Active,
            tasks::TaskState::Finished => Self::Finished,
            tasks::TaskState::Closed => Self::Closed,
        })
    }

    fn to_task_state(self) -> tasks::TaskState {
        match self {
            Self::Todo => tasks::TaskState::Todo,
            Self::Active => tasks::TaskState::Active,
            Self::Finished => tasks::TaskState::Finished,
            Self::Closed => tasks::TaskState::Closed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrepParams {
    /// Configured project name (case-insensitive) or project id.
    pub project_name: String,
    /// What to look for: a bare string, whitespace-separated terms (all must appear),
    /// `"quoted phrases"`, or `key:value` clauses for in/file/type/state/limit. Matching is
    /// always case-insensitive, and an unrecognised `key:value` clause is searched as text.
    /// An empty pattern returns the top-layer overview with counts.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Domain scope: all (default), design, tasks, memory.
    #[serde(default)]
    pub r#in: Option<GrepScope>,
    /// Restrict design hits to elements bound to this file or symbol, and task hits to tasks
    /// whose created or changed file lists contain it.
    #[serde(default)]
    pub file: Option<String>,
    /// Restrict design hits to one C4 element type, for example Container.
    #[serde(default)]
    pub r#type: Option<String>,
    /// Restrict task hits to one state.
    #[serde(default)]
    pub state: Option<GrepTaskState>,
    /// Maximum matches returned (default 50, maximum 200); the response budget can lower it.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One search term. `display` keeps the text as the caller wrote it; `needle` is what is
/// matched, always lower-cased.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Term {
    needle: String,
    display: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Clause {
    In(GrepScope),
    File(String),
    Type(String),
    State(GrepTaskState),
    Limit(usize),
}

/// A parsed pattern: the AND-ed terms plus the clauses recognised as filters.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Query {
    terms: Vec<Term>,
    clauses: Vec<Clause>,
}

impl Query {
    fn scope(&self) -> GrepScope {
        self.clauses
            .iter()
            .rev()
            .find_map(|clause| match clause {
                Clause::In(scope) => Some(*scope),
                _ => None,
            })
            .unwrap_or(GrepScope::All)
    }

    fn file(&self) -> Option<&str> {
        self.clauses.iter().rev().find_map(|clause| match clause {
            Clause::File(file) => Some(file.as_str()),
            _ => None,
        })
    }

    fn element_type(&self) -> Option<&str> {
        self.clauses.iter().rev().find_map(|clause| match clause {
            Clause::Type(element_type) => Some(element_type.as_str()),
            _ => None,
        })
    }

    fn task_state(&self) -> Option<GrepTaskState> {
        self.clauses
            .iter()
            .rev()
            .find_map(|clause| match clause {
                Clause::State(state) => Some(*state),
                _ => None,
            })
    }

    fn limit(&self) -> usize {
        self.clauses
            .iter()
            .rev()
            .find_map(|clause| match clause {
                Clause::Limit(limit) => Some(*limit),
                _ => None,
            })
            .unwrap_or(DEFAULT_LIMIT)
    }
}

/// Rendered grep output plus the accounting an agent needs in order to narrow deliberately.
#[derive(Clone, Debug, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrepResult {
    /// The complete grep-shaped output, bounded by the reply budget.
    pub output: String,
    /// True total over every selected domain, before the limit and the budget apply.
    pub total: usize,
    /// Per-domain split of `total`.
    pub counts: GrepCounts,
    /// Matches actually rendered.
    pub shown: usize,
    /// True when the limit or the reply budget left matches out.
    pub truncated: bool,
    /// Bytes of `output`; never larger than the reply budget.
    pub output_bytes: usize,
    /// Whether the pattern contained anything to search for.
    pub pattern_terms: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, rmcp::schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrepCounts {
    pub design: usize,
    pub tasks: usize,
    pub memory: usize,
}

impl GrepCounts {
    fn of(counts: &BTreeMap<GrepDomain, usize>) -> Self {
        Self {
            design: counts.get(&GrepDomain::Design).copied().unwrap_or(0),
            tasks: counts.get(&GrepDomain::Tasks).copied().unwrap_or(0),
            memory: counts.get(&GrepDomain::Memory).copied().unwrap_or(0),
        }
    }
}

/// One searchable field of one artifact.
#[derive(Clone, Debug)]
struct Candidate {
    domain: GrepDomain,
    /// The drillable address, rendered exactly as it appears in the output.
    locator: String,
    kind: String,
    weight: u16,
    /// The whole searchable text of this one field: every term must appear in it, and the
    /// window is taken around the match inside it.
    text: String,
    /// Artifact source (Structurizr DSL, Mermaid source, mockup SVG) and other open-ended
    /// values: matched, and only ever returned as a short window.
    windowed: bool,
}

impl Candidate {
    fn new(
        domain: GrepDomain,
        locator: impl Into<String>,
        kind: impl Into<String>,
        weight: u16,
        text: impl Into<String>,
    ) -> Self {
        Self {
            domain,
            locator: locator.into(),
            kind: kind.into(),
            weight,
            text: text.into(),
            windowed: false,
        }
    }

    fn windowed(mut self) -> Self {
        self.windowed = true;
        self
    }
}

#[derive(Clone, Debug)]
struct Match {
    domain: GrepDomain,
    locator: String,
    kind: String,
    weight: u16,
    text: String,
    windowed: bool,
}

/// Parses the tolerant pattern into terms and clauses.
///
/// Tolerance rules, all mechanical:
/// - bare words, whitespace-separated terms, `"quoted phrases"` and `key:value` clauses all
///   tokenise the same way;
/// - a term that is not recognised as a filter is searched as text, so `sha:abc123` or
///   `port:8080` can never become an error;
/// - only `in`, `file`, `type`, `state` and `limit` are filters, and a malformed value on one
///   of those is reported as a typo rather than silently searched as text.
fn parse_query(pattern: &str) -> Result<Query, String> {
    if pattern.chars().count() > MAX_PATTERN_CHARS {
        return Err(format!(
            "grep.pattern_too_long: patterns are limited to {MAX_PATTERN_CHARS} characters"
        ));
    }

    let mut terms: Vec<Term> = Vec::new();
    let mut clauses: Vec<Clause> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut was_quoted = false;

    fn push_token(
        token: String,
        quoted: bool,
        terms: &mut Vec<Term>,
        clauses: &mut Vec<Clause>,
    ) -> Result<(), String> {
        if token.is_empty() {
            return Ok(());
        }
        let display = if quoted {
            format!("\"{token}\"")
        } else {
            token.clone()
        };
        if !quoted {
            if let Some(colon) = token.find(':') {
                let key = &token[..colon];
                let value = token[colon + 1..].trim();
                if !key.is_empty() && !value.is_empty() {
                    let clause = match key.to_ascii_lowercase().as_str() {
                        "in" => GrepScope::from_word(value).map(Clause::In).ok_or_else(|| {
                            format!(
                                "grep.invalid_scope: '{value}' is not a scope; use all, design, tasks or memory"
                            )
                        })?,
                        "file" => Clause::File(value.to_string()),
                        "type" => Clause::Type(value.to_string()),
                        "state" => Clause::State(GrepTaskState::from_word(value)?),
                        "limit" => {
                            let limit = value.parse::<usize>().map_err(|_| {
                                format!("grep.invalid_limit: '{value}' is not a whole number")
                            })?;
                            if !(1..=MAX_LIMIT).contains(&limit) {
                                return Err(format!(
                                    "grep.invalid_limit: limit must be 1..={MAX_LIMIT}"
                                ));
                            }
                            Clause::Limit(limit)
                        }
                        // An unknown key is text, never an error.
                        _ => {
                            terms.push(Term {
                                needle: token.to_lowercase(),
                                display,
                            });
                            return Ok(());
                        }
                    };
                    clauses.push(clause);
                    return Ok(());
                }
            }
        }
        terms.push(Term {
            needle: token.to_lowercase(),
            display,
        });
        Ok(())
    }

    for character in pattern.chars() {
        match quote {
            Some(active) => {
                if character == active {
                    quote = None;
                    was_quoted = true;
                } else {
                    current.push(character);
                }
            }
            None => match character {
                quote_character @ ('"' | '\'') => {
                    // A quote starts a phrase only at a token boundary; inside a word it is
                    // part of the word, so `it's` and `say"hi"` stay searchable text.
                    if current.is_empty() {
                        quote = Some(quote_character);
                    } else {
                        current.push(quote_character);
                    }
                }
                character if character.is_whitespace() => {
                    let token = std::mem::take(&mut current);
                    let quoted = was_quoted;
                    was_quoted = false;
                    push_token(token, quoted, &mut terms, &mut clauses)?;
                }
                character => current.push(character),
            },
        }
    }
    let token = std::mem::take(&mut current);
    let quoted = was_quoted;
    push_token(token, quoted, &mut terms, &mut clauses)?;

    Ok(Query { terms, clauses })
}

fn matches_all(haystack: &str, terms: &[Term]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let lowered = haystack.to_lowercase();
    terms.iter().all(|term| lowered.contains(&term.needle))
}

/// Window around the earliest match, never cut mid-word at the tail.
///
/// Grep shows the matched line, not the head of the file: head-truncating a field can hide the
/// very text that matched and return a result the agent cannot act on. The match is always
/// inside the returned window, whatever the budget.
fn render_excerpt(text: &str, terms: &[Term], kind: &str, budget: usize) -> Option<String> {
    let lowered = text.to_lowercase();
    let (byte_start, needle_chars) = terms
        .iter()
        .filter_map(|term| {
            lowered
                .find(&term.needle)
                .map(|at| (at, term.needle.chars().count()))
        })
        .min_by_key(|(at, len)| (*at, usize::MAX - *len))?;

    let characters: Vec<char> = text.chars().collect();
    let start = text
        .char_indices()
        .take_while(|(at, _)| *at < byte_start)
        .count();
    let end = (start + needle_chars.max(1)).min(characters.len());

    let mut from = start.saturating_sub(EXCERPT_CONTEXT_CHARS);
    let mut to = (end + EXCERPT_CONTEXT_CHARS).min(characters.len());
    // Never start or end mid-word; that is what makes an excerpt readable as a line.
    while from > 0 && characters[from].is_alphanumeric() {
        from -= 1;
    }
    while to < characters.len() && characters[to - 1].is_alphanumeric() {
        to += 1;
    }

    if to - from > budget {
        // Keep the match visible and spend what is left on the two sides. Bounded fields
        // rarely land here; open-ended artifact sources always do.
        let fit = budget.saturating_sub(3).max(MIN_PREVIEW_BUDGET);
        let mut head = (fit * 2 / 3).min(start - from);
        let mut tail = fit.saturating_sub(head);
        if tail > characters.len() - end {
            tail = characters.len() - end;
            head = fit.saturating_sub(tail).min(start);
        }
        from = start.saturating_sub(head);
        to = (end + tail).min(characters.len());
        if to - from > fit {
            to = from + fit;
        }
        // The match itself is never sacrificed to the budget.
        if start < from || end > to {
            from = start;
            to = end;
        }
    }

    let mut excerpt = String::new();
    if from > 0 {
        excerpt.push('…');
    }
    excerpt.extend(characters[from..to].iter());
    if to < characters.len() {
        excerpt.push('…');
    }
    let collapsed = excerpt.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        // Only reachable when the match itself is whitespace inside a source artifact.
        return Some(format!("(whitespace match in {kind})"));
    }
    Some(collapsed)
}

/// Identity used for deduplication: the same text reachable through a description, a binding
/// and the generated projection collapses to one line, at its best locator.
fn dedup_key(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalize_file(entry: &str) -> String {
    entry.trim().replace('\\', "/").to_lowercase()
}

/// A file filter asks about involvement, so a short needle matches its enclosing path and a
/// full-path needle matches the stored entry it names.
fn file_matches(entries: &[String], needle: &str) -> bool {
    let needle = normalize_file(needle);
    if needle.is_empty() {
        return true;
    }
    entries.iter().any(|entry| {
        let entry = normalize_file(entry);
        entry.contains(&needle) || needle.contains(&entry)
    })
}

struct RelationshipFields {
    external_id: String,
    source_id: String,
    source_name: String,
    destination_id: String,
    destination_name: String,
    description: String,
    technology: String,
    tags: String,
}

struct ProjectContent {
    elements: Vec<DesignElementRecord>,
    relationships: Vec<RelationshipFields>,
    diagrams: Vec<DesignDiagramRecord>,
    bindings: Vec<DesignBindingRecord>,
    mockups: Vec<MockupSummary>,
    tasks: Vec<tasks::Task>,
    memory_summary: String,
    notes: Vec<(String, String)>,
}

/// Relationship rows plus the names of both endpoints, so a relationship is searchable by the
/// component names an agent actually knows.
fn relationship_fields(
    relationships: Vec<design::DesignRelationshipRecord>,
    elements: &[DesignElementRecord],
) -> Vec<RelationshipFields> {
    let names: HashMap<&str, &str> = elements
        .iter()
        .map(|element| (element.external_id.as_str(), element.name.as_str()))
        .collect();

    relationships
        .into_iter()
        .map(|relationship| RelationshipFields {
            source_name: names
                .get(relationship.source_external_id.as_str())
                .map(|name| (*name).to_string())
                .unwrap_or_else(|| relationship.source_external_id.clone()),
            destination_name: names
                .get(relationship.destination_external_id.as_str())
                .map(|name| (*name).to_string())
                .unwrap_or_else(|| relationship.destination_external_id.clone()),
            external_id: relationship.external_id,
            source_id: relationship.source_external_id,
            destination_id: relationship.destination_external_id,
            description: relationship.description,
            technology: relationship.technology,
            tags: relationship.tags,
        })
        .collect()
}

/// Loads only the domains in scope. Design content is read through the design store, so the
/// workspace-scoped queries and their deterministic ordering stay in one place.
fn load_content(
    db: &Connection,
    project_id: i64,
    scope: GrepScope,
    state: Option<GrepTaskState>,
) -> Result<ProjectContent, String> {    let domains = scope.domains();
    let mut content = ProjectContent {
        elements: Vec::new(),
        relationships: Vec::new(),
        diagrams: Vec::new(),
        bindings: Vec::new(),
        mockups: Vec::new(),
        tasks: Vec::new(),
        memory_summary: String::new(),
        notes: Vec::new(),
    };

    if domains.contains(&GrepDomain::Design) {
        let design = design::load_content(db)?;
        content.relationships = relationship_fields(design.relationships, &design.elements);
        content.elements = design.elements;
        content.diagrams = design.diagrams;
        content.bindings = design.bindings;
        content.mockups = mockups::load_summaries(db, project_id)?;
    }

    if domains.contains(&GrepDomain::Tasks) {
        // Same default as a task listing: closed history stays out of a search unless the caller
        // asks for it, so a search never buries current work under accepted work.
        let filter = match state {
            Some(state) => vec![state.to_task_state()],
            None => tasks::default_state_filter(None),
        };
        content.tasks = tasks::load_tasks(db, project_id, &filter)?;
    }

    if domains.contains(&GrepDomain::Memory) {
        content.memory_summary = crate::memory::load_memory(db, project_id)?.memory;
        content.notes = crate::memory::load_retained_notes(db, project_id)?
            .into_iter()
            .filter(|note| note.superseded_by_version.is_none())
            .map(|note| (note.note_id, note.body))
            .collect();
    }

    Ok(content)
}

/// Every searchable field of every in-scope artifact, as a flat list of candidates.
fn candidates(content: &ProjectContent, query: &Query) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let file = query.file();
    let element_type = query.element_type().map(str::to_lowercase);
    let domains = query.scope().domains();

    if domains.contains(&GrepDomain::Design) {
        // Owned so the file-filter closures below can take short borrows of it; nothing here
        // borrows the loaded content while the candidate list is being built.
        let bound_files: HashMap<String, Vec<String>> = {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for binding in &content.bindings {
                map.entry(binding.design_external_id.clone())
                    .or_default()
                    .push(binding.target.clone());
            }
            map
        };
        let bound_targets = |external_id: &str| -> Vec<String> {
            bound_files.get(external_id).cloned().unwrap_or_default()
        };
        let bound_to_file = |external_id: &str| match file {
            None => true,
            Some(file) => file_matches(&bound_targets(external_id), file),
        };

        for element in &content.elements {
            if let Some(wanted) = &element_type {
                if element.element_type.to_lowercase() != *wanted {
                    continue;
                }
            }
            if !bound_to_file(&element.external_id) {
                continue;
            }
            let locator = format!("design:{}", element.external_id);
            for (kind, weight, value) in [
                ("name", WEIGHT_NAME, element.name.clone()),
                ("type", WEIGHT_ARTIFACT, element.element_type.clone()),
                ("description", WEIGHT_DESCRIPTION, element.description.clone()),
                ("technology", WEIGHT_DESCRIPTION, element.technology.clone()),
                ("tags", WEIGHT_DESCRIPTION, element.tags.clone()),
                ("id", WEIGHT_DEFAULT, element.external_id.clone()),
            ] {
                out.push(Candidate::new(
                    GrepDomain::Design,
                    locator.clone(),
                    kind,
                    weight,
                    value,
                ));
            }
        }
        for relationship in &content.relationships {
            if !bound_to_file(&relationship.source_id) && !bound_to_file(&relationship.destination_id)
            {
                continue;
            }
            let locator = format!("design:{}", relationship.external_id);
            // One line per relationship: the endpoints and, when present, its description.
            let mut text = if relationship.description.trim().is_empty() {
                format!(
                    "{} -> {}",
                    relationship.source_name, relationship.destination_name
                )
            } else {
                format!(
                    "{} -> {}: {}",
                    relationship.source_name, relationship.destination_name, relationship.description
                )
            };
            if file.is_some() {
                for target in bound_targets(&relationship.source_id)
                    .into_iter()
                    .chain(bound_targets(&relationship.destination_id))
                {
                    text.push(' ');
                    text.push_str(&target);
                }
            }
            out.push(Candidate::new(
                GrepDomain::Design,
                locator.clone(),
                "relationship",
                WEIGHT_DESCRIPTION,
                text,
            ));
            for (kind, weight, value) in [
                ("technology", WEIGHT_DESCRIPTION, relationship.technology.clone()),
                ("tags", WEIGHT_DESCRIPTION, relationship.tags.clone()),
                ("id", WEIGHT_DEFAULT, relationship.external_id.clone()),
            ] {
                out.push(Candidate::new(
                    GrepDomain::Design,
                    locator.clone(),
                    kind,
                    weight,
                    value,
                ));
            }
        }

        for diagram in &content.diagrams {
            let locator = format!("design:{}", diagram.key);
            for (kind, weight, value) in [
                ("title", WEIGHT_NAME, diagram.title.clone()),
                ("diagramType", WEIGHT_ARTIFACT, diagram.diagram_type.clone()),
                ("artifactLabel", WEIGHT_ARTIFACT, diagram.artifact_label.clone()),
                ("key", WEIGHT_DEFAULT, diagram.key.clone()),
            ] {
                out.push(Candidate::new(
                    GrepDomain::Design,
                    locator.clone(),
                    kind,
                    weight,
                    value,
                ));
            }
            // Mermaid source is matched, and only ever returned as a short window.
            out.push(
                Candidate::new(
                    GrepDomain::Design,
                    locator,
                    "source",
                    WEIGHT_DEFAULT,
                    diagram.source.clone(),
                )
                .windowed(),
            );
        }

        for mockup in &content.mockups {
            let locator = format!("design:{}", mockup.external_id);
            for (kind, weight, value) in [
                ("title", WEIGHT_NAME, mockup.title.clone()),
                ("attachedTo", WEIGHT_BINDING, mockup.attached_to_external_id.clone()),
                ("screen", WEIGHT_ARTIFACT, mockup.screen.clone()),
                ("state", WEIGHT_ARTIFACT, mockup.state.clone()),
                ("fidelity", WEIGHT_ARTIFACT, mockup.fidelity.clone()),
                ("externalId", WEIGHT_DEFAULT, mockup.external_id.clone()),
            ] {
                out.push(Candidate::new(
                    GrepDomain::Design,
                    locator.clone(),
                    kind,
                    weight,
                    value,
                ));
            }
        }

        for binding in &content.bindings {
            if let Some(file) = file {
                // The file filter is about files; a symbol binding has no path to match.
                if binding.target_type != "file" || !file_matches(&[binding.target.clone()], file) {
                    continue;
                }
            }
            // A binding is the bridge between the codebase and the model, so grepping a file
            // path surfaces the component that owns it, and the locator is that component.
            out.push(Candidate::new(
                GrepDomain::Design,
                format!("design:{}", binding.design_external_id),
                binding.target_type.clone(),
                WEIGHT_BINDING,
                binding.target.clone(),
            ));
        }
    }

    if domains.contains(&GrepDomain::Tasks) {
        for task in &content.tasks {
            let locator = format!("task:{}", task.id);
            let files_match = match file {
                None => true,
                Some(file) => {
                    file_matches(&task.created_files, file) || file_matches(&task.changed_files, file)
                }
            };
            if !files_match {
                continue;
            }
            for (kind, weight, value) in [
                ("title", WEIGHT_NAME, task.title.clone()),
                ("description", WEIGHT_DESCRIPTION, task.description.clone()),
                (
                    "completionMemo",
                    WEIGHT_DESCRIPTION,
                    task.completion_memo.clone(),
                ),
            ] {
                out.push(Candidate::new(
                    GrepDomain::Tasks,
                    locator.clone(),
                    kind,
                    weight,
                    value,
                ));
            }
            for (kind, files) in [
                ("createdFiles", &task.created_files),
                ("changedFiles", &task.changed_files),
            ] {
                for entry in files {
                    if let Some(file) = file {
                        if !file_matches(&[entry.clone()], file) {
                            continue;
                        }
                    }
                    out.push(Candidate::new(
                        GrepDomain::Tasks,
                        locator.clone(),
                        kind,
                        WEIGHT_BINDING,
                        entry.clone(),
                    ));
                }
            }
        }
    }

    if domains.contains(&GrepDomain::Memory) {
        out.push(Candidate::new(
            GrepDomain::Memory,
            "memory:summary",
            "summary",
            WEIGHT_MEMORY_SUMMARY,
            content.memory_summary.clone(),
        ));
        for (note_id, body) in &content.notes {
            out.push(Candidate::new(
                GrepDomain::Memory,
                format!("memory:{note_id}"),
                "note",
                WEIGHT_MEMORY_BODY,
                body.clone(),
            ));
        }
    }

    out
}

/// Filters, orders and deduplicates candidates into the final match list.
///
/// Text that repeats inside one artifact collapses to its best field, so a description that
/// also matches a binding is not reported twice; the cross-artifact pass is
/// `drop_duplicate_excerpts`.
fn collect_matches(candidates: &[Candidate], terms: &[Term]) -> Vec<Match> {
    let mut matched: Vec<Match> = candidates
        .iter()
        .filter(|candidate| {
            !candidate.text.trim().is_empty() && matches_all(&candidate.text, terms)
        })
        .map(|candidate| Match {
            domain: candidate.domain,
            locator: candidate.locator.clone(),
            kind: candidate.kind.clone(),
            weight: candidate.weight,
            text: candidate.text.clone(),
            windowed: candidate.windowed,
        })
        .collect();

    matched.sort_by(|left, right| {
        left.domain
            .cmp(&right.domain)
            .then(left.weight.cmp(&right.weight))
            .then(left.locator.cmp(&right.locator))
            .then(left.kind.cmp(&right.kind))
    });

    let mut seen: Vec<(String, String)> = Vec::new();
    let mut out: Vec<Match> = Vec::new();
    for hit in matched {
        let key = (hit.locator.clone(), dedup_key(&hit.text));
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(hit);
    }
    out
}

/// Drops a match whose excerpt duplicates an earlier one: the same sentence reachable through
/// a description, a binding and a generated projection is returned once, at its best locator,
/// which is the first one under the deterministic ordering.
fn drop_duplicate_excerpts(matches: Vec<Match>, terms: &[Term]) -> Vec<Match> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<Match> = Vec::new();
    for hit in matches {
        let Some(excerpt) = render_excerpt(&hit.text, terms, &hit.kind, 240) else {
            continue;
        };
        if seen.contains(&excerpt) {
            continue;
        }
        seen.push(excerpt);
        out.push(hit);
    }
    out
}

fn header(counts: &BTreeMap<GrepDomain, usize>, total: usize) -> String {
    let split = ALL_DOMAINS
        .iter()
        .map(|domain| {
            format!(
                "{}({})",
                domain.as_str(),
                counts.get(domain).copied().unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{total} matches — {split}")
}

fn omission(shown: usize, total: usize) -> String {
    format!(
        "\n… showing {shown} of {total} — narrow the query ({} omitted)",
        total.saturating_sub(shown)
    )
}

/// Renders grep output under the reply budget, reporting what it had to leave out: the header
/// states the true total, and the body shows as much as the budget allows.
///
/// The trailing omission notice is reserved for before any line is written, so the rendered
/// output can never exceed the budget by the width of its own accounting.
fn render_matches(
    matches: &[Match],
    terms: &[Term],
    counts: &BTreeMap<GrepDomain, usize>,
    total: usize,
    limit: usize,
) -> (String, usize, bool) {
    let reserve = omission(total, total).len() + 8;
    let mut body = header(counts, total);
    let mut shown = 0usize;
    for hit in matches.iter().take(limit) {
        let line_budget = GREP_REPLY_BUDGET
            .saturating_sub(body.len())
            .saturating_sub(reserve)
            .saturating_sub(hit.locator.len() + 3);
        if line_budget < MIN_PREVIEW_BUDGET {
            body.push_str(&omission(shown, total));
            return (body, shown, true);
        }
        // A source-class field is never returned whole, and no single line may take more than
        // its share of the budget.
        let allowance = if hit.windowed {
            WINDOW_CAP
        } else {
            GREP_REPLY_BUDGET / 2
        };
        let excerpt = render_excerpt(
            &hit.text,
            terms,
            &hit.kind,
            line_budget.min(allowance),
        )
        .unwrap_or_default();
        let line = format!("\n{}: {}", hit.locator, excerpt);
        if body.len() + line.len() + reserve > GREP_REPLY_BUDGET {
            body.push_str(&omission(shown, total));
            return (body, shown, true);
        }
        body.push_str(&line);
        shown += 1;
    }
    let truncated = matches.len().min(limit) < total;
    if truncated {
        body.push_str(&omission(shown, total));
    }
    debug_assert!(
        body.len() <= GREP_REPLY_BUDGET,
        "grep body was {} bytes",
        body.len()
    );
    (body, shown, truncated)
}

/// Runs one grep query against a project database.
pub fn search(db: &Connection, project_id: i64, params: &GrepParams) -> Result<GrepResult, String> {
    let pattern = params.pattern.as_deref().unwrap_or("").trim().to_string();
    let mut query = parse_query(&pattern)?;

    // Explicit parameters win over clauses: a caller using the documented schema should not
    // have to know the pattern grammar.
    if let Some(scope) = params.r#in {
        query.clauses.push(Clause::In(scope));
    }
    if let Some(file) = params
        .file
        .as_ref()
        .map(|file| file.trim())
        .filter(|file| !file.is_empty())
    {
        query.clauses.push(Clause::File(file.to_string()));
    }
    if let Some(element_type) = params
        .r#type
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        query.clauses.push(Clause::Type(element_type.to_string()));
    }
    if let Some(state) = params.state {
        query.clauses.push(Clause::State(state));
    }
    if let Some(limit) = params.limit {
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(format!("grep.invalid_limit: limit must be 1..={MAX_LIMIT}"));
        }
        query.clauses.push(Clause::Limit(limit));
    }

    let scope = query.scope();
    let content = load_content(db, project_id, scope, query.task_state())?;
    let limit = query.limit();

    if query.terms.is_empty() {
        return Ok(overview(&content, scope, limit));
    }

    let matches = drop_duplicate_excerpts(
        collect_matches(&candidates(&content, &query), &query.terms),
        &query.terms,
    );
    let mut counts: BTreeMap<GrepDomain, usize> = BTreeMap::new();
    for hit in &matches {
        *counts.entry(hit.domain).or_default() += 1;
    }
    let total = matches.len();
    let (output, shown, truncated) = render_matches(&matches, &query.terms, &counts, total, limit);
    let output_bytes = output.len();
    debug_assert!(output_bytes <= GREP_REPLY_BUDGET, "grep output exceeded its budget");

    Ok(GrepResult {
        output,
        total,
        counts: GrepCounts::of(&counts),
        shown,
        truncated,
        output_bytes,
        pattern_terms: query.terms.len(),
    })
}

/// An empty pattern returns the top-layer overview with counts: what an agent with no idea yet
/// actually needs, rather than a dump or an error. It stays top-layer: the Software System, the
/// Containers and a bounded sample of the boundaries that connect them.
fn overview(content: &ProjectContent, scope: GrepScope, limit: usize) -> GrepResult {
    let domains = scope.domains();
    let mut lines: Vec<String> = Vec::new();

    if domains.contains(&GrepDomain::Design) {
        for element in content
            .elements
            .iter()
            .filter(|element| matches!(element.element_type.as_str(), "Software System" | "Container"))
        {
            lines.push(format!(
                "design:{}: {} \"{}\" — {}",
                element.external_id,
                element.element_type,
                element.name,
                one_line(&element.description)
            ));
        }
        let boundaries = content
            .relationships
            .iter()
            .filter(|relationship| !relationship.description.trim().is_empty())
            .collect::<Vec<_>>();
        if !boundaries.is_empty() {
            lines.push("Boundaries:".to_string());
            for relationship in boundaries.iter().take(OVERVIEW_BOUNDARY_LINES) {
                lines.push(format!(
                    "design:{}: {} -> {}: {}",
                    relationship.external_id,
                    relationship.source_name,
                    relationship.destination_name,
                    one_line(&relationship.description)
                ));
            }
            if boundaries.len() > OVERVIEW_BOUNDARY_LINES {
                lines.push(format!(
                    "{} further boundaries — grep a concept or use a locator above",
                    boundaries.len() - OVERVIEW_BOUNDARY_LINES
                ));
            }
        }
        lines.push(format!(
            "design: {} elements, {} relationships, {} diagrams, {} mockups, {} bindings",
            content.elements.len(),
            content.relationships.len(),
            content.diagrams.len(),
            content.mockups.len(),
            content.bindings.len()
        ));
    }

    if domains.contains(&GrepDomain::Tasks) {
        let states = ["todo", "active", "finished"]
            .iter()
            .map(|state| {
                format!(
                    "{state}({})",
                    content.tasks.iter().filter(|task| task.state == *state).count()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("tasks: {} — {states}", content.tasks.len()));
    }

    if domains.contains(&GrepDomain::Memory) {
        lines.push(format!(
            "memory: summary {} characters, {} active notes",
            content.memory_summary.chars().count(),
            content.notes.len()
        ));
    }

    lines.push(
        "Retrieve detail with a locator address, or narrow with in:, file:, type:, state: and limit:."
            .to_string(),
    );

    // Reserve room for the trailing notice before writing any line, so the rendered overview
    // can never exceed the budget and never claims to have shown a line it dropped.
    let reserve = 120;
    let mut output = String::from("Project content overview\n\n");
    let mut shown = 0usize;
    for line in lines.iter() {
        if shown >= limit.max(1) || output.len() + line.len() + 1 + reserve > GREP_REPLY_BUDGET {
            break;
        }
        output.push_str(line);
        output.push('\n');
        shown += 1;
    }
    let truncated = shown < lines.len();
    if truncated {
        output.push_str(&format!(
            "… showing {shown} of {} overview lines — narrow with in: or drill with a locator\n",
            lines.len()
        ));
    }
    let output_bytes = output.len();
    debug_assert!(
        output_bytes <= GREP_REPLY_BUDGET,
        "overview was {output_bytes} bytes"
    );

    GrepResult {
        total: lines.len(),
        counts: GrepCounts {
            design: if domains.contains(&GrepDomain::Design) {
                content.elements.len()
                    + content.relationships.len()
                    + content.diagrams.len()
                    + content.mockups.len()
                    + content.bindings.len()
            } else {
                0
            },
            tasks: if domains.contains(&GrepDomain::Tasks) {
                content.tasks.len()
            } else {
                0
            },
            memory: if domains.contains(&GrepDomain::Memory) {
                content.notes.len() + 1
            } else {
                0
            },
        },
        output,
        shown,
        truncated,
        output_bytes,
        pattern_terms: 0,
    }
}

/// One line of stored text, whitespace-collapsed, for overview lines.
fn one_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut characters = collapsed.chars();
    let head = characters.by_ref().take(160).collect::<String>();
    if characters.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::AdashiMcpServer;
    use crate::memory::{self, AppendMemoryNote};
    use crate::settings::{AppSettings, ProjectSettings, WindowSettings};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn term(needle: &str) -> Term {
        Term {
            needle: needle.to_string(),
            display: needle.to_string(),
        }
    }

    fn terms(query: &str) -> Vec<Term> {
        parse_query(query).unwrap().terms
    }

    // ---------------------------------------------------------------- tolerant parser

    #[test]
    fn parser_separates_terms_phrases_and_known_clauses() {
        let query = parse_query("Concurrency Guard in:design state:active limit:7").unwrap();
        let needles = query
            .terms
            .iter()
            .map(|term| term.needle.as_str())
            .collect::<Vec<_>>();
        assert_eq!(needles, vec!["concurrency", "guard"]);
        assert_eq!(query.scope(), GrepScope::Design);
        assert_eq!(query.task_state(), Some(GrepTaskState::Active));
        assert_eq!(query.limit(), 7);
    }

    #[test]
    fn parser_is_case_insensitive_for_terms_and_clauses() {
        let query = parse_query("REVISION IN:Memory").unwrap();
        assert_eq!(query.terms[0].needle, "revision");
        assert_eq!(query.scope(), GrepScope::Memory);
    }

    #[test]
    fn parser_keeps_quoted_phrases_whole_and_records_them_verbatim() {
        let query = parse_query("\"concurrency guard\" revision").unwrap();
        assert_eq!(
            query.terms,
            vec![
                Term {
                    needle: "concurrency guard".into(),
                    display: "\"concurrency guard\"".into()
                },
                term("revision"),
            ]
        );
        // A phrase only matches when the words are adjacent.
        assert!(matches_all("a concurrency guard here", &query.terms[..1]));
        assert!(!matches_all("concurrency, then guard", &query.terms[..1]));
    }

    #[test]
    fn parser_only_opens_a_phrase_at_a_token_boundary() {
        // A quote inside a word is part of the word, so an apostrophe stays searchable text.
        let query = parse_query("it's project's").unwrap();
        let needles = query
            .terms
            .iter()
            .map(|term| term.needle.as_str())
            .collect::<Vec<_>>();
        assert_eq!(needles, vec!["it's", "project's"]);

        // A quote with no closing partner is left in the token rather than swallowing the rest.
        let query = parse_query("say\"hello world").unwrap();
        let needles = query
            .terms
            .iter()
            .map(|term| term.needle.as_str())
            .collect::<Vec<_>>();
        assert_eq!(needles, vec!["say\"hello", "world"]);
    }

    #[test]
    fn parser_treats_an_unknown_key_as_a_literal_term_and_never_errors() {
        for pattern in ["sha:abc123", "port:8080", "http://example.com/x", "risk:high"] {
            let query = parse_query(pattern)
                .unwrap_or_else(|error| panic!("{pattern} must not be an error: {error}"));
            assert_eq!(query.terms.len(), 1, "{pattern}");
            assert_eq!(query.terms[0].needle, pattern.to_lowercase());
            assert!(query.clauses.is_empty(), "{pattern}");
        }

        // ...and the literal term really is searched as text.
        let query = parse_query("sha:abc123").unwrap();
        assert!(matches_all("commit sha:abc123 landed", &query.terms));
    }

    #[test]
    fn parser_widens_whitespace_separated_terms_to_and() {
        let query = parse_query("revision guard").unwrap();
        assert!(matches_all("the revision guard holds", &query.terms));
        assert!(!matches_all("the revision holds", &query.terms));
        assert!(!matches_all("the guard holds", &query.terms));
    }

    #[test]
    fn parser_rejects_a_malformed_value_on_a_recognised_clause() {
        assert!(parse_query("in:nope").unwrap_err().contains("invalid_scope"));
        let error = parse_query("state:nope").unwrap_err();
        assert!(error.contains("nope"), "{error}");
        assert!(error.contains("todo"), "the error must name the accepted values: {error}");
        assert!(error.contains("closed"), "{error}");
        assert!(parse_query("limit:many").unwrap_err().contains("invalid_limit"));
        assert!(parse_query("limit:0").unwrap_err().contains("invalid_limit"));
        assert!(parse_query(&"x".repeat(MAX_PATTERN_CHARS + 1))
            .unwrap_err()
            .contains("pattern_too_long"));
    }

    #[test]
    fn parser_has_nothing_to_search_for_when_the_pattern_is_blank() {
        for pattern in ["", "   ", "\"\"", "\t\n"] {
            let query = parse_query(pattern).unwrap();
            assert!(query.terms.is_empty(), "{pattern:?}");
        }
    }

    #[test]
    fn file_and_type_clauses_carry_their_values() {
        let query = parse_query("file:src-tauri/src/mcp.rs type:Container").unwrap();
        assert_eq!(query.file(), Some("src-tauri/src/mcp.rs"));
        assert_eq!(query.element_type(), Some("Container"));
        assert!(query.terms.is_empty());
    }

    // ---------------------------------------------------------------- windowing

    #[test]
    fn excerpt_keeps_the_match_visible_and_never_cuts_a_word_at_the_tail() {
        let text = format!("{} NEEDLE {}", "alpha ".repeat(60), "omega ".repeat(60));
        let excerpt = render_excerpt(&text, &terms("needle"), "description", 120).unwrap();
        assert!(excerpt.contains("NEEDLE"), "{excerpt}");
        assert!(excerpt.len() <= 120 + 6 + "NEEDLE".len(), "{} bytes: {excerpt}", excerpt.len());
        let body = excerpt
            .trim_start_matches('…')
            .trim_end_matches('…')
            .trim();
        // Both ends stop on a word boundary, so neither side is a clipped word.
        assert!(
            body.starts_with("alpha") || body.starts_with("NEEDLE"),
            "{body:?}"
        );
        assert!(body.ends_with("omega") || body.ends_with("NEEDLE"), "{body:?}");
        assert!(!body.ends_with("omeg") && !body.ends_with("alph"), "{body:?}");
    }

    #[test]
    fn excerpt_windows_a_long_source_instead_of_returning_it_whole() {
        let head = "structurizr dsl header line\n".repeat(200);
        let text = format!("{head}needle-token{head}");
        let excerpt = render_excerpt(&text, &terms("needle-token"), "source", WINDOW_CAP).unwrap();
        assert!(excerpt.contains("needle-token"), "{excerpt}");
        assert!(
            excerpt.len() <= WINDOW_CAP + 8,
            "source window was {} bytes",
            excerpt.len()
        );
        assert!(text.len() > 5_000, "fixture must be far larger than the window");
    }

    #[test]
    fn excerpt_positions_a_match_at_the_end_of_a_long_field() {
        let text = format!("{}revision guard", "filler ".repeat(4_000));
        let excerpt = render_excerpt(&text, &terms("revision guard"), "body", 120).unwrap();
        assert!(excerpt.contains("revision guard"), "{excerpt}");
        assert!(excerpt.starts_with('…'), "head-truncation must be marked");
        assert!(excerpt.len() <= 130, "{} bytes", excerpt.len());
    }

    #[test]
    fn excerpt_collapses_stored_newlines_to_spaces() {
        let text = "first line\nsecond\tline\n  revision  \nfourth".to_string();
        let excerpt = render_excerpt(&text, &terms("revision"), "body", 200).unwrap();
        assert!(!excerpt.contains('\n'), "{excerpt}");
        assert!(excerpt.contains("second line"), "{excerpt}");
    }

    #[test]
    fn excerpt_handles_unicode_without_splitting_characters() {
        let text = format!("{} 版 revision 界 {}", "🦀".repeat(80), "🦀".repeat(80));
        let excerpt = render_excerpt(&text, &terms("revision"), "description", 60).unwrap();
        assert!(excerpt.contains("revision"), "{excerpt}");
        assert!(excerpt.chars().count() <= 70, "{} chars", excerpt.chars().count());
    }

    // ---------------------------------------------------------------- ordering and dedup

    fn candidate(domain: GrepDomain, locator: &str, kind: &str, weight: u16, text: &str) -> Candidate {
        Candidate::new(domain, locator, kind, weight, text)
    }

    #[test]
    fn ordering_is_domain_then_weight_then_locator_never_relevance() {
        let pool = vec![
            candidate(GrepDomain::Memory, "memory:note-2", "note", WEIGHT_MEMORY_BODY, "revision memory note"),
            candidate(GrepDomain::Design, "design:9", "description", WEIGHT_DESCRIPTION, "revision in a description"),
            candidate(GrepDomain::Design, "design:9", "name", WEIGHT_NAME, "revision in a component name"),
            candidate(GrepDomain::Tasks, "task:4", "title", WEIGHT_NAME, "revision in a task title"),
            candidate(GrepDomain::Design, "design:10", "name", WEIGHT_NAME, "revision in another name"),
        ];

        let ordered = collect_matches(&pool, &terms("revision"))
            .into_iter()
            .map(|hit| format!("{}:{}", hit.locator, hit.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            ordered,
            vec![
                "design:10:name",
                "design:9:name",
                "design:9:description",
                "task:4:title",
                "memory:note-2:note",
            ]
        );

        // The same query over the same content is byte-identical every time.
        let again = collect_matches(&pool, &terms("revision"));
        assert_eq!(ordered.len(), again.len());
    }

    #[test]
    fn deduplication_returns_a_repeated_sentence_once_at_its_best_locator() {
        let pool = vec![
            candidate(GrepDomain::Design, "design:5", "description", WEIGHT_DESCRIPTION, "Shared sentence about revision."),
            candidate(GrepDomain::Design, "design:6", "description", WEIGHT_DESCRIPTION, "Shared sentence about revision."),
            candidate(GrepDomain::Design, "design:5", "file", WEIGHT_BINDING, "Shared sentence about revision."),
            candidate(GrepDomain::Tasks, "task:1", "title", WEIGHT_NAME, "Shared sentence about revision."),
        ];
        let matched = drop_duplicate_excerpts(collect_matches(&pool, &terms("revision")), &terms("revision"));
        assert_eq!(matched.len(), 1, "{matched:#?}");
        assert_eq!(matched[0].locator, "design:5");
        assert_eq!(matched[0].kind, "description");
    }

    // ---------------------------------------------------------------- budget accounting

    #[test]
    fn the_header_states_the_true_total_and_the_per_domain_split() {
        let mut counts = BTreeMap::new();
        counts.insert(GrepDomain::Design, 318);
        counts.insert(GrepDomain::Tasks, 82);
        counts.insert(GrepDomain::Memory, 12);
        assert_eq!(
            header(&counts, 412),
            "412 matches — design(318), tasks(82), memory(12)"
        );
    }

    #[test]
    fn no_result_set_can_exceed_the_reply_budget() {
        // A single common character matches nearly everything, which is exactly the shape
        // that detonated a context window before.
        let pool = (0..4_000)
            .map(|index| {
                Candidate::new(
                    GrepDomain::Design,
                    format!("design:element-{index}"),
                    "description",
                    WEIGHT_DESCRIPTION,
                    format!(
                        "revision element-{index} with a long tail {}{}",
                        "x".repeat(300),
                        "y".repeat(300)
                    ),
                )
            })
            .collect::<Vec<_>>();

        let matched = collect_matches(&pool, &terms("e"));
        assert!(matched.len() > 1_000, "fixture must match broadly");
        let mut counts = BTreeMap::new();
        counts.insert(GrepDomain::Design, matched.len());

        let (output, shown, truncated) = render_matches(&matched, &terms("e"), &counts, matched.len(), MAX_LIMIT);
        assert!(truncated, "the budget must have left matches out");
        assert!(shown < matched.len());
        assert!(
            output.len() <= GREP_REPLY_BUDGET,
            "output was {} bytes, budget is {GREP_REPLY_BUDGET}",
            output.len()
        );
        assert!(output.starts_with(&format!("{} matches — ", matched.len())));
        assert!(output.contains(&format!("showing {shown} of {}", matched.len())));
        assert!(output.contains("narrow the query"));
    }

    #[test]
    fn the_limit_is_reported_as_truncation_without_breaking_the_budget() {
        // Identical text under one locator is one field, so it is returned once.
        let same_artifact = vec![
            candidate(GrepDomain::Tasks, "task:1", "title", WEIGHT_NAME, "revision guard"),
            candidate(GrepDomain::Tasks, "task:1", "createdFiles", WEIGHT_BINDING, "revision guard"),
        ];
        assert_eq!(collect_matches(&same_artifact, &terms("revision")).len(), 1);

        let pool = (0..200)
            .map(|index| {
                Candidate::new(
                    GrepDomain::Tasks,
                    format!("task:{index}"),
                    "title",
                    WEIGHT_NAME,
                    format!("revision guard {index}"),
                )
            })
            .collect::<Vec<_>>();
        let matched = collect_matches(&pool, &terms("revision"));
        let mut counts = BTreeMap::new();
        counts.insert(GrepDomain::Tasks, matched.len());
        let (output, shown, truncated) = render_matches(&matched, &terms("revision"), &counts, matched.len(), 10);
        assert_eq!(shown, 10);
        assert!(truncated);
        assert!(output.contains(&format!("showing 10 of {}", matched.len())));
        assert!(output.len() <= GREP_REPLY_BUDGET);
    }

    // ---------------------------------------------------------------- project fixtures

    struct Fixture {
        root: std::path::PathBuf,
        settings_path: std::path::PathBuf,
        project: ProjectSettings,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("adashi-grep-{label}-{suffix}"));
            let settings_path = root.join("settings.json");
            let project = ProjectSettings {
                id: format!("grep-{label}"),
                name: format!("Grep {label}"),
                folder: root.join("project").to_string_lossy().into_owned(),
            };
            Self {
                root,
                settings_path,
                project,
            }
        }

        fn open(&self) -> (AdashiMcpServer, rusqlite::Connection, i64) {
            crate::settings::save(
                &self.settings_path,
                &AppSettings {
                    window: WindowSettings {
                        width: 1000,
                        height: 700,
                        x: None,
                        y: None,
                    },
                    projects: vec![self.project.clone()],
                    last_active_project_id: Some(self.project.id.clone()),
                    rule_templates: vec![],
                    architecture_projection: Default::default(),
                },
            )
            .unwrap();
            let db = crate::open_project_database(&self.project).unwrap();
            let project_row_id: i64 = db
                .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
                .unwrap();
            (AdashiMcpServer::new(self.settings_path.clone()), db, project_row_id)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn run(server: &AdashiMcpServer, fixture: &Fixture, pattern: &str) -> GrepResult {
        let params = GrepParams {
            project_name: fixture.project.name.clone(),
            pattern: Some(pattern.to_string()),
            r#in: None,
            file: None,
            r#type: None,
            state: None,
            limit: None,
        };
        server.grep_result_for_tests(&params).unwrap()
    }

    fn scoped(
        server: &AdashiMcpServer,
        fixture: &Fixture,
        params: GrepParams,
    ) -> Result<GrepResult, String> {
        let _ = fixture;
        server.grep_result_for_tests(&params)
    }

    fn populate(db: &mut rusqlite::Connection, project_row_id: i64) {
        let workspace_id: i64 = db
            .query_row("SELECT id FROM design_workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        for (external_id, parent, element_type, name, description) in [
            (
                "2",
                None,
                "Software System",
                "Adashi",
                "Local multi-project context layer for agentic coding workspaces",
            ),
            (
                "5",
                Some("2"),
                "Container",
                "Adashi MCP Server",
                "Passive stdio MCP server that exposes deterministic tools to coding agents.",
            ),
            (
                "6",
                Some("2"),
                "Container",
                "Project Data Store",
                "Project-local SQLite database under each project's .adashi folder.",
            ),
            (
                "concurrency-guard",
                Some("6"),
                "Component",
                "Concurrency Guard",
                "Resource-scoped optimistic concurrency: every mutation carries a guard.",
            ),
        ] {
            db.execute(
                "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![workspace_id, external_id, parent, element_type, name, description],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO c4_relationships(workspace_id, external_id, source_external_id, destination_external_id, description)
             VALUES(?1,'rel-1','5','6','Reads and writes project-local resources through deterministic tools.')",
            rusqlite::params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO design_bindings(workspace_id, design_external_id, target_type, target)
             VALUES(?1,'mcp-server','file','src-tauri/src/mcp.rs'),
                    (?1,'concurrency-guard','file','src-tauri/src/concurrency.rs'),
                    (?1,'5','symbol','run_stdio_server')",
            rusqlite::params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO diagrams(workspace_id, kind, key, title, source, diagram_type)
             VALUES(?1,'mermaid','uml-save','Design save flow','sequenceDiagram\n  participant MCP\n  MCP->>Store: revision guard','sequence')",
            rusqlite::params![workspace_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO ui_mockups(project_id, external_id, title, attached_to_external_id, viewport_width, viewport_height, screen, state, fidelity, accepted_svg)
             VALUES(?1,'mockup-home','Home','5',120,80,'Home','Default','static','<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"1\" height=\"1\"/></svg>')",
            rusqlite::params![project_row_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO agent_tasks(project_id, number, title, description, state, completion_memo, created_files, changed_files)
             VALUES(?1, 1, 'Implement v1 QA system from formal design', 'Adds a bounded listing.', 'finished', 'revision checked', ?2, ?3)",
            rusqlite::params![
                project_row_id,
                r#"["src-tauri/src/qa.rs"]"#,
                r#"["src-tauri/src/mcp.rs","src-tauri/src/concurrency.rs"]"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO agent_tasks(project_id, number, title, description, state)
             VALUES(?1, 2, 'Active follow-up', 'Nothing about the revision guard yet.', 'active')",
            rusqlite::params![project_row_id],
        )
        .unwrap();
        db.execute(
            "UPDATE project_memory SET memory_body=?1 WHERE project_id=?2",
            rusqlite::params![
                "Current constraint: every mutation carries a resource-scoped revision guard.",
                project_row_id
            ],
        )
        .unwrap();
        for (note_id, body) in [
            ("note-7", "Routine check: the concurrency guard rejects stale writes."),
            ("note-8", "Unrelated handover about packaging."),
        ] {
            memory::append_note(
                db,
                project_row_id,
                AppendMemoryNote {
                    note_id: note_id.to_string(),
                    operation_id: format!("op-{note_id}"),
                    run_id: format!("run-{note_id}"),
                    task_id: None,
                    body: body.to_string(),
                },
            )
            .unwrap();
        }
        // Project tooling: never searchable.
        db.execute(
            "INSERT INTO rules(project_id, name, enabled, intend, hook, prompt)
             VALUES(?1,'Tooling rule',1,'implementation','run.start','Always consult the revision guard before writing.')",
            rusqlite::params![project_row_id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO qa_jobs(project_id, number, name, description, command)
             VALUES(?1,1,'Concurrency suite','Runs every revision guard test.','cargo test revision')",
            rusqlite::params![project_row_id],
        )
        .unwrap();
        let qa_job_id = db.last_insert_rowid();
        db.execute(
            "INSERT INTO qa_runs(project_id, trigger_source, query_snapshot, status, summary)
             VALUES(?1,'mcp','{}','failed','0 passed, 1 failed')",
            rusqlite::params![project_row_id],
        )
        .unwrap();
        let qa_run_id = db.last_insert_rowid();
        db.execute(
            "INSERT INTO qa_job_runs(qa_run_id, qa_job_id, command_snapshot, status, exit_code, output)
             VALUES(?1,?2,'{}','failed',1,?3)",
            rusqlite::params![
                qa_run_id,
                qa_job_id,
                "the revision guard panicked\n".repeat(4_000)
            ],
        )
        .unwrap();
    }

    #[test]
    fn grep_output_drills_from_a_locator_to_the_addressed_artifact() {
        let fixture = Fixture::new("drill");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);

        let result = run(&server, &fixture, "concurrency guard");
        assert!(result.total > 0, "output was:\n{}", result.output);
        assert_eq!(result.pattern_terms, 2);
        assert!(
            result.output.starts_with(&format!("{} matches — design(", result.total)),
            "{}",
            result.output
        );
        assert!(result.output.contains("design:concurrency-guard"), "{}", result.output);
        assert!(result.output.contains("narrow the query") || !result.truncated);

        // Every design locator in the output resolves through the design retrieval surface.
        for line in result.output.lines().filter(|line| line.starts_with("design:")) {
            let external_id = line
                .trim_start_matches("design:")
                .split(':')
                .next()
                .unwrap()
                .to_string();
            design::load_scope(&db, project_row_id, &external_id, false, Some(0), false)
                .unwrap_or_else(|error| panic!("design:{external_id} did not resolve: {error}"));
        }

        let task = run(&server, &fixture, "in:tasks v1 QA");
        assert!(task.output.contains("task:1:"), "{}", task.output);
        for line in task.output.lines().filter(|line| line.starts_with("task:")) {
            let id = line
                .trim_start_matches("task:")
                .split(':')
                .next()
                .unwrap()
                .parse::<i64>()
                .unwrap();
            tasks::load_task(&db, project_row_id, id).unwrap();
        }

        let note = run(&server, &fixture, "stale writes");
        assert!(note.output.contains("memory:note-7:"), "{}", note.output);
        let addressed: String = db
            .query_row(
                "SELECT note_id FROM project_memory_notes WHERE project_id=?1 AND note_id='note-7'",
                rusqlite::params![project_row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(addressed, "note-7");
    }

    #[test]
    fn grep_excludes_project_tooling() {
        let fixture = Fixture::new("exclusions");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);

        // The rule prompt, the QA job metadata and the QA console output all contain the
        // searched words; none of them is project content.
        let result = run(&server, &fixture, "revision guard");
        assert!(
            !result.output.contains("Tooling rule"),
            "rules leaked:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("Concurrency suite"),
            "QA job metadata leaked:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("panicked"),
            "QA console output leaked:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("qa:") && !result.output.contains("rule:"),
            "a tooling locator leaked:\n{}",
            result.output
        );

        // The scope parameter cannot reach them either.
        for scope in [GrepScope::All, GrepScope::Design, GrepScope::Tasks, GrepScope::Memory] {
            let scoped_result = scoped(
                &server,
                &fixture,
                GrepParams {
                    project_name: fixture.project.name.clone(),
                    pattern: Some("revision".into()),
                    r#in: Some(scope),
                    file: None,
                    r#type: None,
                    state: None,
                    limit: None,
                },
            )
            .unwrap();
            assert!(!scoped_result.output.contains("Concurrency suite"), "{scope:?}");
        }

        // The protocol rule is a rule, and rule text is tooling, so even a pattern taken
        // from the protocol does not reach it.
        let tooling_only = run(&server, &fixture, "PROJECT MEMORY PROTOCOL");
        assert_eq!(tooling_only.total, 0, "{}", tooling_only.output);
    }

    #[test]
    fn artifact_source_is_matched_but_only_returned_as_a_window() {
        let fixture = Fixture::new("source-window");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);
        let huge = format!("sequenceDiagram\n{}participant Store\n", "  A->>B: filler\n".repeat(2_000));
        db.execute(
            "UPDATE diagrams SET source=?1 WHERE key='uml-save'",
            rusqlite::params![huge],
        )
        .unwrap();

        let result = run(&server, &fixture, "in:design participant");
        assert!(result.total > 0, "{}", result.output);
        assert!(
            result.output_bytes <= GREP_REPLY_BUDGET,
            "{} bytes",
            result.output_bytes
        );
        assert!(
            result.output.len() < huge.len() / 4,
            "a whole artifact was returned"
        );
    }

    #[test]
    fn the_file_filter_surfaces_the_owner_and_the_tasks_that_touched_it() {
        let fixture = Fixture::new("file-filter");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);

        let bindings: Vec<(String, String, String)> = db
            .prepare("SELECT design_external_id, target_type, target FROM design_bindings ORDER BY target")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(bindings.len(), 3, "{bindings:?}");
        let by_file = |file: &str| GrepParams {
            project_name: fixture.project.name.clone(),
            pattern: Some("mcp.rs".into()),
            r#in: None,
            file: Some(file.to_string()),
            r#type: None,
            state: None,
            limit: None,
        };

        // "Where is this file involved?" surfaces the component that owns it. The task that
        // touched the same file resolves to the same path text, so the shared sentence is
        // reported once, at the design locator, rather than twice.
        let result = scoped(&server, &fixture, by_file("src-tauri/src/mcp.rs")).unwrap();
        assert!(
            result.output.contains("design:mcp-server"),
            "the owning component is missing:\n{}",
            result.output
        );
        assert_eq!(
            result.output.matches("src-tauri/src/mcp.rs").count(),
            1,
            "the same sentence must be returned once:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("task:2"),
            "a task that neither names the file nor matches the pattern leaked:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("design:concurrency-guard"),
            "an element not bound to the file leaked:\n{}",
            result.output
        );
        assert!(
            !result.output.contains("design:6:"),
            "an element bound to another file leaked:\n{}",
            result.output
        );
        // Memory has no file association, so the filter narrows nothing there: the summary is
        // visible only through its own text, never through a file.
        let memory = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some("Current constraint".into()),
                r#in: None,
                file: Some("src-tauri/src/mcp.rs".into()),
                r#type: None,
                state: None,
                limit: None,
            },
        )
        .unwrap();
        assert!(memory.output.contains("memory:summary:"), "{}", memory.output);
        assert_eq!(memory.counts.design, 0, "{}", memory.output);

        // The task side of the same question: the file list is searchable text, and the task
        // locator resolves through the tasks get operation.
        let tasks_for_file = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some("bounded listing".into()),
                r#in: None,
                file: Some("src-tauri/src/mcp.rs".into()),
                r#type: None,
                state: None,
                limit: None,
            },
        )
        .unwrap();
        assert!(
            tasks_for_file.output.contains("task:1"),
            "the task that touched the file is missing:\n{}",
            tasks_for_file.output
        );
        tasks::load_task(&db, project_row_id, 1).unwrap();

        // Nothing outside the filtered file survives, even when the pattern would match it.
        let single = scoped(&server, &fixture, by_file("src-tauri/src/concurrency.rs")).unwrap();
        assert_eq!(single.counts.design, 0, "{}", single.output);
        assert!(
            !single.output.contains("design:5"),
            "an element bound to another file leaked:\n{}",
            single.output
        );

        // Grepping the filtered file itself names the component that owns it.
        let involved = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some("concurrency.rs".into()),
                r#in: None,
                file: Some("src-tauri/src/concurrency.rs".into()),
                r#type: None,
                state: None,
                limit: None,
            },
        )
        .unwrap();
        assert!(
            involved.output.contains("design:concurrency-guard"),
            "the owning component is missing:\n{}",
            involved.output
        );
        assert!(
            !involved.output.contains("design:5"),
            "an element not bound to the file leaked:\n{}",
            involved.output
        );
        assert!(
            !involved.output.contains("task:2"),
            "a task that never touched the file leaked:\n{}",
            involved.output
        );
    }

    #[test]
    fn file_and_type_filters_bridge_to_the_artifact_they_belong_to() {
        let fixture = Fixture::new("filters");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);

        // in: and type: narrow design without narrowing the other domains.
        let containers = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some("server".into()),
                r#in: None,
                file: None,
                r#type: Some("Container".into()),
                state: None,
                limit: None,
            },
        )
        .unwrap();
        assert!(containers.output.contains("design:5"), "{}", containers.output);
        assert!(
            !containers.output.contains("design:concurrency-guard"),
            "the Component leaked:\n{}",
            containers.output
        );
        // in:/type: are design filters: the other domains are untouched by them.
        assert!(run(&server, &fixture, "revision").counts.tasks > 0);
        assert!(run(&server, &fixture, "revision").counts.memory > 0);

        // type: narrows C4 elements only; diagrams and the other domains are untouched.
        let no_text = run(&server, &fixture, "in:design type:Container revision");
        assert!(
            !no_text.output.contains("design:concurrency-guard"),
            "a non-matching element type leaked:\n{}",
            no_text.output
        );
        assert!(
            no_text.output.contains("design:uml-save"),
            "type: must narrow C4 elements, not remove artifacts:\n{}",
            no_text.output
        );

        // state: uses the task lifecycle vocabulary, and closed work is out of a search by
        // default just as it is out of a listing.
        let active_only = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some("revision".into()),
                r#in: Some(GrepScope::Tasks),
                file: None,
                r#type: None,
                state: Some(GrepTaskState::Active),
                limit: None,
            },
        )
        .unwrap();
        assert!(
            active_only.output.contains("task:2"),
            "the active task must be found: {}",
            active_only.output
        );

        let finished_only = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some("revision".into()),
                r#in: Some(GrepScope::Tasks),
                file: None,
                r#type: None,
                state: Some(GrepTaskState::Finished),
                limit: None,
            },
        )
        .unwrap();
        assert!(
            !finished_only.output.contains("task:2"),
            "an active task must not answer a finished filter: {}",
            finished_only.output
        );
        assert!(
            finished_only.output.contains("task:1"),
            "the finished task must be found: {}",
            finished_only.output
        );
    }

    #[test]
    fn an_empty_pattern_returns_the_top_layer_overview_with_counts() {
        let fixture = Fixture::new("overview");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);

        let result = run(&server, &fixture, "");
        assert_eq!(result.pattern_terms, 0);
        assert!(!result.output.starts_with('0'), "{}", result.output);
        assert!(result.output.contains("Project content overview"), "{}", result.output);
        // The top layer only: the Software System and the two Containers, never the Component.
        assert!(result.output.contains("design:2:"), "{}", result.output);
        assert!(result.output.contains("design:5:"), "{}", result.output);
        assert!(result.output.contains("design:6:"), "{}", result.output);
        assert!(
            !result.output.contains("design:concurrency-guard"),
            "a non-top-layer element leaked:\n{}",
            result.output
        );
        assert!(result.output.contains("Boundaries:"), "{}", result.output);
        assert!(result.output.contains("tasks: 2 — todo(0), active(1), finished(1)"), "{}", result.output);
        assert!(result.output.contains("2 active notes"), "{}", result.output);
        assert_eq!(result.counts.tasks, 2);
        assert_eq!(result.counts.memory, 3);

        // ...and still not the tooling.
        assert!(!result.output.contains("Concurrency suite"), "{}", result.output);

        // The overview's own accounting is honest: the count it reports is the number of lines
        // it actually wrote, and it stays inside the reply budget.
        assert!(result.output_bytes <= GREP_REPLY_BUDGET, "{}", result.output_bytes);
        assert_eq!(
            result
                .output
                .lines()
                .filter(|line| !line.is_empty() && *line != "Project content overview")
                .count(),
            result.shown
        );
        assert_eq!(
            result.truncated,
            result.output.contains("overview lines — narrow")
        );
        assert_eq!(
            result.output.matches("overview lines — narrow").count(),
            usize::from(result.truncated)
        );

        // Whitespace is the same empty pattern.
        assert_eq!(run(&server, &fixture, "   ").pattern_terms, 0);

        // A limit still bounds the overview, and the bound is stated.
        let limited = scoped(
            &server,
            &fixture,
            GrepParams {
                project_name: fixture.project.name.clone(),
                pattern: Some(String::new()),
                r#in: None,
                file: None,
                r#type: None,
                state: None,
                limit: Some(3),
            },
        )
        .unwrap();
        assert_eq!(limited.shown, 3, "{}", limited.output);
        assert!(limited.truncated);
        assert!(limited.output.contains("showing 3 of"), "{}", limited.output);
        assert!(limited.output_bytes <= GREP_REPLY_BUDGET);
    }

    /// `file:` is a scope, not a query. Alone it has nothing to search for, so the empty-pattern
    /// rule takes over and the overview comes back — the same answer as an empty pattern, not an
    /// error and not "everything bound to this file". The instructions that teach `file:` as the
    /// bridge to the codebase must therefore always pair it with a term.
    #[test]
    fn a_file_clause_alone_is_a_scope_not_a_query() {
        let fixture = Fixture::new("file-scope");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);

        let bare = run(&server, &fixture, "file:src-tauri/src/mcp.rs");
        assert_eq!(bare.pattern_terms, 0, "{}", bare.output);
        assert!(
            bare.output.contains("Project content overview"),
            "a bare file: clause must fall through to the overview:\n{}",
            bare.output
        );
        assert!(
            !bare.output.contains("design:mcp-server"),
            "a bare file: clause must not enumerate the bound artifacts:\n{}",
            bare.output
        );

        // The same scope with a term is the documented form, and it does resolve the owner.
        let paired = run(&server, &fixture, "mcp.rs file:src-tauri/src/mcp.rs");
        assert_eq!(paired.pattern_terms, 1, "the file clause is a scope, not a term");
        assert!(
            paired.output.contains("design:mcp-server"),
            "{}",
            paired.output
        );
    }

    #[test]
    fn a_broad_single_letter_query_stays_inside_the_budget() {
        let fixture = Fixture::new("broad");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);
        let workspace_id: i64 = db
            .query_row("SELECT id FROM design_workspaces LIMIT 1", [], |row| row.get(0))
            .unwrap();
        for index in 0..800 {
            db.execute(
                "INSERT INTO c4_elements(workspace_id, external_id, parent_external_id, element_type, name, description)
                 VALUES(?1,?2,'2','Component',?3,?4)",
                rusqlite::params![
                    workspace_id,
                    format!("e{index}"),
                    format!("Element {index}"),
                    "e".repeat(400)
                ],
            )
            .unwrap();
        }

        let result = run(&server, &fixture, "e");
        assert!(result.total > 1_000, "fixture matched only {}", result.total);
        assert!(result.truncated, "{}", result.output);
        assert!(
            result.output_bytes <= GREP_REPLY_BUDGET,
            "output was {} bytes, budget is {GREP_REPLY_BUDGET}",
            result.output_bytes
        );
        assert!(
            result.output.contains(&format!("showing {} of {} ", result.shown, result.total)),
            "{}",
            result.output
        );
        assert!(result.output.contains("narrow the query"));
    }

    #[test]
    fn an_unknown_key_is_searched_as_text_end_to_end() {
        let fixture = Fixture::new("unknown-key");
        let (server, mut db, project_row_id) = fixture.open();
        populate(&mut db, project_row_id);
        let result = run(&server, &fixture, "sha:deadbeef");
        assert_eq!(result.pattern_terms, 1);
        assert_eq!(result.total, 0);
        assert!(result.output.contains("0 matches"), "{}", result.output);
    }

    // ---------------------------------------------------------------- real data

    /// Copies the developer's live project database before opening it, so a real grep can be
    /// printed without ever mutating the live store.
    ///
    /// Run with:
    /// `cargo test --lib --no-default-features grep::tests::real_database -- --ignored --nocapture`
    #[test]
    #[ignore = "manual verification against this workspace's real project database"]
    fn real_database_prints_grep_output_and_every_locator_resolves() {
        let live = std::path::Path::new(r"C:\src\Adashi\.adashi\adashi.sqlite3");
        if !live.exists() {
            eprintln!("skipping: no live database at {}", live.display());
            return;
        }
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("adashi-grep-live-{suffix}"));
        std::fs::create_dir_all(&root).unwrap();
        let copy = root.join("adashi.sqlite3");
        for extension in ["", "-wal", "-shm"] {
            let from = std::path::PathBuf::from(format!("{}{extension}", live.display()));
            if from.exists() {
                std::fs::copy(&from, root.join(format!("adashi.sqlite3{extension}"))).unwrap();
            }
        }
        let db = rusqlite::Connection::open(&copy).unwrap();
        let project_row_id: i64 = db
            .query_row("SELECT id FROM projects LIMIT 1", [], |row| row.get(0))
            .unwrap();

        for pattern in ["concurrency guard", "revision", ""] {
            let params = GrepParams {
                project_name: "Adashi".into(),
                pattern: Some(pattern.to_string()),
                r#in: None,
                file: None,
                r#type: None,
                state: None,
                limit: None,
            };
            let result = search(&db, project_row_id, &params).unwrap();
            println!("=== pattern {:?} ===", pattern);
            println!("{}", result.output);
            println!(
                "--- total {} shown {} truncated {} bytes {}",
                result.total, result.shown, result.truncated, result.output_bytes
            );
            assert!(result.output_bytes <= GREP_REPLY_BUDGET);

            // Every locator in the output must resolve through the retrieval surface.
            for line in result.output.lines() {
                if let Some(rest) = line.strip_prefix("design:") {
                    let external_id = rest.split(':').next().unwrap();
                    let by_ids = design::load_by_ids(&db, project_row_id, &[external_id.to_string()])
                        .unwrap();
                    let resolved = by_ids
                        .elements
                        .iter()
                        .any(|element| element.external_id == external_id)
                        || by_ids
                            .relationships
                            .iter()
                            .any(|relationship| relationship.external_id == external_id)
                        || by_ids.diagrams.iter().any(|diagram| diagram.key == external_id)
                        || by_ids.bindings.iter().any(|binding| {
                            binding.design_external_id == external_id
                        });
                    assert!(resolved, "design:{external_id} did not resolve");
                    if by_ids
                        .elements
                        .iter()
                        .any(|element| element.external_id == external_id)
                    {
                        // Elements are drillable one step further, into their scope.
                        design::load_scope(
                            &db,
                            project_row_id,
                            external_id,
                            false,
                            Some(0),
                            false,
                        )
                        .unwrap_or_else(|error| panic!("design:{external_id}: {error}"));
                    }
                } else if let Some(rest) = line.strip_prefix("task:") {
                    let id = rest.split(':').next().unwrap().parse::<i64>().unwrap();
                    tasks::load_task(&db, project_row_id, id).unwrap();
                } else if let Some(rest) = line.strip_prefix("memory:") {
                    let note_id = rest.split(':').next().unwrap();
                    if note_id == "summary" {
                        memory::load_memory(&db, project_row_id).unwrap();
                    } else {
                        let found: i64 = db
                            .query_row(
                                "SELECT COUNT(*) FROM project_memory_notes WHERE project_id=?1 AND note_id=?2",
                                rusqlite::params![project_row_id, note_id],
                                |row| row.get(0),
                            )
                            .unwrap();
                        assert_eq!(found, 1, "memory:{note_id} did not resolve");
                    }
                }
            }
        }

        let _ = std::fs::remove_dir_all(root);
    }
}
