# Contributing to RUWAC

RUWAC preserves daemon-owned terminals while the native GTK desktop presents terminals, remote desktops and files. Contributions must preserve running processes, stable identities, private configuration and the independent headless dependency graph.

Use one focused pull request with the observed problem, resulting behavior and relevant test evidence. Explain which tests used actual PTYs, authenticated endpoints or GDK transport and which used synthetic callbacks. External Windows/Fedora and physical input results require actual evidence; local fixtures do not establish those gates.

## Build and validate

Keep build output and generated evidence outside the checkout:

```sh
export CARGO_TARGET_DIR="$(mktemp -d)"
export EVIDENCE_DIR="$(mktemp -d)"
cargo fmt --all --check --manifest-path ruwac/Cargo.toml
cargo fmt --all --check --manifest-path ruwac/desktop/Cargo.toml
cargo clippy --locked --workspace --all-targets --manifest-path ruwac/Cargo.toml
cargo clippy --locked --all-targets --manifest-path ruwac/desktop/Cargo.toml
cargo test --locked --workspace --manifest-path ruwac/Cargo.toml
cargo test --locked --manifest-path ruwac/desktop/Cargo.toml
cargo build --locked --manifest-path ruwac/Cargo.toml
python3 ruwac/tests/integration/spike.py --binary "$CARGO_TARGET_DIR/debug/ruwac" --evidence "$EVIDENCE_DIR"
python3 scripts/dependency-check.py --output "$EVIDENCE_DIR/dependencies.json"
python3 scripts/public-files.py --check
```

The quality workflow also validates GitHub syntax/contexts with checksum-pinned actionlint; plain YAML parsing does not prove hosted validity. The desktop requires GTK4, libadwaita, OpenSSL, libssh2 and SQLite development libraries. Fedora CI installs its declared tool packages and checks both workspaces. Existing lint warnings are reported; compiler and Clippy errors fail the job. Format checks are enforced. A separate pinned cargo-audit job checks both lockfiles without advisory exceptions; the known desktop RSA finding currently makes that check fail. Source publication is separate from stable security qualification. Full native, RDP, SSH and package/hardware acceptance remains part of the qualified project workflow.

## Review boundaries

PTY lifecycle, input mapping, authentication, credential handling, CLIPRDR, FFI and release changes require maintainer review. RUWAC’s workflows do not merge pull requests. CODEOWNERS requests review; it does not establish branch protection by itself. Contribution labels describe affected areas and do not authorize merge or publication.

Keep private operational files, credentials, private host maps, generated artifacts and raw VM logs out of source and history. Use generated credentials and isolated sockets for tests. Never print passwords, private keys or clipboard contents in reports.

Dependency changes must update the appropriate Cargo.lock, retain upstream license notices and include advisory/license evidence. Original contributions use the repository's Apache-2.0 license. The contributor must have permission to contribute the code.

Product publication and version allocation belong to the existing qualified build/release authority. Contributors do not invent release versions or bypass that workflow. GitHub quality checks build test artifacts; they do not publish binaries, RPMs or stable releases.

## Development releases and stable milestones

Completed functional changes follow the established development patch-release
workflow: controller-assigned version, changelog, build, required tests and
validation, GitHub publication, then authorized installation/update delivery.
Stabilization recommendations, unfinished unrelated features and incomplete
higher milestones do not impose a standing release freeze. Stable milestones
must additionally meet their own acceptance criteria.

Cloud pull-request contributors use the approved review and CI process; the
designated release authority publishes the validated merged changes. Audit-only
and candidate-only work retains its stated scope and does not authorize invented
changes or deployments just to create a version.

Release gates remain mandatory, including security checks, active release locks
and required approval for service changes. A blocked release must identify the
specific failed gate and corrective action. Reconcile local changes and merged
upstream work before publishing; never overwrite another active checkout.
Use the existing version controller and release scripts throughout.

Report vulnerabilities according to [SECURITY.md](SECURITY.md). Participation follows [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
