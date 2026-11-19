# Contributing to EdgeBridge KVM

Open an issue for a bug or a concrete enhancement. For input and connection problems, include your Windows version, PC roles, display resolutions and scaling, selected edges, and exact reproduction steps. Keep pairing codes, IP addresses, and typed text out of public reports.

## Local checks

Run these commands in the project root on Windows with the prerequisites in the README installed.

```powershell
npm ci
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Add targeted tests when changing frame formats, pairing, display geometry, or state transitions. When changing input code, use the [physical test matrix](docs/validation.md) and state which cases you ran. Keep Windows-specific logic in `native/` where practical.

By contributing, you agree to license your contribution under the project’s MIT License.
