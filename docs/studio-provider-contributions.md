# Studio provider contribution and licensing record

This is a local engineering provenance report, not a release authorization or a legal determination. Proposed original-source license: the existing workspace **MIT OR Apache-2.0**, retaining both license files, upstream notices and the separate brand exception. No commit author, contributor profile, source history or sealed historical evidence was rewritten. No PR was published and no release/deployment occurred.

The provider foundation reuses texjer's upstream PR #2878, commit `4754d3c6edb9b37a7ad8516f15425abeca84987d`. Original copies and SHA-256 values were preserved before adaptation in `work/provider-upstream`. The independent phase patch adds typed guidance, local protocol support, bounded request/placement, native per-request consent, regeneration and tests to that contribution. It must be coordinated with that PR and current main before publication. The existing #730 OpenAI provider remains a separate contribution; it is not included here. Unrelated .DS_Store files were excluded.

The local adapter was written against the public API contract. No AUTOMATIC1111 source, model weights, checkpoints, generated model assets or proprietary editor implementation is distributed. Installing a backend/model and obtaining its license are separate from licensing this client. Gemini use is subject to its provider terms, API billing and data policy; the client source license grants no provider service or model rights. Test images, brush illustrations and native screenshot fixtures in this task are procedural/original, with no personal photographs.

## Added lockfile packages

Names/versions below come from the actual phase delta. License expressions are package metadata from `cargo metadata --locked --offline --filter-platform x86_64-pc-windows-msvc`; this is not a complete distribution compliance audit. Full-platform offline metadata could not resolve the uncached macOS `security-framework 2.11.1`; those platform-only rows remain explicitly unverified. Before releasing binaries, audit bundled/transitive notices for each target and preserve required notices.

| Package | Version | Declared license |
|---|---|---|
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| byteorder | 1.5.0 | Unlicense OR MIT |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| keyring | 3.6.3 | MIT OR Apache-2.0 |
| photocraft-genai | 0.7.0 | MIT OR Apache-2.0 |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| rustls | 0.23.45 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.15 | ISC |
| security-framework | 2.11.1 | Not available in the Windows-filtered offline metadata |
| security-framework | 3.7.0 | Not available in the Windows-filtered offline metadata |
| security-framework-sys | 2.17.0 | Not available in the Windows-filtered offline metadata |
| subtle | 2.6.1 | BSD-3-Clause |
| untrusted | 0.9.0 | ISC |
| ureq | 3.4.2 | MIT OR Apache-2.0 |
| ureq-proto | 0.6.4 | MIT OR Apache-2.0 |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 |
| webpki-roots | 1.0.9 | CDLA-Permissive-2.0 |
| zeroize | 1.9.0 | Apache-2.0 OR MIT |

The owner's master task requires explicit release approval before publishing advanced palette, custom-bristle or AI components. That boundary has not been crossed. Existing palette/rack original-source attribution stays in ATTRIBUTION.md; advanced bristle simulation is not implemented because its prerequisite performance gate remains blocked. Commit identifiers for new work remain absent pending the owner's Git author identity; the phase manifest supplies base revisions, content trees and verified sequential patches without inventing authorship.
