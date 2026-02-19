{ pkgs, ... }:

{
  # Project metadata
  name = "prescience";

  # Core build tools
  packages = with pkgs; [
    # Protobuf compiler — required by tonic-build in build.rs
    protobuf

    # Cargo utilities
    cargo-watch
    cargo-nextest

    # SpiceDB — for integration testing
    spicedb
    spicedb-zed
  ];

  # Rust toolchain
  languages.rust = {
    enable = true;
    channel = "stable";
  };

  # Pre-commit hooks for code quality
  pre-commit.hooks = {
    clippy.enable = true;
    rustfmt.enable = true;
  };

  # Environment variables
  env = {
    # Ensure protoc is discoverable by tonic-build
    PROTOC = "${pkgs.protobuf}/bin/protoc";
  };

  # Handy scripts
  scripts = {
    build.exec = "cargo build";
    test.exec = "cargo nextest run";
    test-all.exec = "cargo nextest run --all-features";
    lint.exec = "cargo clippy --all-features -- -D warnings";
    fmt.exec = "cargo fmt --check";

    # Start a local SpiceDB for integration testing
    spicedb-up.exec = ''
      echo "Starting SpiceDB on :50051 (in-memory, insecure)..."
      spicedb serve --grpc-preshared-key "test-key" --datastore-engine memory
    '';
  };

  # Ensure submodules are initialized
  enterShell = ''
    if [ ! -f proto/authzed-api/authzed/api/v1/core.proto ]; then
      echo "⚠️  Proto submodule not initialized. Run: git submodule update --init --recursive"
    fi
  '';
}
