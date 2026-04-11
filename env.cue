package cuenv

import (
	"github.com/cuenv/cuenv/schema"
	c "github.com/cuenv/cuenv/contrib/contributors"
)

schema.#Base

runtime: schema.#DevenvRuntime

hooks: onEnter: devenv: schema.#Devenv

ci: providers: ["github"]
ci: contributors: [
	c.#Nix,
	c.#CuenvRelease,
]

formatters: rust: {edition: "2021"}

let _t = tasks
let _s = services

services: {
	spicedb: {
		description: "Local SpiceDB for integration testing (in-memory, insecure)"
		command:     "spicedb"
		args: ["serve", "--grpc-preshared-key", "test-key", "--datastore-engine", "memory"]
		readiness: port: {
			port: 50051
		}
		restart: mode: "onFailure"
	}
}

tasks: {
	build: {
		command: "cargo"
		args: ["build"]
		inputs: [
			"Cargo.toml",
			"Cargo.lock",
			"src/**",
			"build.rs",
			"proto/**",
		]
	}
	test: {
		command: "cargo"
		args: ["nextest", "run"]
		dependsOn: [_t.build]
		inputs: [
			"Cargo.toml",
			"Cargo.lock",
			"src/**",
			"tests/**",
			"build.rs",
			"proto/**",
		]
	}
	"test-all": {
		command: "cargo"
		args: ["nextest", "run", "--all-features"]
		dependsOn: [_t.build]
		inputs: [
			"Cargo.toml",
			"Cargo.lock",
			"src/**",
			"tests/**",
			"build.rs",
			"proto/**",
		]
	}
	"test-integration": {
		command: "cargo"
		args: ["nextest", "run", "--all-features"]
		dependsOn: [_t.build, _s.spicedb]
		inputs: [
			"Cargo.toml",
			"Cargo.lock",
			"src/**",
			"tests/**",
			"build.rs",
			"proto/**",
		]
	}
	lint: {
		command: "cargo"
		args: ["clippy", "--all-features", "--", "-D", "warnings"]
		dependsOn: [_t.build]
		inputs: [
			"Cargo.toml",
			"Cargo.lock",
			"src/**",
			"build.rs",
			"proto/**",
		]
	}
	fmt: {
		command: "cargo"
		args: ["fmt", "--check"]
		inputs: ["src/**"]
	}
}
