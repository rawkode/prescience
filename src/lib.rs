//! # Prescience
//!
//! An idiomatic Rust client library for [SpiceDB](https://authzed.com/spicedb),
//! the open-source, Google Zanzibar-inspired authorization system.
//!
//! Prescience wraps the SpiceDB gRPC API with strong Rust types, ergonomic builders,
//! and first-class async support via [tonic](https://github.com/hyperium/tonic).
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use prescience::{Client, ObjectReference, SubjectReference, Consistency, PermissionResult};
//!
//! # async fn example() -> Result<(), prescience::Error> {
//! let client = Client::new("http://localhost:50051", "my-token").await?;
//!
//! let result = client
//!     .check_permission(
//!         &ObjectReference::new("document", "doc-123")?,
//!         "view",
//!         &SubjectReference::new(ObjectReference::new("user", "alice")?, None::<String>)?,
//!     )
//!     .consistency(Consistency::FullyConsistent)
//!     .await?;
//!
//! match result {
//!     PermissionResult::Allowed => println!("access granted"),
//!     PermissionResult::Denied => println!("access denied"),
//!     PermissionResult::Conditional { missing_fields } => {
//!         println!("need caveat context: {:?}", missing_fields);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Feature Flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `watch` | No | Enables the WatchService for streaming relationship changes |
//! | `experimental` | No | Enables experimental APIs (BulkCheckPermission, BulkImport/Export) |
//! | `serde` | No | Enables Serialize/Deserialize on ZedToken and domain types |
//! | `tls-rustls` | No | Use rustls for TLS |
//! | `tls-native` | No | Use native TLS |

pub mod client;
pub mod error;
pub mod types;

mod proto {
    #![allow(clippy::all)]
    #![allow(warnings)]

    tonic::include_proto!("authzed.api.v1");

    pub mod google {
        pub mod rpc {
            /// A gRPC Status message (simplified).
            #[derive(Clone, PartialEq, ::prost::Message)]
            pub struct Status {
                #[prost(int32, tag = "1")]
                pub code: i32,
                #[prost(string, tag = "2")]
                pub message: ::prost::alloc::string::String,
                #[prost(message, repeated, tag = "3")]
                pub details: ::prost::alloc::vec::Vec<::prost_types::Any>,
            }
        }
    }
}

pub use client::Client;
pub use error::Error;
pub use types::*;

#[cfg(feature = "experimental")]
pub use client::experimental::BulkCheckItem;
