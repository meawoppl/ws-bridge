# Contributing and releasing

Use Rust 1.94 or newer. Before opening a PR run:

```sh
cargo fmt -- --check
cargo test --all-features --locked
cargo test --locked
cargo clippy --all-features -- -D warnings
cargo check --features yew-client --target wasm32-unknown-unknown --locked
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps --locked
cargo +1.94.0 check --all-features --locked
cargo audit
cargo package --all-features --locked
```

CI also checks each adapter independently. Add live interoperability coverage when
adding a transport. Keep default features empty and avoid framework dependencies in
the codec/core API. Changes must preserve the documented 1.x compatibility contract.

For a release, update package version, changelog, dependency examples and migration
notes together. Review the packaged file list (`cargo package --list`) and ensure all
CI checks pass on the release commit. Merge the release PR, tag that commit `vX.Y.Z`,
and publish GitHub release notes. Publish to crates.io with `cargo publish --locked`
using the maintainer account, then verify the version on crates.io and docs.rs.
Do not announce package availability before publication succeeds. Keep credentials
out of repository files; releases are deliberate maintainer actions.
