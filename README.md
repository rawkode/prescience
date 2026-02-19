# Prescience

An idiomatic Rust client library for [SpiceDB](https://authzed.com/spicedb), the open-source, Google Zanzibar-inspired authorization system.

## Features

- **Type-safe**: Wraps the SpiceDB gRPC API with idiomatic Rust types — no raw protobufs
- **Async-first**: Built on [tonic](https://github.com/hyperium/tonic) and tokio
- **3-state permissions**: `PermissionResult` correctly models `Allowed`, `Denied`, and `Conditional` (caveated) outcomes
- **Streaming**: All streaming RPCs return `impl Stream<Item = Result<T, Error>>`
- **Shareable**: `Client` is `Clone + Send + Sync` — clone it freely across tasks
- **Feature-gated**: `watch`, `experimental`, `serde`, TLS backends

## Quick Start

```rust
use prescience::{Client, ObjectReference, SubjectReference, Consistency, PermissionResult};

#[tokio::main]
async fn main() -> Result<(), prescience::Error> {
    let client = Client::new("http://localhost:50051", "my-token").await?;

    let result = client
        .check_permission(
            &ObjectReference::new("document", "doc-123")?,
            "view",
            &SubjectReference::new(ObjectReference::new("user", "alice")?, None::<String>)?,
        )
        .consistency(Consistency::FullyConsistent)
        .await?;

    match result {
        PermissionResult::Allowed => println!("access granted"),
        PermissionResult::Denied => println!("access denied"),
        PermissionResult::Conditional { missing_fields } => {
            println!("need caveat context: {:?}", missing_fields);
        }
    }

    Ok(())
}
```

## Writing Relationships

```rust
use prescience::{Client, ObjectReference, SubjectReference, Relationship, RelationshipUpdate};

# async fn example(client: &Client) -> Result<(), prescience::Error> {
let token = client
    .write_relationships(vec![
        RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "doc-123")?,
            "viewer",
            SubjectReference::new(ObjectReference::new("user", "bob")?, None::<String>)?,
        )),
    ])
    .await?;

// Use the token for subsequent reads
let result = client
    .check_permission(
        &ObjectReference::new("document", "doc-123")?,
        "view",
        &SubjectReference::new(ObjectReference::new("user", "bob")?, None::<String>)?,
    )
    .consistency(prescience::Consistency::AtLeastAsFresh(token))
    .await?;
# Ok(())
# }
```

## Streaming Lookups

```rust
use tokio_stream::StreamExt;

# async fn example(client: &prescience::Client) -> Result<(), prescience::Error> {
let subject = prescience::SubjectReference::new(
    prescience::ObjectReference::new("user", "alice")?,
    None::<String>,
)?;

let mut stream = client
    .lookup_resources("document", "view", &subject)
    .consistency(prescience::Consistency::FullyConsistent)
    .send()
    .await?;

while let Some(result) = stream.next().await {
    let item = result?;
    println!("resource: {}, permission: {:?}", item.resource_id, item.permission);
}
# Ok(())
# }
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `watch` | No | WatchService for streaming relationship changes |
| `experimental` | No | Bulk APIs: BulkCheckPermission, BulkImport/Export |
| `serde` | No | Serialize/Deserialize on ZedToken and domain types |
| `tls-rustls` | No | Use rustls for TLS |
| `tls-native` | No | Use native/system TLS |

## Error Handling

All methods return `Result<T, prescience::Error>`. The error type provides:

- **Structured matching**: `Error::Transport`, `Error::Status`, `Error::InvalidArgument`, etc.
- **Retryability**: `error.is_retryable()` returns `true` for `UNAVAILABLE` and `DEADLINE_EXCEEDED`
- **gRPC code access**: `error.code()` returns the gRPC status code

## Development

This project uses [devenv](https://devenv.sh) for a reproducible development environment:

```bash
# Enter the dev shell (provides Rust, protoc, SpiceDB)
devenv shell

# Build
cargo build --all-features

# Test
cargo test --all-features

# Lint
cargo clippy --all-features -- -D warnings

# Start a local SpiceDB for integration testing
spicedb serve --grpc-preshared-key "test-key" --datastore-engine memory
```

## License

Apache-2.0
