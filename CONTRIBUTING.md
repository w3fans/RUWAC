# Contributing to RUWAC

RUWAC preserves daemon-owned terminals while the native GTK desktop presents terminals, remote desktops and files. Contributions must preserve running processes, stable identities, private configuration and the independent headless dependency graph.

Use one focused pull request with the observed problem, resulting behavior and relevant test evidence. Explain which tests used actual PTYs, authenticated endpoints or GDK transport and which used synthetic callbacks. External Windows/Fedora and physical input results require actual evidence; local fixtures do not establish those gates.

## Open your first pull request

Fork [w3fans/RUWAC](https://github.com/w3fans/RUWAC) on GitHub, then clone your
fork. Replace `YOUR-USERNAME` below with your GitHub username. Run these commands
from the directory where you keep source checkouts:

```sh
git clone https://github.com/YOUR-USERNAME/RUWAC.git
cd RUWAC
git remote add upstream https://github.com/w3fans/RUWAC.git
git fetch upstream
git switch -c docs/contributor-notes upstream/main
```

Use a descriptive branch name for your change. Make one focused improvement,
then run the checks relevant to it from the checkout root. For a documentation-only
change, check the diff and public source inventory before committing:

```sh
git diff --check
python3 scripts/public-files.py --check
git diff
```

Code changes also need the relevant build and validation checks below. Include
what you actually ran in the pull request; do not claim unperformed runtime or
hardware tests.

Stage only the intended files, commit and push the branch to your fork. For
example, if you edited this guide:

```sh
git add CONTRIBUTING.md
git commit -m "docs: clarify the contribution workflow"
git push -u origin docs/contributor-notes
```

On GitHub, open a pull request with **base repository** `w3fans/RUWAC`, **base**
`main`, and your fork and branch as the head. Fill in the pull request template
with the problem, change and validation evidence. Further commits pushed to the
same branch update that pull request.

The maintainer can review **Files changed**, inspect the checks and request
revisions before merging. A pull request proposes a change; opening it does not
merge it or publish a release. After it is merged, update your local main branch
before starting another contribution:

```sh
git switch main
git fetch upstream
git merge --ff-only upstream/main
```

If the fast-forward fails, inspect your local commits before proceeding; do not
reset or force-push away work to make the command succeed.

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

Report vulnerabilities according to [SECURITY.md](SECURITY.md). Participation follows [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
