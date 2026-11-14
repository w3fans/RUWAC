RUWAC v0.4.78 — rustix security maintenance

Update rustix from 1.1.5 to the upstream patched 1.1.6 for
RUSTSEC-2026-0486. Its extended-attribute functions could treat a zero-length
buffer query as initialized data. The update keeps the existing APIs and
uses the normal Cargo resolver, without an advisory suppression or crate override.

Cargo also reselects existing Windows-only dependency edges in errno, gio-sys
and tempfile within their upstream version ranges. Their versions and checksums
remain unchanged. Other package versions and checksums are unchanged. Windows
runtime acceptance remains pending on the established R7 validation path.

The separate desktop RSA advisory and maintenance warnings remain visible.
This update follows the existing source, runtime, package and publication gates.
Installing packages preserves running engines and terminal programs; existing
sessions are not migrated or restarted.
