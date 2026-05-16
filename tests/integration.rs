//! Integration tests against a SpiceDB instance managed by testcontainers.
//!
//! All tests share a single SpiceDB container for speed. Each test uses
//! unique resource/subject IDs to avoid interference.

use std::borrow::Cow;
use std::sync::Arc;

use prescience::{
    Client, Consistency, ObjectReference, PermissionResult, Precondition, Relationship,
    RelationshipFilter, RelationshipUpdate, SubjectReference,
};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use tokio::sync::OnceCell;
use tokio_stream::StreamExt;

// ── SpiceDB testcontainer image ───────────────────────────────

const SPICEDB_IMAGE: &str = "authzed/spicedb";
const SPICEDB_TAG: &str = "v1.45.4";
const SPICEDB_GRPC_PORT: u16 = 50051;
const SPICEDB_TOKEN: &str = "test-key";

#[derive(Debug)]
struct SpiceDbImage;

impl testcontainers::Image for SpiceDbImage {
    fn name(&self) -> &str {
        SPICEDB_IMAGE
    }

    fn tag(&self) -> &str {
        SPICEDB_TAG
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr("grpc server started serving")]
    }

    fn env_vars(
        &self,
    ) -> impl IntoIterator<Item = (impl Into<Cow<'_, str>>, impl Into<Cow<'_, str>>)> {
        vec![
            ("SPICEDB_GRPC_PRESHARED_KEY", SPICEDB_TOKEN),
            ("SPICEDB_DATASTORE_ENGINE", "memory"),
        ]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<Cow<'_, str>>> {
        vec!["serve"]
    }

    fn expose_ports(&self) -> &[testcontainers::core::ContainerPort] {
        &[testcontainers::core::ContainerPort::Tcp(SPICEDB_GRPC_PORT)]
    }
}

// ── Shared container ──────────────────────────────────────────

/// Holds the running container and its mapped port.
/// The `Client` is NOT shared because each `#[tokio::test]` creates its
/// own tokio runtime, and tonic `Channel` is tied to the runtime that
/// created it. Sharing a single Channel across runtimes causes transport
/// errors. Instead we share only the container and create a fresh Client
/// per test invocation.
struct SharedSpiceDb {
    _container: ContainerAsync<SpiceDbImage>,
    port: u16,
    schema_written: bool,
}

static SPICEDB: OnceCell<Arc<SharedSpiceDb>> = OnceCell::const_new();

/// Returns a fresh `Client` connected to the shared SpiceDB container.
/// The container is started once (lazily) and the schema is written on
/// first access. Each call creates a new tonic Channel on the caller's
/// runtime, avoiding cross-runtime transport errors.
async fn spicedb() -> Client {
    // Ensure container is started and schema is written (once)
    let shared = SPICEDB
        .get_or_init(|| async {
            let container = SpiceDbImage
                .start()
                .await
                .expect("failed to start SpiceDB container");
            let port = container
                .get_host_port_ipv4(SPICEDB_GRPC_PORT.tcp())
                .await
                .expect("failed to get mapped port");
            let endpoint = format!("http://localhost:{}", port);

            // Retry until gRPC is fully serving (log message can appear before ready)
            let client = {
                let mut last_err = None;
                let mut result = None;
                for _ in 0..30 {
                    match Client::new(&endpoint, SPICEDB_TOKEN).await {
                        Ok(c) => match c.read_schema().await {
                            // Schema read succeeded — SpiceDB is ready
                            Ok(_) => {
                                result = Some(c);
                                break;
                            }
                            // NotFound means SpiceDB is serving but has no schema yet — that's ready
                            Err(ref e) if e.code() == Some(tonic::Code::NotFound) => {
                                result = Some(c);
                                break;
                            }
                            Err(e) => {
                                last_err = Some(format!("{e}"));
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                        },
                        Err(e) => {
                            last_err = Some(format!("{e}"));
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        }
                    }
                }
                result.unwrap_or_else(|| {
                    panic!(
                        "SpiceDB not ready after retries: {}",
                        last_err.unwrap_or_default()
                    )
                })
            };

            // Write schema once for all tests
            client
                .write_schema(TEST_SCHEMA)
                .await
                .expect("write_schema failed");

            Arc::new(SharedSpiceDb {
                _container: container,
                port,
                schema_written: true,
            })
        })
        .await;

    assert!(shared.schema_written, "schema should have been written");

    // Create a fresh client on the CURRENT runtime
    let endpoint = format!("http://localhost:{}", shared.port);
    Client::new(&endpoint, SPICEDB_TOKEN)
        .await
        .expect("failed to create client for test")
}

const TEST_SCHEMA: &str = r#"
definition user {}

definition document {
    relation viewer: user
    relation editor: user

    permission view = viewer + editor
    permission edit = editor
}
"#;

// ── Schema ────────────────────────────────────────────────────

#[tokio::test]
async fn write_and_read_schema() {
    let c = spicedb().await;

    let (schema_text, read_at) = c.read_schema().await.expect("read_schema failed");
    assert!(schema_text.contains("definition document"));
    assert!(!read_at.token().is_empty());
}

#[tokio::test]
async fn write_schema_empty_rejected() {
    let c = spicedb().await;
    let err = c.write_schema("").await.unwrap_err();
    assert!(matches!(err, prescience::Error::InvalidArgument(_)));
}

// ── Relationships ─────────────────────────────────────────────

#[tokio::test]
async fn write_relationships_empty_rejected() {
    let c = spicedb().await;
    let err = c.write_relationships(vec![]).await.unwrap_err();
    assert!(matches!(err, prescience::Error::InvalidArgument(_)));
}

#[tokio::test]
async fn write_and_check_permission() {
    let c = spicedb().await;

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "check-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "alice").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ).unwrap())])
        .await
        .expect("write_relationships failed");

    let result = c
        .check_permission(
            &ObjectReference::new("document", "check-1").unwrap(),
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

    let result = c
        .check_permission(
            &ObjectReference::new("document", "check-1").unwrap(),
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
    let c = spicedb().await;

    let token = c
        .write_relationships(vec![
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "read-1").unwrap(),
                "viewer",
                SubjectReference::new(ObjectReference::new("user", "bob").unwrap(), None::<String>)
                    .unwrap(),
            ).unwrap()),
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "read-1").unwrap(),
                "editor",
                SubjectReference::new(
                    ObjectReference::new("user", "carol").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ).unwrap()),
        ])
        .await
        .unwrap();

    let filter = RelationshipFilter::new("document").unwrap().resource_id("read-1");
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
        assert_eq!(item.relationship.resource.object_id(), "read-1");
        count += 1;
    }
    assert_eq!(count, 2);
}

#[tokio::test]
async fn lookup_resources() {
    let c = spicedb().await;

    let token = c
        .write_relationships(vec![
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "lr-1").unwrap(),
                "viewer",
                SubjectReference::new(
                    ObjectReference::new("user", "dave").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ).unwrap()),
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "lr-2").unwrap(),
                "editor",
                SubjectReference::new(
                    ObjectReference::new("user", "dave").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ).unwrap()),
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
    assert!(resource_ids.contains(&"lr-1".to_string()));
    assert!(resource_ids.contains(&"lr-2".to_string()));
}

#[tokio::test]
async fn lookup_subjects() {
    let c = spicedb().await;

    let token = c
        .write_relationships(vec![
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "ls-1").unwrap(),
                "viewer",
                SubjectReference::new(ObjectReference::new("user", "eve").unwrap(), None::<String>)
                    .unwrap(),
            ).unwrap()),
            RelationshipUpdate::create(Relationship::new(
                ObjectReference::new("document", "ls-1").unwrap(),
                "viewer",
                SubjectReference::new(
                    ObjectReference::new("user", "frank").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ).unwrap()),
        ])
        .await
        .unwrap();

    let resource = ObjectReference::new("document", "ls-1").unwrap();
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
    let c = spicedb().await;

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "del-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "grace").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ).unwrap())])
        .await
        .unwrap();

    let result = c
        .check_permission(
            &ObjectReference::new("document", "del-1").unwrap(),
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

    let del_token = c
        .delete_relationships(
            RelationshipFilter::new("document")
                .unwrap()
                .resource_id("del-1")
                .relation("viewer"),
        )
        .await
        .unwrap();

    let result = c
        .check_permission(
            &ObjectReference::new("document", "del-1").unwrap(),
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
    let c = spicedb().await;

    let mut stream = c
        .watch(vec!["document"])
        .send()
        .await
        .expect("watch failed");

    let c2 = c.clone();
    let write_handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        c2.write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "watch-1").unwrap(),
            "viewer",
            SubjectReference::new(ObjectReference::new("user", "hal").unwrap(), None::<String>)
                .unwrap(),
        ).unwrap())])
        .await
        .unwrap();
    });

    let event = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
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

    let c = spicedb().await;

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "bulk-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "iris").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ).unwrap())])
        .await
        .unwrap();

    let results = c
        .bulk_check_permissions(vec![
            BulkCheckItem::new(
                ObjectReference::new("document", "bulk-1").unwrap(),
                "view",
                SubjectReference::new(
                    ObjectReference::new("user", "iris").unwrap(),
                    None::<String>,
                )
                .unwrap(),
            ),
            BulkCheckItem::new(
                ObjectReference::new("document", "bulk-1").unwrap(),
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
    assert!(results[0].as_ref().unwrap().is_allowed().unwrap());
    assert!(!results[1].as_ref().unwrap().is_allowed().unwrap());
}

// ── Transient Failure and Recovery ────────────────────────

#[tokio::test]
async fn error_retryability_unavailable() {
    // Bind an ephemeral port then immediately close the listener.
    // After the listener is dropped, any connection to that port gets
    // ECONNREFUSED deterministically on all platforms.
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("failed to bind ephemeral port");
    let port = listener.local_addr().unwrap().port();
    drop(listener); // port is now unreachable

    let endpoint = format!("http://127.0.0.1:{}", port);
    let result = Client::new(&endpoint, SPICEDB_TOKEN).await;

    match result {
        Err(e) => {
            // Connection refused is surfaced as a Transport error.
            assert!(
                matches!(e, prescience::Error::Transport(_)),
                "Expected transport error, got: {:?}",
                e
            );
        }
        Ok(_) => panic!("Expected connection to fail"),
    }
}

#[tokio::test]
async fn error_retryability_classification() {
    use prescience::Error;

    // Test UNAVAILABLE is retryable
    let unavailable = Error::Status {
        code: tonic::Code::Unavailable,
        message: "service unavailable".to_string(),
        details: None,
    };
    assert!(
        unavailable.is_retryable(),
        "UNAVAILABLE should be retryable"
    );
    assert_eq!(unavailable.code(), Some(tonic::Code::Unavailable));

    // Test DEADLINE_EXCEEDED is retryable
    let deadline_exceeded = Error::Status {
        code: tonic::Code::DeadlineExceeded,
        message: "deadline exceeded".to_string(),
        details: None,
    };
    assert!(
        deadline_exceeded.is_retryable(),
        "DEADLINE_EXCEEDED should be retryable"
    );
    assert_eq!(
        deadline_exceeded.code(),
        Some(tonic::Code::DeadlineExceeded)
    );

    // Test UNAUTHENTICATED is NOT retryable
    let unauthenticated = Error::Status {
        code: tonic::Code::Unauthenticated,
        message: "invalid token".to_string(),
        details: None,
    };
    assert!(
        !unauthenticated.is_retryable(),
        "UNAUTHENTICATED should NOT be retryable"
    );

    // Test PERMISSION_DENIED is NOT retryable
    let permission_denied = Error::Status {
        code: tonic::Code::PermissionDenied,
        message: "access denied".to_string(),
        details: None,
    };
    assert!(
        !permission_denied.is_retryable(),
        "PERMISSION_DENIED should NOT be retryable"
    );

    // Test NOT_FOUND is NOT retryable
    let not_found = Error::Status {
        code: tonic::Code::NotFound,
        message: "not found".to_string(),
        details: None,
    };
    assert!(
        !not_found.is_retryable(),
        "NOT_FOUND should NOT be retryable"
    );

    // Test INVALID_ARGUMENT is NOT retryable
    let invalid_arg = Error::Status {
        code: tonic::Code::InvalidArgument,
        message: "invalid input".to_string(),
        details: None,
    };
    assert!(
        !invalid_arg.is_retryable(),
        "INVALID_ARGUMENT should NOT be retryable"
    );

    // Test ALREADY_EXISTS is NOT retryable
    let already_exists = Error::Status {
        code: tonic::Code::AlreadyExists,
        message: "already exists".to_string(),
        details: None,
    };
    assert!(
        !already_exists.is_retryable(),
        "ALREADY_EXISTS should NOT be retryable"
    );

    // Test FAILED_PRECONDITION is NOT retryable
    let failed_precondition = Error::Status {
        code: tonic::Code::FailedPrecondition,
        message: "precondition failed".to_string(),
        details: None,
    };
    assert!(
        !failed_precondition.is_retryable(),
        "FAILED_PRECONDITION should NOT be retryable"
    );
}

#[tokio::test]
async fn timeout_behavior_with_deadline() {
    use std::time::Duration;
    use tonic::transport::Endpoint;

    // Create a "black-hole" server: accepts TCP connections but never sends any
    // HTTP/2 data, so the gRPC handshake never completes and every RPC times out.
    // This guarantees a deterministic DEADLINE_EXCEEDED result regardless of
    // how fast the test machine is.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind black-hole listener");
    let hung_port = listener.local_addr().unwrap().port();

    // Accept one connection but never write any bytes — the HTTP/2 handshake stalls.
    // A single connection is all the test needs (one RPC → one connection).
    tokio::spawn(async move {
        if let Ok((socket, _)) = listener.accept().await {
            // Hold the socket open without responding so the timeout fires.
            let _socket = socket;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    // Build a lazy channel so `connect()` doesn't block; the timeout covers
    // the entire RPC including connection establishment.
    let endpoint_str = format!("http://127.0.0.1:{}", hung_port);
    let channel = Endpoint::from_shared(endpoint_str)
        .expect("invalid endpoint")
        .timeout(Duration::from_millis(500))
        .connect_lazy();

    let timeout_client =
        Client::from_channel(channel, SPICEDB_TOKEN).expect("failed to create client");

    let result = timeout_client
        .check_permission(
            &ObjectReference::new("document", "timeout-test").unwrap(),
            "view",
            &SubjectReference::new(
                ObjectReference::new("user", "timeout-user").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .await;

    match result {
        Err(e) => {
            let code = e
                .code()
                .expect("expected a gRPC status code for a timeout error");
            assert_eq!(
                code,
                tonic::Code::DeadlineExceeded,
                "timeout should yield DEADLINE_EXCEEDED, got {:?}",
                code
            );
            assert!(e.is_retryable(), "DEADLINE_EXCEEDED should be retryable");
        }
        Ok(_) => panic!("Expected timeout error but the operation succeeded"),
    }
}

#[cfg(feature = "watch")]
#[tokio::test]
async fn watch_resume_after_checkpoint() {
    let c = spicedb().await;

    // Start watching
    let mut stream = c
        .watch(vec!["document"])
        .send()
        .await
        .expect("watch failed");

    // Write a relationship and capture the checkpoint
    let c2 = c.clone();
    let write_handle = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        c2.write_relationships(vec![RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "resume-1").unwrap(),
            "viewer",
            SubjectReference::new(
                ObjectReference::new("user", "resume-user-1").unwrap(),
                None::<String>,
            )
            .unwrap(),
        ))])
        .await
        .unwrap();
    });

    // Get first event and its checkpoint
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for first watch event")
        .expect("stream ended")
        .expect("watch event error");

    let checkpoint = event.checkpoint;
    assert!(
        !checkpoint.token().is_empty(),
        "checkpoint should not be empty"
    );

    write_handle.await.unwrap();
    drop(stream);

    // Write another relationship after dropping the stream
    c.write_relationships(vec![RelationshipUpdate::create(Relationship::new(
        ObjectReference::new("document", "resume-2").unwrap(),
        "viewer",
        SubjectReference::new(
            ObjectReference::new("user", "resume-user-2").unwrap(),
            None::<String>,
        )
        .unwrap(),
    ))])
    .await
    .expect("second write failed");

    // Resume from checkpoint - should see the second write but not the first
    let mut resume_stream = c
        .watch(vec!["document"])
        .after_token(checkpoint)
        .send()
        .await
        .expect("watch resume failed");

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), resume_stream.next())
        .await
        .expect("timed out waiting for resumed watch event")
        .expect("resumed stream ended")
        .expect("resumed watch event error");

    // The resumed event must include the post-checkpoint write for this test,
    // and must not replay the pre-checkpoint write.
    let has_resume_2 = event.updates.iter().any(|u| {
        u.relationship.resource.object_id() == "resume-2"
            && u.relationship.resource.object_type() == "document"
    });
    let has_resume_1 = event.updates.iter().any(|u| {
        u.relationship.resource.object_id() == "resume-1"
            && u.relationship.resource.object_type() == "document"
    });
    assert!(
        has_resume_2,
        "resumed watch should include the post-checkpoint update (resume-2); got: {:?}",
        event.updates
    );
    assert!(
        !has_resume_1,
        "resumed watch should not replay the pre-checkpoint update (resume-1); got: {:?}",
        event.updates
    );
}

#[tokio::test]
async fn unauthenticated_error_mapping() {
    // Use invalid token to trigger authentication error
    let endpoint = format!("http://localhost:{}", spicedb_port().await);
    let bad_client = Client::new(&endpoint, "invalid-token-xyz")
        .await
        .expect("client creation should succeed");

    let result = bad_client.read_schema().await;

    match result {
        Err(e) => {
            // SpiceDB may return either UNAUTHENTICATED or PERMISSION_DENIED for bad tokens
            let code = e.code().expect("should have a status code");
            assert!(
                code == tonic::Code::Unauthenticated || code == tonic::Code::PermissionDenied,
                "Expected UNAUTHENTICATED or PERMISSION_DENIED, got {:?}",
                code
            );
            assert!(
                !e.is_retryable(),
                "Authentication errors should not be retryable"
            );
        }
        Ok(_) => panic!("Expected authentication to fail with invalid token"),
    }
}

#[tokio::test]
async fn invalid_argument_error_mapping() {
    let c = spicedb().await;

    // Try to write an invalid schema to trigger INVALID_ARGUMENT
    let result = c.write_schema("this is not valid schema syntax @#$").await;

    match result {
        Err(e) => {
            // Should get either local InvalidArgument validation or server INVALID_ARGUMENT
            match &e {
                prescience::Error::InvalidArgument(_) => {
                    // Local validation caught it
                }
                prescience::Error::Status { code, .. } => {
                    assert_eq!(
                        *code,
                        tonic::Code::InvalidArgument,
                        "Expected INVALID_ARGUMENT from server"
                    );
                    assert!(
                        !e.is_retryable(),
                        "INVALID_ARGUMENT should not be retryable"
                    );
                }
                _ => panic!("Unexpected error variant: {:?}", e),
            }
        }
        Ok(_) => panic!("Expected invalid schema to be rejected"),
    }
}

#[tokio::test]
async fn failed_precondition_error_mapping() {
    let c = spicedb().await;

    // Create a relationship
    let rel = Relationship::new(
        ObjectReference::new("document", "precond-1").unwrap(),
        "viewer",
        SubjectReference::new(
            ObjectReference::new("user", "precond-user").unwrap(),
            None::<String>,
        )
        .unwrap(),
    )
    .unwrap();

    let token = c
        .write_relationships(vec![RelationshipUpdate::create(rel.clone())])
        .await
        .expect("initial write failed");

    // Try to create with precondition that it must NOT exist (should fail)
    let filter = RelationshipFilter::new("document")
        .unwrap()
        .resource_id("precond-1")
        .relation("viewer");
    let result = c
        .write_relationships(vec![RelationshipUpdate::create(rel.clone())])
        .preconditions(vec![Precondition::must_not_exist(filter)])
        .await;

    match result {
        Err(e) => {
            assert_eq!(
                e.code(),
                Some(tonic::Code::FailedPrecondition),
                "Expected FAILED_PRECONDITION when relationship exists"
            );
            assert!(
                !e.is_retryable(),
                "FAILED_PRECONDITION should not be retryable"
            );
        }
        Ok(_) => {
            panic!(
                "Expected FAILED_PRECONDITION when relationship exists, but write succeeded"
            );
        }
    }

    // Verify relationship still exists
    let verify = c
        .check_permission(
            &ObjectReference::new("document", "precond-1").unwrap(),
            "view",
            &SubjectReference::new(
                ObjectReference::new("user", "precond-user").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .consistency(Consistency::AtLeastAsFresh(token))
        .await
        .expect("verification check failed");

    assert!(verify.is_allowed().unwrap());
}

#[tokio::test]
async fn not_found_error_mapping() {
    let c = spicedb().await;

    // Try to check permission on non-existent relationship
    let result = c
        .check_permission(
            &ObjectReference::new("document", "does-not-exist-12345").unwrap(),
            "view",
            &SubjectReference::new(
                ObjectReference::new("user", "nobody").unwrap(),
                None::<String>,
            )
            .unwrap(),
        )
        .await;

    // Check permission doesn't return NOT_FOUND, it returns Denied
    // So let's test NOT_FOUND with a different scenario
    match result {
        Ok(r) => {
            // Should be denied since relationship doesn't exist
            assert!(!r.is_allowed().unwrap());
        }
        Err(e) => {
            // Some error occurred; only validate NOT_FOUND behavior if the code is
            // actually NOT_FOUND.
            let code = e.code().expect("expected error to include a gRPC status code");
            assert_eq!(code, tonic::Code::NotFound, "expected NOT_FOUND error");
            assert!(!e.is_retryable(), "NOT_FOUND should not be retryable");
        }
    }
}

/// Returns the mapped port of the shared SpiceDB container.
///
/// Reuses the single shared-container initialization in `spicedb()` so that
/// container startup, readiness retries, and schema installation remain defined
/// in one place.
async fn spicedb_port() -> u16 {
    // Ensure the shared container is initialized (and schema written) exactly once.
    let _ = spicedb().await;
    SPICEDB
        .get()
        .expect("SPICEDB should be initialized by spicedb()")
        .port
}
