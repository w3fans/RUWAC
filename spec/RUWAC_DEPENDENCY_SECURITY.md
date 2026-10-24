# RUWAC dependency security assessment

Release qualification update (2026-10-24): a fresh unsuppressed scan discovered
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html)
in crypto-bigint 0.7.5. The pending release now locks crypto-bigint 0.7.6 and
ctutils 0.4.5, using the compatible upstream patch. The advisory covers timing
variability in arithmetic intended to be constant-time; upstream identifies
0.7.6 as patched. This does not resolve the separate RSA finding. Fresh build,
scan and runtime qualification are required before this candidate is delivered.

Published baseline reconciliation (2026-10-24): v0.4.40 includes the compatible
patches described below and completed frozen/native/public-artifact acceptance.
Its hosted unsuppressed scanner reports RUSTSEC-2023-0071 for the desktop, no
headless vulnerabilities, and the two documented maintenance warnings. Hosted
quality/privacy and package builds passed; the advisory job remains nonzero.
The following candidate notes and older assessment are dated history, not a
claim that v0.4.39 remains the latest delivered build. Any future dependency
change requires a fresh scan and runtime qualification.

Current remediation candidate (2026-10-23): the desktop lockfile resolves
`rustls-webpki 0.103.16` and `ctutils 0.4.4`; both workspaces pin Tokio 1.53.3.
The Tokio patch also removes the unsoundness warning RUSTSEC-2026-0386 found
in the first fresh candidate scan. These are compatible patch updates
for [RUSTSEC-2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382) and
[the ctutils fix](https://github.com/RustCrypto/utils/pull/1569). The last published
v0.4.39 still contained the affected versions. The locked Fedora normal dependency tree and the all-target normal tree contain
no path to rustls-webpki: this candidate's rustls provider is in the development
server graph. This does not establish a production outer-TLS certificate flaw;
RUWAC's RDP connection uses native-tls/OpenSSL. The fresh unsuppressed cargo-audit 0.22.2 scan reports no headless
vulnerabilities and only RUSTSEC-2023-0071 for the desktop; both patchable
findings and the Tokio unsoundness warning are absent. The maintenance warnings for atomic-polyfill and
rustls-pemfile remain visible. Fresh full-build/native evidence is still
required for this changed candidate; it is not a
claim that v0.4.39's advisory job passed. The new WebKitGTK GUI uses the Fedora
system browser libraries and adds no browser dependency to the headless workspace.

RSA remains an unresolved production dependency; no advisory suppression,
unreviewed cryptographic fork or forced incompatible SSPI upgrade is introduced.
The older assessment below is retained as dated history.


Assessed 2026-10-12. This is a source and dependency-path assessment, not a cryptographic timing audit or stable-distribution clearance.

The locked desktop graph contains `rsa 0.10.0-rc.18`, flagged by [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html). The advisory describes timing disclosure of RSA private keys when an attacker can observe private-key operations. Its patched-version list is empty. The headless lockfile has no reported vulnerabilities in the inspected advisory snapshot and contains no RSA, IronRDP or GTK dependency.

## Production and fixture paths

A production-only `cargo tree --locked --manifest-path ruwac/desktop/Cargo.toml --edges normal --invert rsa` confirms three direct users: `picky 7.0.0-rc.25`, `sspi 0.21.3` and `winscard 0.3.3`. These remain through IronRDP connector/async/Tokio dependencies without the development server. This is not a fixture-only finding.

`desktop/src/rdp.rs::run_session` constructs only `Credentials::UsernamePassword`, sets CredSSP on, and does not configure Kerberos, PKU2U or smart-card credentials. In locked `ironrdp-connector/src/credssp.rs::CredsspSequence::init`, this maps to SSPI `AuthIdentity`; absent Kerberos configuration selects `ClientMode::Ntlm`. RUWAC's outer TLS client uses native-tls/OpenSSL with system roots, optional explicit CA and hostname validation. It does not use RustCrypto RSA for TLS private-key decryption.

The same connector supports a separate smart-card route which imports a PKCS#1 private key into SSPI `SmartCardIdentity`. The locked winscard implementation performs private RSA operations in `Scard::sign` through `rsa::hazmat::rsa_decrypt_and_check` and RSA signing. Picky implements private RSA signing; SSPI also includes optional PKU2U and Kerberos/smart-card paths. RUWAC's current profile and connection configuration does not expose these modes. Adding them requires a fresh path assessment and a reviewed cryptographic remediation. No timing experiment or complete binary-level reachability proof was performed; the lockfile finding remains active.

The development-only `ironrdp-server` and acceptor widen the test graph. The local fixture generates a disposable RSA certificate with OpenSSL and serves TLS through rustls/provider code. That proves authenticated fixture interoperability, not absence of the production dependency or resistance to timing attacks. Private fixture keys are never published.

## Upstream remediation

Registry and release checks found newer `rsa 0.10.0-rc.19`, `sspi 0.23.0` and `picky 7.0.0-rc.27`; IronRDP 0.17.0 and connector 0.10.0 remain latest. The [RSA rc.19 release](https://github.com/RustCrypto/RSA/releases/tag/v0.10.0-rc.19) describes PKCS#1 compatibility, not resolution of this advisory. The current advisory database still lists no patched version. The downloaded SSPI 0.23 and Picky rc.27 manifests retain RSA. Connector pins Picky rc.25 and SSPI 0.21; moving to SSPI 0.23 is outside that semver range. A blind dependency bump is neither compatible remediation nor evidence of a fix.

Viable upstream work is a maintained IronRDP/SSPI/Picky release that adopts reviewed RSA padding/implicit-rejection and blinding changes and receives advisory clearance, or an upstream feature split that removes unused private-key providers from the password-only graph. Disabling RUWAC's top-level default features currently cannot remove RSA: connector explicitly enables SSPI smart-card support, and Picky/SSPI also depend on RSA directly. Follow [padding issue 626](https://github.com/RustCrypto/RSA/issues/626), [implicit rejection 680](https://github.com/RustCrypto/RSA/pull/680) and [default blinding 702](https://github.com/RustCrypto/RSA/pull/702). Unreviewed cryptographic forks and forced version overrides were not introduced.

## Executed checks and release boundary

Pinned cargo-audit 0.22.2 scans both lockfiles without ignored advisories. The inspected database commit is `7eebec69c352c7191b1f13eb95dd510eeca5d1de`: headless exits 0, desktop exits 1 for RUSTSEC-2023-0071. Desktop additionally reports unmaintained `atomic-polyfill` and `rustls-pemfile`; they remain visible maintenance work. Hosted source/format/lint/tests run in a separate quality job. Each lockfile has its own advisory step, so an earlier scan failure does not hide desktop results.

Operational source backup releases remain distinct from stable security qualification, RPM distribution, Windows/GNOME external-host acceptance and physical desktop acceptance. No finding is suppressed to obtain a green badge.

## Maintenance-warning runtime scope

Production-only dependency trees on the Fedora target contain neither maintenance-warning package. An all-platform production tree places atomic-polyfill under heapless → iso7816 → winscard → SSPI; it is a platform-conditioned route. rustls-pemfile is absent even from the all-platform normal graph and appears through the development IronRDP server fixture. Both lockfile warnings remain visible. These distinctions do not remove or downgrade the production RSA finding.
