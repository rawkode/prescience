//! Integration tests against a live SpiceDB instance.
//!
//! Requires SpiceDB running locally. To start one:
//!   spicedb serve --grpc-preshared-key test-key --datastore-engine memory --grpc-no-tls &
//!
//! Configure via environment variables (defaults shown):
//!   SPICEDB_ENDPOINT=http://localhost:50051
//!   SPICEDB_TOKEN=test-key

use prescience::{
    Client, Consistency, ObjectReference, PermissionResult, Relationship, RelationshipFilter,
    RelationshipUpdate, SubjectReference,
};
use tokio_stream::StreamExt;

fn endpoint() -> String {
    std::env::var("SPICEDB_ENDPOINT").unwrap_or_else(|_| "http://localhost:50051".into())
}

fn token() -> String {
    std::env::var("SPICEDB_TOKEN").unwrap_or_else(|_| "test-key".into())
}

async fn client() -> Client {
    Client::new(&endpoint(), &token()).await.expect("failed to connect to SpiceDB")
}

// ── Schema ────────────────────────────────────────────────────

const TEST_SCHEMA: &str = r#"
definition user {}

definition document {
    relation viewer: user
    relation editor: user

    permission view = viewer + editor
    permission edit = editor
}
"#;

#[tokio::test]
async fn write_and_read_schema() {
    let c = client().await;

    let written_at = c.write_schema(TEST_SCHEMA).await.expect("write_schema failed");
    assert!(!written_at.token().is_empty());

    let (schema_text, read_at) = c.read_schema().await.expect("read_schema failed");
    assert!(schema_text.contains("definition document"));
    assert!(!read_at.token().is_empty());
}

#[tokio::test]
async fn write_schema_empty_rejected() {
    let c = client().await;
    let err = c.write_schema("").await.unwrap_err();
    assert!(matches!(err, prescience::Error::InvalidArgument(_)));
}

// ── Relationships ─────────────────────────────────────────────

#[tokio::test]
async fn write_and_check_permission() {
    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "doc-1").unwrap(),
            "viewer",
            SubjectReference::new(ObjectReference::new("user", "alice").unwrap(), None::<String>)
                .unwrap(),
        ))])
        .await
        .expect("write_relationships failed");

    // Check: alice should have view on doc-1
    let result = c
        .check_permission(
            &ObjectReference::new("document", "doc-1").unwrap(),
            "view",
            &SubjectReference::new(
                ObjectReference::new("user", "alice").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .consistency(Consistency::AtLeastAsFresh(token.clone()))
        .await
        .expect("check_permission failed");

    assert!(result.is_allowed().unwrap());
    assert_eq!(result, PermissionResult::Allowed);

    // Check: alice should NOT have edit on doc-1
    let result = c
        .check_permission(
            &ObjectReference::new("document", "doc-1").unwrap(),
            "edit",
            &SubjectReference::new(
                ObjectReference::new("user", "alice").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .consistency(Consistency::AtLeastAsFresh(token))
        .await
        .expect("check_permission failed");

    assert!(!result.is_allowed().unwrap());
    assert_eq!(result, PermissionResult::Denied);
}

#[tokio::test]
async fn read_relationships() {
    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let token = c
        .write_relationships(vec![
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "doc-read-1").unwrap(),
                "viewer",
                SubjectReference::new(
                    ObjectReference::new("user", "bob").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            )),
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "doc-read-1").unwrap(),
                "editor",
                SubjectReference::new(
                    ObjectReference::new("user", "carol").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            )),
        ])
        .await
        .unwrap();

    let filter = RelationshipFilter::new("document").resource_id("doc-read-1");
    let mut stream = c
        .read_relationships(filter)
        .consistency(Consistency::AtLeastAsFresh(token))
        .send()
        .await
        .expect("read_relationships failed");

    let mut count = 0;
    while let Some(result) = stream.next().await {
        let item = result.expect("stream item error");
        assert_eq!(item.relationship.resource.object_type(), "document");
        assert_eq!(item.relationship.resource.object_id(), "doc-read-1");
        count += 1;
    }
    assert_eq!(count, 2);
}

#[tokio::test]
async fn lookup_resources() {
    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let token = c
        .write_relationships(vec![
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "doc-lr-1").unwrap(),
                "viewer",
                SubjectReference::new(
                    ObjectReference::new("user", "dave").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            )),
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "doc-lr-2").unwrap(),
                "editor",
                SubjectReference::new(
                    ObjectReference::new("user", "dave").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            )),
        ])
        .await
        .unwrap();

    let subject = SubjectReference::new(
        ObjectReference::new("user", "dave").unwrap(),
        None::<String>,
    )
    .unwrap();

    let mut stream = c
        .lookup_resources("document", "view", &subject)
        .consistency(Consistency::AtLeastAsFresh(token))
        .send()
        .await
        .expect("lookup_resources failed");

    let mut resource_ids = vec![];
    while let Some(result) = stream.next().await {
        let item = result.expect("stream item error");
        resource_ids.push(item.resource_id);
    }
    resource_ids.sort();
    assert!(resource_ids.contains(&"doc-lr-1".to_string()));
    assert!(resource_ids.contains(&"doc-lr-2".to_string()));
}

#[tokio::test]
async fn lookup_subjects() {
    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let token = c
        .write_relationships(vec![
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "doc-ls-1").unwrap(),
                "viewer",
                SubjectReference::new(
                    ObjectReference::new("user", "eve").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            )),
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "doc-ls-1").unwrap(),
                "viewer",
                SubjectReference::new(
                    ObjectReference::new("user", "frank").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            )),
        ])
        .await
        .unwrap();

    let resource = ObjectReference::new("document", "doc-ls-1").unwrap();
    let mut stream = c
        .lookup_subjects(&resource, "view", "user")
        .consistency(Consistency::AtLeastAsFresh(token))
        .send()
        .await
        .expect("lookup_subjects failed");

    let mut subject_ids = vec![];
    while let Some(result) = stream.next().await {
        let item = result.expect("stream item error");
        subject_ids.push(item.subject_id);
    }
    subject_ids.sort();
    assert!(subject_ids.contains(&"eve".to_string()));
    assert!(subject_ids.contains(&"frank".to_string()));
}

#[tokio::test]
async fn delete_relationships() {
    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "doc-del-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "grace").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ))])
        .await
        .unwrap();

    // Verify relationship exists
    let result = c
        .check_permission(
            &ObjectReference::new("document", "doc-del-1").unwrap(),
            "view",
            &SubjectReference::new(
                ObjectReference::new("user", "grace").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .consistency(Consistency::AtLeastAsFresh(token))
        .await
        .unwrap();
    assert!(result.is_allowed().unwrap());

    // Delete and verify
    let del_token = c
        .delete_relationships(
            RelationshipFilter::new("document")
                .resource_id("doc-del-1")
                .relation("viewer"),
        )
        .await
        .unwrap();

    let result = c
        .check_permission(
            &ObjectReference::new("document", "doc-del-1").unwrap(),
            "view",
            &SubjectReference::new(
                ObjectReference::new("user", "grace").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .consistency(Consistency::AtLeastAsFresh(del_token))
        .await
        .unwrap();
    assert!(!result.is_allowed().unwrap());
}

// ── Watch ─────────────────────────────────────────────────────

#[cfg(feature = "watch")]
#[tokio::test]
async fn watch_receives_updates() {
    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let mut stream = c.watch(vec!["document"]).send().await.expect("watch failed");

    // Write a relationship to trigger an event
    let c2 = c.clone();
    let write_handle = tokio::spawn(async move {
        // Small delay to ensure watch is established
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        c2.write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "doc-watch-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "hal").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ))])
        .await
        .unwrap();
    });

    // Should receive at least one event within a reasonable timeout
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for watch event")
        .expect("stream ended")
        .expect("watch event error");

    assert!(!event.updates.is_empty());
    write_handle.await.unwrap();
}

// ── Bulk (experimental) ───────────────────────────────────────

#[cfg(feature = "experimental")]
#[tokio::test]
async fn bulk_check_permissions() {
    use prescience::BulkCheckItem;

    let c = client().await;
    c.write_schema(TEST_SCHEMA).await.unwrap();

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "doc-bulk-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "iris").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ))])
        .await
        .unwrap();

    let results = c
        .bulk_check_permissions(vec![
            BulkCheckItem::new(
                ObjectReference::new("document", "doc-bulk-1").unwrap(),
                "view",
                SubjectReference::new(
                    ObjectReference::new("user", "iris").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ),
            BulkCheckItem::new(
                ObjectReference::new("document", "doc-bulk-1").unwrap(),
                "edit",
                SubjectReference::new(
                    ObjectReference::new("user", "iris").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ),
        ])
        .consistency(Consistency::AtLeastAsFresh(token))
        .await
        .expect("bulk_check failed");

    assert_eq!(results.len(), 2);
    // First: viewer -> can view
    assert!(results[0].as_ref().unwrap().is_allowed().unwrap());
    // Second: not editor -> cannot edit
    assert!(!results[1].as_ref().unwrap().is_allowed().unwrap());
}
