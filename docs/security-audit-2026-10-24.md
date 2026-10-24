# Locked dependency security remediation, 2026-10-24

## Scope

This is a focused dependency remediation, not a claim that the whole application has been
security audited. The existing `cargo audit --deny warnings` gate reported 12 vulnerabilities
and two denied warnings against the previous lockfile. No advisory is ignored or suppressed.
Admission, submission, licensing, and workflow policy are unchanged.

| Dependency | Previous | Remediated | RustSec advisories |
| --- | --- | --- | --- |
| `ctutils` | 0.4.2 | 0.4.4 | [2026-0385](https://rustsec.org/advisories/RUSTSEC-2026-0385) |
| `quinn-proto` | 0.11.16 | 0.11.18 | 2026-0338 through 2026-0342 |
| `rustls` | 0.23.45 | 0.23.46 | 2026-0346 through 2026-0349 |
| `rustls-webpki` | 0.103.15 | 0.103.16 | [2026-0382](https://rustsec.org/advisories/RUSTSEC-2026-0382) |
| `webpki-roots` | 1.0.9 | 1.0.10 | [2026-0381](https://rustsec.org/advisories/RUSTSEC-2026-0381) |
| `tokio` | 1.53.1 | 1.53.3 | [2026-0386](https://rustsec.org/advisories/RUSTSEC-2026-0386) |
| YAML backend | `serde_yaml` 0.9.34 / `unsafe-libyaml` 0.2.11 | `yaml_serde` 0.10.7 / `libyaml-rs` 0.3.0 | [2024-0449](https://rustsec.org/advisories/RUSTSEC-2024-0449) |

## YAML compatibility and maintenance

The [YAML Organization's maintained fork](https://github.com/yaml/yaml-serde) documents
Cargo package renaming as its migration path. The dependency retains the local `serde_yaml`
name so existing parser calls and the application error conversion do not change. The published
0.10.7 package requires Rust 1.82; its `libyaml-rs` 0.3.0 dependency requires Rust 1.60, both
below ContribAI's Rust 1.88 minimum.

This is not a switch to `serde_yaml_ng`, which still depends on the unmaintained
`unsafe-libyaml`. It also does not claim to eliminate unsafe Rust: the maintained
[libyaml-rs backend](https://github.com/yaml/libyaml-rs) remains a translated libyaml
implementation. The change removes the unmaintained package, not every possible parser risk.

The added compatibility tests cover duplicate consent fields, multiple YAML documents,
malformed syntax, missing anchors, non-boolean enablement, unknown permissions, valid anchors,
schema-two defaults, configuration round trips, profile/template scalars, error conversion,
and the `Value`/`from_value` provider-configuration API used by the bounded harness.
Existing tests continue to cover both consent schemas and the shipped configuration examples.

## Verification

- `cargo audit --deny warnings`: passed with no ignored advisories or denied warnings.
- `cargo fmt --all -- --check`: passed.
- `cargo test --workspace --locked`: 844 passed, zero failed, one existing ignored doc test
  on the dependency-only patch over `cea4725331962f6b3a365e5b3122e0e7037e2a35`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo +1.88 check --workspace --all-targets --locked`: passed.
- `cargo build --workspace --release --locked`: passed.
- Release-binary `conformance`: 43/43 passed. The offline admission `demo` passed with
  submission disabled. The combined harness release's offline `harness demo`, `runs`, and
  `inspect` smoke checks also passed, with external writes disabled.

The audit reflects the advisory database available on the date above, not a guarantee against
future advisories. Platform-specific CI remains necessary in addition to local Linux checks.
