//! Backend-independent contract cases. Future adapters supply their fixture's
//! reopen function; none of these assertions depend on SQLite or filesystem layout.
use super::*;
use crate::rules::{NewRule, Rule};

pub(super) fn draft(name: &str, prompt: &str) -> NewRule {
    NewRule {
        name: name.into(),
        enabled: true,
        intend: "implementation".into(),
        hook: "task.start".into(),
        prompt: prompt.into(),
    }
}

pub(super) fn mutation(operation_id: &str, changes: Vec<RuleChange>) -> RuleMutation {
    RuleMutation {
        operation_id: operation_id.into(),
        changes,
    }
}

fn saved(outcome: &RuleOutcome) -> Rule {
    match outcome {
        RuleOutcome::Saved { rule } => rule.clone(),
        _ => panic!("expected saved rule"),
    }
}

fn json(value: &impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

pub(super) fn assert_rules_contract(open: impl Fn() -> Box<dyn ProjectStorage>) {
    let mut first = open();
    let initial = first.rules_snapshot().unwrap();
    assert_eq!(first.change_cursor().unwrap(), initial.cursor);
    assert_eq!(json(&first.rules_snapshot().unwrap()), json(&initial));

    let create = mutation(
        "contract-create",
        vec![
            RuleChange::Create {
                rule: draft("First", "one"),
            },
            RuleChange::Create {
                rule: draft("Second", "two"),
            },
        ],
    );
    let created = first.mutate_rules(create.clone()).unwrap();
    assert_ne!(created.cursor, initial.cursor);
    let a = saved(&created.outcomes[0]);
    let b = saved(&created.outcomes[1]);
    assert_ne!(a.id, b.id);
    assert!(a.version > 0 && b.version > 0);

    let mut second = open();
    assert_eq!(second.change_cursor().unwrap(), created.cursor);
    assert_eq!(
        json(&second.mutate_rules(create.clone()).unwrap()),
        json(&created)
    );
    assert_eq!(
        second.rules_snapshot().unwrap().rules.len(),
        initial.rules.len() + 2
    );

    let mut reused = create.clone();
    reused.changes.push(RuleChange::Create {
        rule: draft("Third", "three"),
    });
    assert!(matches!(
        second.mutate_rules(reused),
        Err(StorageError::OperationReused)
    ));

    let before_invalid = second.rules_snapshot().unwrap();
    let invalid = mutation(
        "contract-invalid",
        vec![
            RuleChange::Create {
                rule: draft("Valid", "would be rolled back"),
            },
            RuleChange::Create {
                rule: draft(" ", "invalid"),
            },
        ],
    );
    assert!(matches!(
        second.mutate_rules(invalid),
        Err(StorageError::Validation(_))
    ));
    assert_eq!(
        json(&second.rules_snapshot().unwrap()),
        json(&before_invalid)
    );

    let change_a = mutation(
        "contract-a",
        vec![RuleChange::Update {
            id: a.id,
            expected_version: a.version,
            rule: draft("First", "changed"),
        }],
    );
    let changed_a = first.mutate_rules(change_a.clone()).unwrap();
    let a2 = saved(&changed_a.outcomes[0]);
    assert!(a2.version > a.version);
    // The second writer's project cursor is stale, but this resource is unchanged.
    let changed_b = second
        .mutate_rules(mutation(
            "contract-b",
            vec![RuleChange::Update {
                id: b.id,
                expected_version: b.version,
                rule: draft("Second", "changed"),
            }],
        ))
        .unwrap();
    let b2 = saved(&changed_b.outcomes[0]);

    let before_conflict = first.rules_snapshot().unwrap();
    let conflict = first
        .mutate_rules(mutation(
            "contract-conflict",
            vec![
                RuleChange::Update {
                    id: b.id,
                    expected_version: b2.version,
                    rule: draft("Second", "must not leak"),
                },
                RuleChange::Update {
                    id: a.id,
                    expected_version: a.version,
                    rule: draft("First", "stale"),
                },
            ],
        ))
        .unwrap_err();
    match conflict {
        StorageError::Conflict(conflicts) => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].resource_id, a.id.to_string());
            assert_eq!(conflicts[0].current_version, a2.version);
        }
        error => panic!("{error}"),
    }
    assert_eq!(
        json(&first.rules_snapshot().unwrap()),
        json(&before_conflict)
    );

    let noop = first
        .mutate_rules(mutation(
            "contract-noop",
            vec![RuleChange::Update {
                id: a.id,
                expected_version: a2.version,
                rule: draft("First", "changed"),
            }],
        ))
        .unwrap();
    assert_eq!(noop.cursor, before_conflict.cursor);
    assert_eq!(saved(&noop.outcomes[0]).version, a2.version);
    // Replaying an old create returns its original cursor/result, not current data.
    assert_eq!(json(&first.mutate_rules(create).unwrap()), json(&created));
    assert_eq!(first.change_cursor().unwrap(), before_conflict.cursor);

    let deleted = mutation(
        "contract-delete",
        vec![RuleChange::Delete {
            id: a.id,
            expected_version: a2.version,
        }],
    );
    let result = first.mutate_rules(deleted.clone()).unwrap();
    match &result.outcomes[0] {
        RuleOutcome::Deleted { id, version } => {
            assert_eq!(*id, a.id);
            assert!(*version > a2.version);
        }
        _ => panic!("expected deleted rule"),
    }
    assert_eq!(json(&second.mutate_rules(deleted).unwrap()), json(&result));
    let stale = mutation(
        "contract-deleted-stale",
        vec![RuleChange::Update {
            id: a.id,
            expected_version: a2.version,
            rule: draft("First", "resurrection"),
        }],
    );
    assert!(matches!(
        first.mutate_rules(stale),
        Err(StorageError::Conflict(_))
    ));
    assert!(!first
        .rules_snapshot()
        .unwrap()
        .rules
        .iter()
        .any(|rule| rule.id == a.id));
}
