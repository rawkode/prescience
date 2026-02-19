# 001 — Rust SpiceDB Client Library

**Date**: 2026-02-19
**Status**: Draft v4 (post-review rework round 3)
**Author**: Requirements interview
**Review**: Third rework pass. Round 1: all 4 REWORK (6 structural issues). Round 2: PO APPROVED, 3 REWORK (4 targeted issues). Round 3: PO+TS APPROVED, SD+DR REWORK (FR-9.2 trait blanket rule). v4 fixes FR-9.2 with per-type trait specifications.

---

## Problem Statement

Rust developers building backend services that use [SpiceDB](https://github.com/authzed/spicedb) for authorization currently lack a well-maintained, production-quality, idiomatic Rust client library. The existing community client ([lunaetco/spicedb-client](https://github.com/lunaetco/spicedb-client)) may be stale, incomplete, or insufficiently idiomatic. This project delivers a standalone Rust library crate (`prescience`) that wraps the full SpiceDB gRPC API with strong types, ergonomic builders, and first-class async support — giving Rust teams a dependency they can confidently ship to production.

---

## Users & Actors

| Actor | Description | Key Needs |
|---|---|---|
| **Rust backend developer** (primary) | Integrates SpiceDB permission checks into application services | Ergonomic API, strong typing, clear error handling, good docs |
| **Platform / DevOps engineer** (secondary) | Manages SpiceDB schemas and bulk-loads relationships | Schema read/write, bulk import/export, watch for changes |
| **SpiceDB server** (external system) | Exposes gRPC API (v1) on a configured endpoint | Stable protobuf contract defined in `authzed/api` |

No UI actors. No end-user-facing surface. This is a library consumed programmatically.

---

## Functional Requirements

### FR-1: Client Construction & Connection

1. **FR-1.1** — The library MUST provide a client constructor that accepts a SpiceDB endpoint (URI) and a bearer token.
2. **FR-1.2** — The constructor MUST support TLS-encrypted connections. TLS is determined by URI scheme: `https://` = TLS enabled, `http://` = plaintext. For advanced TLS configuration (custom CA certs, mTLS), use `Client::from_channel()` with a pre-configured `tonic::Channel`.
3. **FR-1.3** — The constructor MUST support plaintext/insecure connections (explicit opt-in) for local development. Attempting `http://` to a non-loopback address without explicit `.insecure(true)` on the builder MUST return `Err(Error::InvalidArgument(...))` with a message indicating that insecure connections to remote addresses require explicit opt-in. No warning-log-and-proceed behavior. (Rationale: fail-closed is safer. Consistent with the error-handling approach.)
4. **FR-1.4** — The client MUST reuse the underlying `tonic::Channel` across calls (connection pooling via channel reuse).
5. **FR-1.5** — The client SHOULD accept an externally-constructed `tonic::Channel` for advanced use cases (custom interceptors, load balancing, custom TLS configuration such as CA certs, client certificates, etc.).
6. **FR-1.6** — The bearer token MUST be attached to every outgoing gRPC request as an `authorization: Bearer <token>` metadata header.
7. **FR-1.7** — The `Client` MUST implement `Clone`, `Send`, and `Sync`. This is required for sharing across `tokio::spawn` tasks in real-world async services. Internally, `Client` uses `tonic::Channel` which is already `Clone + Send + Sync`.
8. **FR-1.8** — The `Client` MUST be usable when wrapped in `Arc<Client>` and shared across `tokio::spawn` tasks. Since `Client` is `Clone` (wrapping a `tonic::Channel` which is cheaply cloneable), `Arc` wrapping is not strictly necessary but MUST work correctly.
9. **FR-1.9** — All RPC methods SHOULD accept an optional per-request timeout/deadline. The builder MUST support a `.default_timeout(Duration)` that applies to all RPCs unless overridden per-request.

### FR-2: PermissionsService

10. **FR-2.1** — `CheckPermission` — Given a resource (type + id), a permission/relation name, a subject (type + id + optional relation), and an optional caveat context (`HashMap<String, ContextValue>`), return a `PermissionResult` indicating whether the subject has the permission. MUST support all consistency modes (see FR-6).

    > **Important**: SpiceDB returns a 3-state `Permissionship` enum: `HAS_PERMISSION`, `NO_PERMISSION`, `CONDITIONAL`. Returning `bool` would be lossy and a security hazard — a `CONDITIONAL` result silently coerced to `false` could deny legitimate access, and coerced to `true` could grant unauthorized access. The `PermissionResult` enum preserves all three states faithfully.

    A convenience method `.is_allowed() -> Result<bool, Error>` MUST be provided on `PermissionResult`. It MUST return `Ok(true)` for `Allowed`, `Ok(false)` for `Denied`, and `Err(Error::ConditionalPermission { missing_fields })` for `Conditional` — forcing the caller to handle the ambiguous case explicitly rather than silently dropping it.

11. **FR-2.2** — `LookupResources` — Given a resource type, permission, subject, and optional caveat context, stream back all resource IDs the subject can access. Returns a `Stream`. Each item includes the resource ID and its `PermissionResult` (which may be `Conditional` for caveated relationships).
12. **FR-2.3** — `LookupSubjects` — Given a resource, permission, subject type, and optional caveat context, stream back all subjects that have the permission. Returns a `Stream`.
13. **FR-2.4** — `ExpandPermissionTree` — Given a resource and permission, return the full permission tree. MUST return a structured `PermissionTree` type (recursive tree node), not raw proto.
14. **FR-2.5** — `ReadRelationships` — Given a relationship filter, stream back matching relationships. Returns a `Stream`. Each relationship includes its optional `Caveat` if one is attached.
15. **FR-2.6** — `WriteRelationships` — Accept a list of relationship updates (create/touch/delete) with optional preconditions (`Vec<Precondition>`). Relationships MAY include a `Caveat` (caveat name + context). Return a `ZedToken`.
16. **FR-2.7** — `DeleteRelationships` — Given a relationship filter, optional preconditions (`Vec<Precondition>`), and optional consistency, delete matching relationships. Return a `ZedToken`.

    > **Note on consistency for DeleteRelationships**: This is a mutating operation, but SpiceDB accepts a `Consistency` parameter on `DeleteRelationships` to control the snapshot at which the relationship filter is evaluated. The filter determines which relationships to delete; the consistency mode determines which snapshot is used to resolve that filter. The operation then returns a `ZedToken` representing the state after deletion. See the consistency matrix in FR-6 for the full picture.

### FR-3: SchemaService

17. **FR-3.1** — `ReadSchema` — Return the current SpiceDB schema as a `String`. Returns a `ZedToken` alongside the schema text.
18. **FR-3.2** — `WriteSchema` — Accept a schema string and apply it. Return a `ZedToken`.

### FR-4: WatchService

19. **FR-4.1** — `Watch` — Given optional object types to filter on, return a `Stream<Item = Result<WatchEvent, Error>>` of relationship update events. MUST handle long-lived streaming connections with the following explicit behavioral contracts:

    - **On server disconnect**: stream yields `Err(Error::Status { code: UNAVAILABLE, .. })` then terminates (`next()` returns `None`).
    - **On empty filter list**: watches all relationship types (SpiceDB default behavior). Stream blocks waiting for events; `next()` returns `None` only when the stream terminates.
    - **On server error mid-stream**: stream yields `Err` with the gRPC status mapped to the appropriate `Error` variant, then terminates.
    - **No auto-reconnect**: Watch does NOT auto-reconnect. This is the caller's responsibility, consistent with NG-6 (no built-in retry/circuit breaker). Callers can use the checkpoint `ZedToken` from the last `WatchEvent` to resume from where they left off.
    - **WatchEvent contents**: Each `WatchEvent` includes the list of relationship updates and a checkpoint `ZedToken` for caller-driven resume.
    - **On caller drop**: stream is dropped, underlying gRPC stream is cancelled (standard tonic behavior — the `Drop` impl on the tonic `Streaming` handle cancels the RPC).

### FR-5: ExperimentalService

> **Note**: All FR-5.x methods are gated behind `#[cfg(feature = "experimental")]`. See Cargo Feature Flags section.

20. **FR-5.1** — `BulkCheckPermission` — Accept a batch of check requests (each with optional caveat context) and return a `Vec<CheckResult>`. Each `CheckResult` contains either a `PermissionResult` or a per-item `Error`. Single round-trip.
21. **FR-5.2** — `BulkImportRelationships` — Accept an `impl Stream<Item = Relationship>` for client-streaming bulk import. Return an import count.
22. **FR-5.3** — `BulkExportRelationships` — Given a filter, stream back all matching relationships for export.

### FR-6: Consistency Semantics (ZedTokens)

23. **FR-6.1** — The library MUST model the four SpiceDB consistency modes as a Rust enum:
    - `FullyConsistent` — always check at the latest snapshot
    - `AtLeastAsFresh(ZedToken)` — at least as fresh as the given token
    - `AtExactSnapshot(ZedToken)` — exactly at the given token's snapshot
    - `MinimizeLatency` — server picks the fastest available snapshot
24. **FR-6.2** — Methods that accept a `Consistency` parameter and methods that return a `ZedToken` are detailed in the following matrix:

#### Consistency Matrix

| Method | Accepts `Consistency`? | Returns `ZedToken`? | Notes |
|---|---|---|---|
| FR-2.1 `CheckPermission` | ✅ Yes | ✅ Yes (in response) | Read path |
| FR-2.2 `LookupResources` | ✅ Yes | ✅ Yes (per item + final) | Streaming read |
| FR-2.3 `LookupSubjects` | ✅ Yes | ✅ Yes (per item + final) | Streaming read |
| FR-2.4 `ExpandPermissionTree` | ✅ Yes | ✅ Yes (in response) | Read path |
| FR-2.5 `ReadRelationships` | ✅ Yes | ✅ Yes (per item) | Streaming read |
| FR-2.6 `WriteRelationships` | ❌ No | ✅ Yes | Mutating — always writes at latest |
| FR-2.7 `DeleteRelationships` | ❌ No | ✅ Yes | Mutating — the SpiceDB proto does not accept a consistency parameter for deletes. |
| FR-3.1 `ReadSchema` | ❌ No | ✅ Yes | Schema reads don't accept consistency |
| FR-3.2 `WriteSchema` | ❌ No | ✅ Yes | Mutating |
| FR-4.1 `Watch` | N/A (uses `after_token`) | ✅ Yes (checkpoint per event) | Watch resumes from a token, not a consistency mode |
| FR-5.1 `BulkCheckPermission` | ✅ Yes | ✅ Yes (in response) | Experimental |
| FR-5.2 `BulkImportRelationships` | ❌ No | ❌ No (returns count) | Experimental, client-streaming |
| FR-5.3 `BulkExportRelationships` | ✅ Yes | ✅ Yes (per item) | Experimental, streaming read |

25. **FR-6.3** — Every mutating method (FR-2.6, FR-2.7, FR-3.2) MUST return a `ZedToken` that callers can store and pass to subsequent reads.
26. **FR-6.4** — When no consistency is specified on a method that accepts it, the library MUST send no consistency preference to the server. This means SpiceDB will apply its default consistency mode, which is `MinimizeLatency`. This behavior MUST be documented clearly in the crate-level docs and on each method that accepts consistency, so test authors know what to expect.

### FR-7: Error Handling

27. **FR-7.1** — The library MUST define a crate-level `Error` enum with the following concrete variants:

    ```rust
    #[derive(Debug)]
    pub enum Error {
        /// Connection-level failures: connection refused, DNS resolution failure, TLS handshake
        /// errors, channel closed.
        Transport(tonic::transport::Error),

        /// gRPC status errors returned by SpiceDB. Includes the status code, human-readable
        /// message, and optionally decoded SpiceDB-specific error details.
        Status {
            code: tonic::Code,
            message: String,
            details: Option<SpiceDbErrorDetails>,
        },

        /// Local validation failures before a request is sent. Examples: empty object_type,
        /// empty object_id, empty schema string, empty relationship update list.
        InvalidArgument(String),

        /// Protobuf encode/decode failures. Should be rare — indicates a bug or proto mismatch.
        Serialization(String),

        /// Returned by PermissionResult::is_allowed() when the result is Conditional.
        /// Forces callers to handle the caveated case explicitly.
        ConditionalPermission {
            missing_fields: Vec<String>,
        },
    }
    ```

28. **FR-7.2** — All public methods MUST return `Result<T, Error>`.
29. **FR-7.3** — The `Error` type MUST implement `std::error::Error`, `Debug`, `Display`, `Send`, `Sync`, and `'static`.
30. **FR-7.4** — The library SHOULD decode SpiceDB-specific error details from gRPC status metadata where available (e.g., `debug_information` from `ErrorReason`).
31. **FR-7.5** — The `Error` type MUST provide an `is_retryable(&self) -> bool` helper method. The following gRPC status codes are considered retryable: `UNAVAILABLE`, `DEADLINE_EXCEEDED`. All other status codes are non-retryable by default.
32. **FR-7.6** — The library MUST map gRPC status codes to semantically meaningful categories. The following mapping table MUST be documented in crate-level error docs:

    | gRPC Status Code | Semantic Meaning | Retryable? |
    |---|---|---|
    | `UNAUTHENTICATED` | Authentication failure — invalid or missing bearer token | No |
    | `PERMISSION_DENIED` | Authorization failure — token valid but insufficient permissions for this operation | No |
    | `NOT_FOUND` | Referenced resource or schema not found | No |
    | `FAILED_PRECONDITION` | Precondition on a write/delete was violated | No |
    | `INVALID_ARGUMENT` | Server rejected the request as malformed | No |
    | `ALREADY_EXISTS` | Attempted to create a relationship that already exists (when using `Create` not `Touch`) | No |
    | `UNAVAILABLE` | Server temporarily unavailable — safe to retry | Yes |
    | `DEADLINE_EXCEEDED` | Request timed out — may be safe to retry | Yes |

    > **Distinguishing authentication from authorization**: `UNAUTHENTICATED` means the caller's identity could not be established (bad token). `PERMISSION_DENIED` means the identity is known but lacks the required SpiceDB service-level permission. These are distinct failure modes and callers may need to handle them differently (e.g., refresh token vs. escalate to admin).

### FR-8: Type-Safe Domain Model

33. **FR-8.1** — The library MUST provide high-level Rust types for core domain concepts: `ObjectReference` (type + id), `SubjectReference` (object ref + optional relation), `Relationship`, `RelationshipUpdate`, `RelationshipFilter`, `SubjectFilter`, `Precondition`, `Permission`, `ZedToken`, `PermissionResult`, `PermissionTree`, `WatchEvent`, `CheckResult`, `Caveat`, `ContextValue`, `LookupResourceResult`, `LookupSubjectResult`, `ReadRelationshipResult`, `SpiceDbErrorDetails`.
34. **FR-8.2** — These types MUST NOT be raw protobuf-generated structs. They MUST be idiomatic Rust types with the generated protos as an internal implementation detail.
35. **FR-8.3** — Builder patterns or `From`/`Into` conversions SHOULD be provided for constructing complex request types.
36. **FR-8.4** — `ObjectReference` and `SubjectReference` MUST validate that `object_type` and `object_id` are non-empty at construction time.

### FR-9: Common Trait Derives

37. **FR-9.1** — All public **domain/value** types MUST derive `Debug` and `Clone`. The `Error` type is excluded from `Clone` since it wraps `tonic::transport::Error` which is not `Clone`.
38. **FR-9.2** — Equality and hash trait requirements are specified per-type based on field implementability, not as a blanket rule:
    - Public value types MUST derive `PartialEq` **unless** the type transitively contains `Error` (which wraps non-comparable transport internals).
    - Public value types MUST derive `Eq` and `Hash` **only when all fields support those traits**.
    - **`PartialEq + Eq + Hash`**: `ObjectReference`, `SubjectReference`, `SubjectFilter`, `RelationshipFilter`, `Precondition`, `PreconditionOp`, `Operation`, `ZedToken`, `Consistency`, `PermissionResult`, `LookupResourceResult`, `LookupSubjectResult`, `PermissionTree`, `PermissionTreeNode`, `SpiceDbErrorDetails`.
    - **`PartialEq` only** (no `Eq`/`Hash` — contains `f64` or `HashMap` transitively): `ContextValue`, `Caveat`, `Relationship`, `RelationshipUpdate`, `ReadRelationshipResult`, `WatchEvent`.
    - **No equality/hash traits**: `CheckResult` (contains `Error`).
39. **FR-9.3** — `ZedToken` MUST implement `Serialize` and `Deserialize` (via `serde`) for persistence (e.g., storing in a database or passing between services). This is gated behind the `serde` feature flag.
40. **FR-9.4** — `ZedToken` MUST redact its token value in `Debug` output for security (display as `ZedToken("***")` or similar). Token values may be sensitive and should not appear in application logs.
41. **FR-9.5** — `Error` MUST implement `Debug` and `Display` (via `std::error::Error` — see FR-7.3).

### FR-10: Boundary Conditions

42. **FR-10.1** — Passing an empty `Vec` to `write_relationships` MUST return `Err(Error::InvalidArgument("..."))` without making a network call.
43. **FR-10.2** — Passing an empty string to `write_schema` MUST return `Err(Error::InvalidArgument("..."))` without making a network call.
44. **FR-10.3** — Passing an empty filter list to `watch` MUST watch all relationship types (this is SpiceDB's default behavior, not an error).
45. **FR-10.4** — `ZedToken` construction MUST require a non-empty string. Attempting to construct a `ZedToken` from an empty string MUST return `Err(Error::InvalidArgument(...))`. No panicking. (Rationale: consistent with FR-8.4 which uses construction-time validation returning errors.)

---

## Non-Functional Requirements

| ID | Category | Requirement |
|---|---|---|
| **NFR-1** | **Idiomaticity** | Public API follows Rust API guidelines (C-COMMON-TRAITS, C-BUILDER, etc. per [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)). |
| **NFR-2** | **Async** | All network-calling methods are `async`. No blocking calls on async threads. |
| **NFR-3** | **Runtime** | Requires `tokio` runtime. No runtime-agnostic abstraction layer (see Non-Goals). |
| **NFR-4** | **Compilation** | Compiles on latest stable Rust. No nightly-only features. |
| **NFR-5** | **Dependencies** | Minimal dependency tree. Core deps: `tonic`, `prost`, `prost-types`, `tokio`, `tower` (for interceptors). Optional deps: `serde` (behind feature flag), `tracing` (for diagnostics). No further large frameworks. |
| **NFR-6** | **Documentation** | Every public type, method, and module has `///` doc comments. Crate-level docs include a getting-started example. Error docs include gRPC status code mapping table. |
| **NFR-7** | **Testing** | Unit tests for type construction/validation, boundary conditions, error variant matching. Integration tests against a real SpiceDB instance (via `testcontainers` or similar). CI runs both. |
| **NFR-8** | **Streaming** | Server-streaming RPCs return `impl Stream<Item = Result<T, Error>>` (via `tokio-stream` or `futures::Stream`). Streaming methods MUST NOT return boxed trait objects — use `impl Stream` or named stream types. |
| **NFR-9** | **Performance** | Channel reuse: no per-request connection overhead. Library itself adds negligible overhead above tonic/prost serialization. |
| **NFR-10** | **Semver** | Crate follows semver. Breaking API changes only on major version bumps. |
| **NFR-11** | **Concurrency** | `Client` is `Clone + Send + Sync` and safe to share across `tokio::spawn` tasks (see FR-1.7, FR-1.8). |

---

## Data Model

### Core Entities

```
┌──────────────────┐       ┌──────────────────────┐
│ ObjectReference   │       │ SubjectReference      │
│──────────────────│       │──────────────────────│
│ object_type: String       │ object: ObjectReference│
│ object_id: String│       │ optional_relation:     │
└──────────────────┘       │   Option<String>       │
        │                  └──────────────────────┘
        │                           │
        ▼                           ▼
┌────────────────────────────────────────┐
│ Relationship                           │
│────────────────────────────────────────│
│ resource: ObjectReference              │
│ relation: String                       │
│ subject: SubjectReference              │
│ optional_caveat: Option<Caveat>        │
└────────────────────────────────────────┘
        │
        ▼
┌────────────────────────────────────────┐
│ RelationshipUpdate                     │
│────────────────────────────────────────│
│ operation: Operation (Create|Touch|Del)│
│ relationship: Relationship             │
└────────────────────────────────────────┘

┌──────────────────┐
│ ZedToken         │
│──────────────────│
│ token: String    │  // Debug output: ZedToken("***")
│                  │  // Serialize/Deserialize behind `serde` feature
└──────────────────┘

┌──────────────────────────────────┐
│ Consistency                      │
│──────────────────────────────────│
│ FullyConsistent                  │
│ AtLeastAsFresh(ZedToken)         │
│ AtExactSnapshot(ZedToken)        │
│ MinimizeLatency                  │
└──────────────────────────────────┘

┌──────────────────────────────────┐
│ RelationshipFilter               │
│──────────────────────────────────│
│ resource_type: String            │
│ optional_resource_id: Option<Str>│
│ optional_relation: Option<String>│
│ optional_subject_filter:         │
│   Option<SubjectFilter>          │
└──────────────────────────────────┘
```

### Caveat & Context Types

```
┌──────────────────────────────────┐
│ Caveat                           │
│──────────────────────────────────│
│ name: String                     │  // Caveat name as defined in schema
│ context: HashMap<String,         │  // Key-value pairs for caveat evaluation
│          ContextValue>           │
└──────────────────────────────────┘

┌──────────────────────────────────┐
│ ContextValue                     │
│──────────────────────────────────│
│ Represents a typed value for     │
│ caveat context. Maps to          │
│ prost_types::Value internally.   │
│──────────────────────────────────│
│ Variants:                        │
│   Null                           │
│   Bool(bool)                     │
│   Number(f64)                    │
│   String(String)                 │
│   List(Vec<ContextValue>)        │
│   Struct(HashMap<String,         │
│          ContextValue>)          │
└──────────────────────────────────┘
```

### Permission Result Types

```
┌──────────────────────────────────┐
│ PermissionResult                 │
│──────────────────────────────────│
│ Allowed                          │  // Subject definitively has permission
│ Denied                           │  // Subject definitively lacks permission
│ Conditional {                    │  // Permission depends on unresolved caveat
│   missing_fields: Vec<String>    │  // Context fields needed to resolve
│ }                                │
│──────────────────────────────────│
│ Methods:                         │
│   is_allowed() -> Result<bool>   │  // Ok(true/false) or Err for Conditional
│   is_denied() -> bool            │  // true only for Denied
│   is_conditional() -> bool       │  // true only for Conditional
└──────────────────────────────────┘
```

### Filter & Precondition Types

```
┌──────────────────────────────────┐
│ SubjectFilter                    │
│──────────────────────────────────│
│ subject_type: String             │
│ optional_subject_id: Option<Str> │
│ optional_relation: Option<String>│
└──────────────────────────────────┘

┌──────────────────────────────────┐
│ Precondition                     │
│──────────────────────────────────│
│ operation: PreconditionOp        │  // MustExist | MustNotExist
│ filter: RelationshipFilter       │
└──────────────────────────────────┘

┌──────────────────────────────────┐
│ PreconditionOp                   │
│──────────────────────────────────│
│ MustExist                        │
│ MustNotExist                     │
└──────────────────────────────────┘
```

### Tree & Expansion Types

```
┌──────────────────────────────────────┐
│ PermissionTree                       │
│──────────────────────────────────────│
│ expanded_object: ObjectReference     │
│ expanded_relation: String            │
│ node: PermissionTreeNode             │
└──────────────────────────────────────┘

┌──────────────────────────────────────┐
│ PermissionTreeNode                   │
│──────────────────────────────────────│
│ Leaf {                               │
│   subjects: Vec<SubjectReference>    │
│ }                                    │
│ Union {                              │
│   children: Vec<PermissionTreeNode>  │
│ }                                    │
│ Intersection {                       │
│   children: Vec<PermissionTreeNode>  │
│ }                                    │
│ Exclusion {                          │
│   base: Box<PermissionTreeNode>,     │
│   excluded: Box<PermissionTreeNode>  │
│ }                                    │
└──────────────────────────────────────┘
```

### Watch & Streaming Types

```
┌──────────────────────────────────┐
│ WatchEvent                       │
│──────────────────────────────────│
│ updates: Vec<RelationshipUpdate> │  // Relationship changes in this event
│ checkpoint: ZedToken             │  // Token for caller-driven resume
└──────────────────────────────────┘
```

### Bulk Check Types

```
┌──────────────────────────────────┐
│ CheckResult                      │
│──────────────────────────────────│
│ Ok(PermissionResult)             │  // Successful check for this item
│ Err(Error)                       │  // Per-item error (e.g., invalid ref)
└──────────────────────────────────┘
```

> **Note**: `CheckResult` is semantically `Result<PermissionResult, Error>` but may be a type alias or newtype depending on ergonomics during implementation.

### Streaming Result Types

```
┌──────────────────────────────────────┐
│ LookupResourceResult                 │
│──────────────────────────────────────│
│ resource_id: String                  │
│ permission: PermissionResult         │
│ looked_up_at: ZedToken               │  // per-item token
└──────────────────────────────────────┘

┌──────────────────────────────────────┐
│ LookupSubjectResult                  │
│──────────────────────────────────────│
│ subject: SubjectReference            │
│ excluded_subjects: Vec<SubjectRef>   │
│ permission: PermissionResult         │
│ looked_up_at: ZedToken               │  // per-item token
└──────────────────────────────────────┘

┌──────────────────────────────────────┐
│ ReadRelationshipResult               │
│──────────────────────────────────────│
│ relationship: Relationship           │
│ read_at: ZedToken                    │  // per-item token
└──────────────────────────────────────┘
```

### Error Detail Types

```
┌──────────────────────────────────────┐
│ SpiceDbErrorDetails                  │
│──────────────────────────────────────│
│ error_reason: Option<String>         │  // SpiceDB ErrorReason enum value
│ debug_message: Option<String>        │  // Human-readable debug info from server
│ retry_info: Option<Duration>         │  // Suggested retry delay, if applicable
└──────────────────────────────────────┘
```

### Mapping to Protobuf

All domain types convert to/from their `authzed.api.v1` protobuf counterparts via `From`/`Into` implementations. The generated proto types are kept internal (`pub(crate)`) and never exposed in the public API. `ContextValue` maps to/from `prost_types::Value` (from the `google.protobuf.Struct` well-known type). Caveat context in requests maps to `prost_types::Struct`.

---

## API Contracts

The library exposes a single primary entry point: the `Client` struct. The `Client` internally wraps a `tonic::Channel` (which is `Clone + Send + Sync`) and a bearer token interceptor. Cloning a `Client` is cheap (channel is reference-counted internally).

### API Style Conventions

- **Unary RPCs** use direct `.await?` on a builder chain (e.g., `client.check_permission(...).consistency(...).await?`).
- **Streaming RPCs** use a `.send().await?` builder pattern that returns a `Stream` (e.g., `client.lookup_resources(...).consistency(...).send().await?`).
- **Streaming return types** are `impl Stream<Item = Result<T, Error>>` — not boxed trait objects.
- **Per-request timeout** can be applied via `.timeout(Duration)` on any request builder.

### Client Construction

```rust
// Minimal — http:// = plaintext (localhost only without .insecure(true))
let client = Client::new("http://localhost:50051", "my-token").await?;

// With options (TLS inferred from https:// scheme)
let client = Client::builder("https://spicedb.prod.internal:50051", "my-token")
    .connect_timeout(Duration::from_secs(5))
    .default_timeout(Duration::from_secs(10))  // applies to all RPCs unless overridden
    .build()
    .await?;

// From existing channel (supports custom TLS: CA certs, client certs, mTLS, etc.)
let client = Client::from_channel(channel, "my-token");

// Cloneable — cheap to share across tasks
let client2 = client.clone();
tokio::spawn(async move {
    client2.check_permission(/* ... */).await
});
```

### PermissionsService Methods

```rust
// Check permission — returns PermissionResult, not bool
let result: PermissionResult = client
    .check_permission(
        &ObjectReference::new("document", "doc-123")?,
        "view",
        &SubjectReference::new(ObjectReference::new("user", "alice")?, None)?,
    )
    .consistency(Consistency::FullyConsistent)
    .timeout(Duration::from_secs(5))  // per-request timeout override
    .await?;

match result {
    PermissionResult::Allowed => println!("access granted"),
    PermissionResult::Denied => println!("access denied"),
    PermissionResult::Conditional { missing_fields } => {
        println!("need caveat context: {:?}", missing_fields);
    }
}

// Convenience: panics on Conditional (use only when you know there are no caveats)
let allowed: bool = result.is_allowed()?;

// Check permission WITH caveat context
let mut context = HashMap::new();
context.insert("ip_address".to_string(), ContextValue::String("10.0.0.1".into()));
context.insert("time_of_day".to_string(), ContextValue::Number(14.0));

let result: PermissionResult = client
    .check_permission(
        &ObjectReference::new("document", "doc-123")?,
        "view",
        &SubjectReference::new(ObjectReference::new("user", "alice")?, None)?,
    )
    .context(context)
    .consistency(Consistency::FullyConsistent)
    .await?;

// Write relationships (with optional caveat on relationship)
let token: ZedToken = client
    .write_relationships(vec![
        RelationshipUpdate::create(Relationship::new(
            ObjectReference::new("document", "doc-123")?,
            "viewer",
            SubjectReference::new(ObjectReference::new("user", "bob")?, None)?,
        )),
        RelationshipUpdate::create(
            Relationship::new(
                ObjectReference::new("document", "doc-456")?,
                "viewer",
                SubjectReference::new(ObjectReference::new("user", "carol")?, None)?,
            ).with_caveat(Caveat::new("ip_allowlist", HashMap::from([
                ("allowed_ranges".to_string(), ContextValue::List(vec![
                    ContextValue::String("10.0.0.0/8".into()),
                ])),
            ]))),
        ),
    ])
    .await?;

// Write relationships with preconditions
let token: ZedToken = client
    .write_relationships(vec![
        RelationshipUpdate::create(/* ... */),
    ])
    .preconditions(vec![
        Precondition::must_exist(RelationshipFilter::new("document")
            .resource_id("doc-123")
            .relation("owner")),
    ])
    .await?;

// Lookup resources (streaming)
let mut stream = client
    .lookup_resources("document", "view", &subject)
    .consistency(Consistency::AtLeastAsFresh(token))
    .send()
    .await?;

while let Some(result) = stream.next().await {
    let lookup: LookupResourceResult = result?;
    println!("resource: {}, permission: {:?}", lookup.resource_id, lookup.permission);
}

// Read relationships (streaming)
let mut stream = client
    .read_relationships(RelationshipFilter::new("document"))
    .consistency(Consistency::FullyConsistent)
    .send()
    .await?;

while let Some(result) = stream.next().await {
    let item: ReadRelationshipResult = result?;
    println!("rel: {:?}, read_at: {:?}", item.relationship, item.read_at);
}

// Delete relationships with consistency (filter evaluated at specified snapshot)
let token: ZedToken = client
    .delete_relationships(RelationshipFilter::new("document").resource_id("doc-123"))
    .consistency(Consistency::AtLeastAsFresh(prev_token))
    .preconditions(vec![
        Precondition::must_exist(RelationshipFilter::new("document")
            .resource_id("doc-123")
            .relation("viewer")),
    ])
    .await?;

// Watch (long-lived stream)
let mut stream = client
    .watch(vec!["document", "user"])
    .after_token(token)
    .send()
    .await?;

while let Some(event) = stream.next().await {
    match event {
        Ok(watch_event) => {
            for update in &watch_event.updates {
                println!("change: {:?}", update);
            }
            // Store checkpoint for resume
            last_token = watch_event.checkpoint;
        }
        Err(Error::Status { code, .. }) if code == tonic::Code::Unavailable => {
            // Server disconnected — reconnect with last_token
            break;
        }
        Err(e) => {
            eprintln!("watch error: {}", e);
            break;
        }
    }
}
```

### SchemaService Methods

```rust
let (schema, token): (String, ZedToken) = client.read_schema().await?;
let token: ZedToken = client.write_schema("definition user {} ...").await?;
```

### ExperimentalService Methods

```rust
// Bulk check — requires `experimental` feature
let results: Vec<CheckResult> = client
    .bulk_check_permissions(vec![check1, check2, check3])
    .consistency(Consistency::FullyConsistent)
    .await?;

for result in &results {
    match result {
        CheckResult::Ok(permission) => println!("perm: {:?}", permission),
        CheckResult::Err(e) => eprintln!("item error: {}", e),
    }
}

// Bulk import (client-streaming) — accepts impl Stream<Item = Relationship>
let relationship_stream = futures::stream::iter(vec![
    Relationship::new(
        ObjectReference::new("document", "doc-1")?,
        "viewer",
        SubjectReference::new(ObjectReference::new("user", "alice")?, None)?,
    ),
    Relationship::new(
        ObjectReference::new("document", "doc-2")?,
        "viewer",
        SubjectReference::new(ObjectReference::new("user", "bob")?, None)?,
    ),
]);
let count: u64 = client
    .bulk_import_relationships(relationship_stream)
    .await?;

// Bulk export (server-streaming)
let mut stream = client
    .bulk_export_relationships(RelationshipFilter::new("document"))
    .consistency(Consistency::FullyConsistent)
    .send()
    .await?;
```

### Streaming Behavior Table

All streaming methods follow a consistent behavioral contract:

| Behavior | FR-2.2 LookupResources | FR-2.3 LookupSubjects | FR-2.5 ReadRelationships | FR-4.1 Watch | FR-5.3 BulkExport |
|---|---|---|---|---|---|
| **Return type** | `impl Stream<Item=Result<LookupResourceResult, Error>>` | `impl Stream<Item=Result<LookupSubjectResult, Error>>` | `impl Stream<Item=Result<ReadRelationshipResult, Error>>` | `impl Stream<Item=Result<WatchEvent, Error>>` | `impl Stream<Item=Result<Relationship, Error>>` |
| **Empty results** | Stream terminates immediately (`None`) | Stream terminates immediately (`None`) | Stream terminates immediately (`None`) | Blocks until events arrive | Stream terminates immediately (`None`) |
| **Server disconnect** | Yields `Err(Status{UNAVAILABLE})`, then `None` | Yields `Err(Status{UNAVAILABLE})`, then `None` | Yields `Err(Status{UNAVAILABLE})`, then `None` | Yields `Err(Status{UNAVAILABLE})`, then `None` | Yields `Err(Status{UNAVAILABLE})`, then `None` |
| **Server error mid-stream** | Yields `Err` with mapped status, then `None` | Yields `Err` with mapped status, then `None` | Yields `Err` with mapped status, then `None` | Yields `Err` with mapped status, then `None` | Yields `Err` with mapped status, then `None` |
| **Caller drops stream** | gRPC cancelled (tonic `Drop`) | gRPC cancelled (tonic `Drop`) | gRPC cancelled (tonic `Drop`) | gRPC cancelled (tonic `Drop`) | gRPC cancelled (tonic `Drop`) |
| **Auto-reconnect** | No | No | No | No (caller's responsibility) | No |
| **Long-lived?** | No (bounded) | No (bounded) | No (bounded) | Yes (indefinite) | No (bounded) |

> **Note**: The above is illustrative API design. Exact method signatures will be refined during technical specification. The key contract is: high-level Rust types in, high-level Rust types out, with `Result` and `Stream` where appropriate.

---

## Cargo Feature Flags

| Feature | Default? | Contents | Rationale |
|---|---|---|---|
| `default` | Yes | Core PermissionsService + SchemaService | Minimum viable client for authorization checks |
| `experimental` | No | ExperimentalService methods (FR-5.x): BulkCheckPermission, BulkImportRelationships, BulkExportRelationships | These APIs may change without notice in SpiceDB. Feature-gating signals instability to consumers. |
| `watch` | No | WatchService (FR-4.x) | Requires long-lived connections with different operational characteristics. Separated so consumers who don't need it don't pull in watch-related code. |
| `serde` | No | `Serialize`/`Deserialize` derives on `ZedToken` and other domain types | Optional dependency — many consumers won't need persistence. |
| `tls-rustls` | No | Use `rustls` as TLS backend (via tonic's `tls-rustls` feature) | Pure-Rust TLS — no system OpenSSL dependency. |
| `tls-native` | No | Use native TLS (via tonic's `tls-native` feature) | Uses system OpenSSL/Schannel/SecureTransport. |

> **Note**: If neither `tls-rustls` nor `tls-native` is enabled, TLS is handled by tonic's default behavior (which typically requires one of these features for `https://` connections). The crate documentation MUST clearly state this.

---

## Non-Goals

The following are explicitly **out of scope** for v1:

| # | Non-Goal | Rationale |
|---|---|---|
| NG-1 | CLI binary | This is a library crate. CLI tooling is a separate concern. |
| NG-2 | Schema DSL parser/compiler | Schema strings are passed through as-is. Parsing the Zanzibar-style DSL is out of scope. |
| NG-3 | Admin / metrics endpoints | SpiceDB admin APIs (health, metrics, dispatch) are not part of the core authorization API. |
| NG-4 | Non-tokio runtime support | Tonic requires tokio. Abstract runtime support adds complexity for minimal benefit. |
| NG-5 | Caching layer | Permission caching is application-specific. The library should be a thin, faithful client. |
| NG-6 | Retry / circuit breaker middleware | Users can compose these via `tower` layers on the channel. Not built in for v1. The `Error::is_retryable()` helper assists callers in building their own retry logic. |
| NG-7 | mTLS support (stretch goal) | Acknowledged as desirable, but deferred as a first-class API. Bearer token auth covers the primary use case. mTLS is achievable today by passing a custom `tonic::Channel` via FR-1.5. |
| NG-8 | Auto-reconnect for Watch | Watch streams terminate on error. The caller is responsible for reconnection logic using the checkpoint `ZedToken` from the last `WatchEvent`. This is consistent with NG-6. |

---

## Open Questions

| # | Question | Impact | Owner |
|---|---|---|---|
| OQ-3 | Should the crate be published to crates.io or remain internal-only initially? | Affects naming, documentation, and versioning rigor. | Maintainer |
| OQ-4 | What is the minimum supported Rust version (MSRV) policy? Latest stable? N-2? | Affects dependency choices and CI matrix. | Maintainer |
| OQ-7 | What crate name? `prescience`, `spicedb`, `spicedb-client`, `authzed`? | Affects discoverability and potential trademark concerns. | Maintainer |

---

## Risks & Dependencies

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R-1 | **SpiceDB API breaking changes** — proto definitions change in incompatible ways | Low (v1 API is stable) | High — breaks compiled client | Pin to a specific proto tag; test against multiple SpiceDB versions in CI. |
| R-2 | **Tonic major version bump** — tonic 0.x is pre-1.0 and may have breaking changes | Medium | Medium — requires code changes | Pin tonic version range; monitor releases. |
| R-3 | **Protobuf codegen drift** — generated code diverges from what SpiceDB actually accepts | Low | High — runtime failures | Integration tests against a real SpiceDB container in CI. |
| R-4 | **Streaming complexity** — long-lived Watch streams may have subtle error handling bugs | Medium | Medium — unreliable watch | Explicit streaming behavioral contracts (see FR-4.1 and streaming behavior table). Document limitations clearly. |
| R-5 | **Naming collision** — crate name conflicts with existing crates.io packages | Low | Low — rename before publish | Check crates.io availability early. |
| R-6 | **Caveat complexity** — `ContextValue` mapping to/from `prost_types::Value` may have edge cases (e.g., NaN, infinity, deeply nested structs) | Medium | Low — runtime surprises | Thorough unit tests for ContextValue round-tripping. Document known limitations. |

### External Dependencies

| Dependency | Version (indicative) | Purpose |
|---|---|---|
| `tonic` | 0.12+ | gRPC client framework |
| `prost` | 0.13+ | Protobuf serialization |
| `prost-types` | 0.13+ | Well-known protobuf types (Struct, Value for caveat context) |
| `tokio` | 1.x | Async runtime |
| `tower` | 0.4+ / 0.5+ | Service middleware (bearer token interceptor) |
| `tonic-build` | 0.12+ (build-dep) | Protobuf code generation |
| `authzed/api` | latest tagged release | Protobuf source definitions (pinned to tag, not `main`) |
| `serde` | 1.x (optional) | Serialization for ZedToken and domain types |
| `tracing` | 0.1.x | Diagnostic logging (e.g., insecure connection warnings) |

---

## Decisions Made

| # | Decision | Rationale |
|---|---|---|
| D-1 | **Use `tonic` for gRPC transport** | De facto standard Rust gRPC library. Mature, actively maintained, built on tokio + hyper + tower. |
| D-2 | **Generate types from official `authzed/api` protos** | Single source of truth. Avoids manual type definitions falling out of sync. |
| D-3 | **Wrap generated protos in idiomatic Rust types** | Raw proto types have `Option` everywhere and stringly-typed fields. Wrapping provides validation, ergonomics, and a stable public API decoupled from proto layout. |
| D-4 | **Require `tokio` runtime** | Tonic requires tokio. Supporting multiple runtimes adds complexity with no current demand. |
| D-5 | **Bearer token auth, not mTLS, for v1** | Covers the vast majority of SpiceDB deployments. mTLS can be layered in by passing a custom channel (FR-1.5). |
| D-6 | **Library crate only** | Keeps scope focused. CLI and other binaries are separate concerns. |
| D-7 | **Custom error type, not `anyhow`/`eyre`** | Library crates should provide structured errors that callers can match on. Concrete enum variants enable exhaustive matching and testable error assertions. |
| D-8 | **Streaming methods return `impl Stream`** | Consistent async Rust idiom. Not boxed trait objects — `impl Stream` for zero-cost. Callers can use `StreamExt` combinators or `while let` loops. |
| D-9 | **Pin to latest tagged release of `authzed/api` (not `main`)** | Resolved from OQ-1. Tagged releases are stable and versioned. Tracking `main` risks ingesting breaking or incomplete proto changes. Pin to a specific tag (e.g., `v1.35.0`) and update deliberately. |
| D-10 | **Use `build.rs` codegen (don't commit generated code)** | Resolved from OQ-2. Standard Rust approach. Avoids stale generated code in the repo. Consumers need `protoc` installed (documented in README) or we vendor the proto files and use `prost-build` which doesn't require `protoc`. |
| D-11 | **Feature-gate ExperimentalService behind `#[cfg(feature = "experimental")]`** | Resolved from OQ-5. These APIs may change without notice in SpiceDB. Feature-gating clearly signals instability and prevents accidental reliance. |
| D-12 | **Caveats are IN SCOPE for v1 (read/check path + write path)** | Resolved from OQ-6. Caveated relationships are a core SpiceDB feature. Omitting caveat support would make the client unable to correctly interpret `CONDITIONAL` permission results — a security hazard. The `PermissionResult` enum faithfully represents all three states. |
| D-13 | **`CheckPermission` returns `PermissionResult` enum, not `bool`** | SpiceDB's 3-state Permissionship (HAS_PERMISSION / NO_PERMISSION / CONDITIONAL) cannot be faithfully represented as a boolean. Returning `bool` is lossy and a security hazard. A convenience `.is_allowed()` method is provided but returns `Result` to force handling of the CONDITIONAL case. |
| D-14 | **TLS determined by URI scheme** | `https://` = TLS, `http://` = plaintext. Non-loopback `http://` requires `.insecure(true)`. For advanced TLS (custom CAs, mTLS), use `Client::from_channel()`. Follows principle of secure-by-default with explicit opt-out. |
| D-15 | **No auto-reconnect for streaming** | Consistent with NG-6 (no built-in retry). Callers manage reconnection using checkpoint ZedTokens. This keeps the library thin and avoids opinionated retry policies. |

---

## References

- [SpiceDB Documentation](https://authzed.com/docs)
- [SpiceDB Caveats Documentation](https://authzed.com/docs/spicedb/concepts/caveats)
- [authzed/api Protobuf Definitions](https://github.com/authzed/api)
- [lunaetco/spicedb-client (community Rust client)](https://github.com/lunaetco/spicedb-client)
- [Google Zanzibar Paper](https://research.google/pubs/pub48190/)
- [Tonic gRPC Framework](https://github.com/hyperium/tonic)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
