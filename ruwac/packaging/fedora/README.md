# Fedora and COPR packaging

`ruwac.spec` here builds RUWAC from source for COPR and, later, Fedora.
It is separate from `../ruwac.spec`, which repackages the binaries built by the
upstream release pipeline for the GitHub testing RPMs and `ruwac-rpm-update`.

| | GitHub testing RPMs | This package |
|---|---|---|
| Built from | Release pipeline binaries | Source, in mock/COPR |
| Crates | — | Vendored tarball (`Source1`) |
| Updates | `ruwac-rpm-update`, optional timer | DNF only; no updater, timer or GitHub checks |

The package writes `/usr/share/ruwac/rpm-installation.json` with
`"kind":"distribution"`. With that marker, `ruwac update` and the desktop's
update button point to `dnf upgrade` and never contact GitHub.

## Install the terminal or desktop

The COPR project is **zvegelj/ruwac**. Its Packages page has one entry per
**source package**. The `ruwac` source package builds two installable packages:

- `ruwac`: the persistent terminal engine and CLI, without GTK runtime dependencies.
- `ruwac-desktop`: the GTK GUI, RDP and remote workspace; it requires the matching
  `ruwac` package automatically.

The GUI is already distributed through COPR; a second source-package entry is
not needed. Both packages are published for Fedora 43, 44, 45 and Rawhide x86_64.
These targets have successful COPR builds and downloadable binary RPMs; this
does not establish physical runtime acceptance on every target. Other versions
and architectures need their own successful build qualification.

Enable the repository, then choose the package you need:

```sh
# Install dnf5-plugins first if the "dnf copr" command is unavailable.
sudo dnf copr enable zvegelj/ruwac
sudo dnf install ruwac-desktop  # GUI and matching CLI
# On a headless SSH server, install only ruwac instead.
```

Update a desktop installation with the package manager:

```sh
sudo dnf upgrade --refresh ruwac ruwac-desktop
```

DNF manages the installed files. Already-running GUI windows, attached terminal
clients and persistent engines keep their current code; package updates preserve
those processes. Reopening the GUI loads its new version, while a persistent
engine remains old until deliberately retired. Use `ruwac engine status --json`
with v0.4.59 or later to distinguish installed and running engine versions.

### Moving from a per-user installation

Install the COPR package first. Then, as the same ordinary user who owns the
per-user install, run:

```sh
/usr/bin/ruwac update
hash -r
type -a ruwac ruwac-desktop
rpm -q ruwac ruwac-desktop
```

For a distribution installation, this explicit `update` command retires only
recognized RUWAC-owned per-user command/resource links, the desktop launcher and
its automatic updater. It preserves the old release files, running engines and
sessions, profiles and credentials. Launchers outside the recognized RUWAC pattern are retained for review. Back up
any customized RUWAC launcher before retirement, because a customized entry may
still match that pattern. Check that the selected commands now use `/usr/bin`;
do not delete
old release directories while any running process or test fixture uses them.
A previous GitHub testing RPM with the same version may need an explicit COPR
reinstall transaction; inspect its vendor and distribution marker first.

## Many machines: one-time setup and automatic updates

Run the setup once per machine as root, for example from an administration
host over SSH (the same script is installed as `/usr/libexec/ruwac/fleet-setup`
for later changes):

```sh
ssh HOST 'sudo bash -s -- --ring standard' < ruwac/packaging/fedora/fleet-setup
```

It enables exactly the `zvegelj/ruwac` COPR, sets that repository's metadata
to expire after five minutes, installs `ruwac` (and `ruwac-desktop` only on
machines that boot to a graphical session, or with `--desktop`), and enables
`ruwac-package-update.timer`. Re-running it with the same options changes
nothing; `--json` reports what changed per host.

| Ring | Check interval | Spread |
| --- | --- | --- |
| `canary` | about every 45 minutes | up to 10 minutes per machine |
| `standard` | about every 3 hours | up to 30 minutes per machine |
| `disabled` | timer off | — |

The timer runs `/usr/libexec/ruwac/package-update`, which only updates the
installed RUWAC packages from the COPR (dependencies from Fedora's own
repositories), defers while an offline system update is queued, and records a
sanitized result. It never stops, restarts or migrates a user's RUWAC engine:
running terminals keep their engine and new sessions use the installed version.
See the state on any machine, including over SSH:

```sh
ruwac update status        # or --json
ruwac engine show          # engines, live terminals, where new sessions go
```

Roll out in stages: one canary desktop, one headless machine, a few varied
desktops, then batches.

## Build an SRPM

```sh
ruwac/packaging/fedora/make-sources.sh --tag v0.4.53 --outdir /tmp/ruwac-srpm
# or, from the current checkout:
ruwac/packaging/fedora/make-sources.sh --outdir /tmp/ruwac-srpm
```

The script needs `git`, `cargo` (network or a populated Cargo cache) and
`rpm-build`. It writes `RUWAC-<version>.tar.gz`, `ruwac-<version>-vendor.tar.xz`
and the SRPM.

Test locally with `mock -r fedora-44-x86_64 --rebuild <srpm>`.

## COPR

- **Upload:** `copr-cli build <owner>/<project> /tmp/ruwac-srpm/ruwac-*.src.rpm`
- **SCM (automatic):** add the package with source type *SCM*, the GitHub URL,
  SRPM build method **make srpm** and spec `ruwac/packaging/fedora/ruwac.spec`.
  COPR then runs `.copr/Makefile`. A GitHub webhook can rebuild on every tag.

Chroots: `fedora-43-x86_64`, `fedora-44-x86_64`, `fedora-45-x86_64`
and `fedora-rawhide-x86_64`. Add
`fedora-44-aarch64` once aarch64 builds are verified.

## Status

Phase A, COPR:

- [x] From-source spec with both subpackages
- [x] Vendored-crate SRPM tooling
- [x] COPR SCM entry point
- [x] Desktop file and AppStream metadata
- [x] Distribution-managed updates
- [ ] Builds verified on Fedora 44 and Rawhide in mock and COPR

Phase B, Fedora review:

Start with the headless `ruwac` package if the GUI needs additional review work.
Keep `ruwac-desktop` available on COPR in the meantime. Before submitting a
CLI-only source package, qualify a build path that does not require the GTK/RDP
build stack, and test version/dependency compatibility with the COPR desktop.
A runtime-only CLI package is already separate; this does not yet prove that the
combined SRPM can be submitted as a CLI-only Fedora build unchanged.

- [ ] Trim Windows/macOS-only and test-only crates from the vendor tarball
- [ ] Per-crate `License:` reconciliation from `LICENSE.dependencies`
- [ ] Unbundle or justify the patched IronRDP and alacritty_terminal crates
- [ ] Re-check the known RSA advisory (RUSTSEC-2023-0071) with the Rust SIG
- [ ] Clean `rpmlint` and `fedora-review`
- [ ] Package review request in Red Hat Bugzilla

Phase C, maintenance:

- [ ] Webhook builds from release tags
- [ ] Rawhide rebuilds
- [ ] Upgrade/rollback test between consecutive COPR builds
