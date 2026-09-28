//! Random text allocation across domains, with stable conversion identities.
use super::*;

fn numeric_rows(rows: &engine::Rows) -> BTreeMap<String, Vec<(i64, Option<i64>)>> {
    let mut result = BTreeMap::<String, Vec<(i64, Option<i64>)>>::new();
    for ((collection, _), data) in rows {
        if collection == "mutation_operations" {
            continue; // Checkout-local receipts are deliberately not project records.
        }
        if let Some(id) = data.get("id").and_then(Value::as_i64) {
            result
                .entry(collection.clone())
                .or_default()
                .push((id, data.get("number").and_then(Value::as_i64)));
        }
    }
    for values in result.values_mut() {
        values.sort();
    }
    result
}

#[test]
fn all_domain_creation_ids_are_randomized_in_text_and_stable_after_reopen() {
    let (_dir, source_request, mut source) = sqlite::tests::fixture();
    let target = api::OpenRequest {
        location: Path::new(&source_request.location)
            .parent()
            .unwrap()
            .join("text")
            .to_string_lossy()
            .into_owned(),
        ..source_request.clone()
    };
    let sql = sqlite::SqliteStorage::open_request(&source_request).unwrap();
    let image = transfer::from_sqlite(
        sql.connection().unwrap(),
        None,
        &Path::new(&target.location).parent().unwrap().join("local"),
    )
    .unwrap();
    write_files(Path::new(&target.location), &image.files);
    let mut text = store(&target);
    sqlite::tests::conformance::populate(&mut source);
    sqlite::tests::conformance::populate(&mut text);
    for store in [&mut source as &mut dyn ProjectStorage, &mut text] {
        for operation in ["qa-first", "qa-second"] {
            let job = store
                .snapshot()
                .unwrap()
                .qa_jobs(&api::qa::QaJobQuery::default())
                .unwrap()
                .remove(0);
            let result = store
                .commit(Mutation {
                    operation_id: operation.into(),
                    changes: vec![Change::Qa(api::QaWrite::StartRun {
                        query: api::qa::QaJobQuery::default(),
                        trigger_source: "test".into(),
                        jobs: vec![api::QaExecutionPlan {
                            job_id: job.id,
                            expected_version: job.version,
                            command_snapshot: crate::qa_runner::command_snapshot(&job).unwrap(),
                        }],
                    })],
                })
                .unwrap();
            let api::ChangeOutcome::QaRun(run) = result.outcomes.into_iter().next().unwrap()
            else {
                panic!("qa run outcome");
            };
            let job_run = run.job_runs.first().unwrap().clone();
            // A job cannot be reserved twice while its run is live, so close the
            // evidence before the next run of the same job.
            let versions = store
                .snapshot()
                .unwrap()
                .resource_versions(&[api::ResourceKey {
                    kind: "qa.job-run".into(),
                    id: job_run.id.to_string(),
                }])
                .unwrap();
            store
                .commit(Mutation {
                    operation_id: format!("{operation}-evidence"),
                    changes: vec![Change::Qa(api::QaWrite::CompleteJob {
                        job_run_id: job_run.id,
                        expected_version: versions[0].version,
                        evidence: api::QaEvidence {
                            outcome: api::QaJobOutcome::Passed,
                            exit_code: Some(0),
                            duration_ms: 1,
                            output: "ok".into(),
                        },
                    })],
                })
                .unwrap();
            let run_version = store
                .snapshot()
                .unwrap()
                .resource_versions(&[api::ResourceKey {
                    kind: "qa.run".into(),
                    id: run.id.to_string(),
                }])
                .unwrap()[0]
                .version;
            store
                .commit(Mutation {
                    operation_id: format!("{operation}-complete"),
                    changes: vec![Change::Qa(api::QaWrite::CompleteRun {
                        run_id: run.id,
                        expected_version: run_version,
                    })],
                })
                .unwrap();
        }
        // Exercise repeated and equivalent upserts before another allocation.
        // Existing identities survive, while text reserves fresh random gaps.
        for (operation, name) in [
            ("rename-one", "One"),
            ("rename-two", "Two"),
            ("same-name-new-operation", "Two"),
        ] {
            let doc = store
                .snapshot()
                .unwrap()
                .design_documents(&["element:app".into()])
                .unwrap()
                .remove(0);
            let mut change = doc.document.clone();
            change["op"] = "upsert_element".into();
            change["name"] = name.into();
            store
                .commit(Mutation {
                    operation_id: operation.into(),
                    changes: vec![Change::Design(api::DesignWrite::Save {
                        change_intent: "Exercise repeated upserts and stable identities".into(),
                        changes: vec![serde_json::from_value(change).unwrap()],
                        read_tokens: vec![api::documents::DocumentReadToken {
                            document_id: doc.document_id,
                            read_token: doc.read_token,
                        }],
                    })],
                })
                .unwrap();
        }
        store
            .commit(Mutation {
                operation_id: "after-upserts".into(),
                changes: vec![Change::Design(api::DesignWrite::Save {
                    change_intent: "Allocate another identity after upserts".into(),
                    changes: serde_json::from_value(serde_json::json!([{
                        "op":"upsert_element", "externalId":"after-upserts", "parentExternalId":"1",
                        "elementType":"Container", "name":"After upserts"
                    }]))
                    .unwrap(),
                    read_tokens: vec![],
                })],
            })
            .unwrap();
    }
    let sql = sqlite::SqliteStorage::open_request(&source_request).unwrap();
    let tables = engine::tables(sql.connection().unwrap()).unwrap();
    let expected = engine::dump(sql.connection().unwrap(), &tables).unwrap();
    let files = journal::inventory(Path::new(&target.location)).unwrap();
    let records = engine::parse_records(&files).unwrap();
    let actual = engine::rows_from_records(&records, &tables).unwrap();
    let expected = numeric_rows(&expected);
    let actual = numeric_rows(&actual);
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    let mut randomized = 0;
    for (name, values) in &actual {
        assert_eq!(values.len(), expected[name].len(), "{name}");
        assert!(values.iter().all(|(id, _)| *id > 0 && *id <= MAX_SAFE));
        if values != &expected[name] { randomized += 1; }
    }
    assert!(randomized >= 12, "expected randomized allocations across domains, got {randomized}");
    drop(text);
    let mut reopened = store(&target);
    reopened.snapshot().unwrap();
    assert_eq!(journal::inventory(Path::new(&target.location)).unwrap(), files);
}

#[test]
fn every_numeric_collection_resumes_after_live_and_deleted_ids_without_gaps() {
    let db = engine::empty().unwrap();
    let tables = engine::tables(&db).unwrap();
    let mut tombstones = BTreeMap::new();
    let mut names = Vec::new();
    for table in &tables {
        let primary = table.primary();
        if primary.len() != 1 || primary[0].name != "id" {
            continue;
        }
        let identity = uuid::Uuid::new_v4().to_string();
        tombstones.insert(
            identity.clone(),
            codec::Record {
                identity,
                schema_version: 1,
                collection: table.name.clone(),
                deleted: true,
                data: BTreeMap::from([("id".into(), 123.into())]),
            },
        );
        names.push(table.name.clone());
    }
    assert!(names.len() > 20, "inventory must cover every domain");
    eprintln!(
        "Numeric collections covered: {}: {}",
        names.len(),
        names.join(", ")
    );
    sequences::restore(&db, &tables, &tombstones).unwrap();
    for name in names {
        let high: i64 = db
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name=?1",
                [&name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(high, 123, "{name}: next generated ID must be 124");
    }
    sequences::randomize(&db, &tables, &tombstones).unwrap();
    for name in tombstones.values().map(|r| &r.collection) {
        let seed: i64 = db.query_row("SELECT seq FROM sqlite_sequence WHERE name=?1", [name], |r| r.get(0)).unwrap();
        assert!(seed > 123 && seed < MAX_SAFE, "{name}: text allocation reserves a random gap");
    }
}
