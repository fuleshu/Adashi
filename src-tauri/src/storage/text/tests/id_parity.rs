//! Numeric identities must match the SQL behavior that predates the adapters.
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
fn all_domain_creation_ids_and_numbers_match_sqlite() {
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
            let job = store.snapshot().unwrap().qa_job(1).unwrap();
            store
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
        }
        // A fresh equivalent upsert also consumes a SQL ID, despite reporting
        // no design changes. Preserve that pre-adapter allocation behavior too.
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
                        change_intent: "Exercise SQL upsert sequence allocation".into(),
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
                    change_intent: "Next ID must match SQL, including consumed upsert IDs".into(),
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
    assert_eq!(numeric_rows(&actual), numeric_rows(&expected));
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
}
