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

Chroots: `fedora-44-x86_64` and `fedora-rawhide-x86_64`. Add
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
