use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = PathBuf::from("proto/authzed-api");
    let stubs_root = PathBuf::from("proto/stubs");

    let proto_files = &[
        "authzed/api/v1/core.proto",
        "authzed/api/v1/permission_service.proto",
        "authzed/api/v1/schema_service.proto",
        "authzed/api/v1/watch_service.proto",
        "authzed/api/v1/debug.proto",
        "authzed/api/v1/experimental_service.proto",
    ];

    let proto_paths: Vec<PathBuf> = proto_files
        .iter()
        .map(|f| proto_root.join(f))
        .collect();

    // Include paths for resolving imports
    let includes = &[
        proto_root.clone(),
        stubs_root,
    ];

    tonic_build::configure()
        .build_server(false)
        .compile_protos(&proto_paths, includes)?;

    // Rerun if proto files change
    for f in proto_files {
        println!("cargo:rerun-if-changed=proto/authzed-api/{}", f);
    }

    Ok(())
}
