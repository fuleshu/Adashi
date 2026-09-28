//! On-demand Adashi skills.
//!
//! The always-on workflow (`agents_template.md`) is intentionally short: it carries the
//! lifecycle contract, the write discipline and an index into these skills. A skill is
//! compiled into the server so it is available before any project exists and always matches
//! the running contract; `adashi_help` serves it, and projection mirrors it into the project.
use serde_json::{Value, json};

/// One on-demand instruction document.
#[derive(Clone, Copy)]
pub struct Skill {
    /// Stable selector used by `adashi_help` and by the projection file name.
    pub name: &'static str,
    /// One-line title for the catalog.
    pub title: &'static str,
    /// When an agent should read this skill.
    pub when: &'static str,
    /// Full Markdown body.
    pub body: &'static str,
}

pub const SKILLS: &[Skill] = &[
    Skill {
        name: "design-authoring",
        title: "Formal design authoring",
        when: "Before creating or changing C4 elements, relationships, UML artifacts, bindings or mockups.",
        body: include_str!("../skills/design-authoring.md"),
    },
    Skill {
        name: "markdown-documents",
        title: "Markdown design documents",
        when: "Before writing or editing Markdown design prose, generated docs, or an AGENTS.md architecture block.",
        body: include_str!("../skills/markdown-documents.md"),
    },
    Skill {
        name: "task-workflow",
        title: "Task workflow",
        when: "Before creating, updating, finishing or closing a task, or resolving a task number.",
        body: include_str!("../skills/task-workflow.md"),
    },
    Skill {
        name: "qa-jobs",
        title: "QA jobs",
        when: "Before creating, changing or running a QA job.",
        body: include_str!("../skills/qa-jobs.md"),
    },
    Skill {
        name: "retrieval",
        title: "Searching project context",
        when: "Before searching design, tasks or memory, or when grep output is too broad.",
        body: include_str!("../skills/retrieval.md"),
    },
    Skill {
        name: "memory",
        title: "Project memory",
        when: "When a prior decision, constraint or blocker is needed, or before writing a handover note.",
        body: include_str!("../skills/memory.md"),
    },
    Skill {
        name: "write-recovery",
        title: "Write recovery",
        when: "When an Adashi write is rejected and the correct repair is not obvious.",
        body: include_str!("../skills/write-recovery.md"),
    },
];

/// Resolves a skill selector. Names are stable and case-insensitive.
pub fn get(name: &str) -> Option<&'static Skill> {
    let name = name.trim();
    SKILLS.iter().find(|skill| skill.name.eq_ignore_ascii_case(name))
}

pub fn names() -> Vec<&'static str> {
    SKILLS.iter().map(|skill| skill.name).collect()
}

/// Compact catalog for `adashi_help` and error details.
pub fn catalog() -> Value {
    json!(SKILLS
        .iter()
        .map(|skill| json!({"name": skill.name, "title": skill.title, "when": skill.when}))
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_skill_is_addressable_and_nonempty() {
        assert!(SKILLS.len() >= 7);
        for skill in SKILLS {
            assert_eq!(get(skill.name).map(|s| s.name), Some(skill.name));
            assert_eq!(get(&skill.name.to_uppercase()).map(|s| s.name), Some(skill.name));
            assert!(skill.body.len() > 200, "{}", skill.name);
            assert!(skill.when.len() > 10, "{}", skill.name);
            assert!(!skill.title.is_empty(), "{}", skill.name);
        }
        assert!(get("no-such-skill").is_none());
        assert_eq!(catalog().as_array().unwrap().len(), SKILLS.len());
    }

    /// The always-on workflow must index every skill, or a skill becomes unreachable.
    #[test]
    fn the_always_on_workflow_indexes_every_skill() {
        let workflow = crate::projection::AGENT_WORKFLOW;
        for name in names() {
            assert!(
                workflow.contains(&format!("`{name}`")),
                "agents_template.md does not index skill {name}"
            );
        }
    }
}
