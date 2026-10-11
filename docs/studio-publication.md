# Studio contribution publication

On 2026-10-11, after reviewing the licensing report and testing the optimized
Windows studio, durtyrobbin explicitly requested publication to the open-source
product's GitHub repository. This supersedes the earlier local-only publication
status in the historical validation documents. It does not authorize merging
upstream main or publishing a binary release.

The complete tested studio series is published on the contributor fork for
inspection. The first upstream draft pull request contains only the live mixer
foundation and its optimization/baseline work (`6c9ec00`, `92891b5`). The remaining
painting tools, palette/rack, and AI phases are follow-up contributions, not part
of that initial pull request.

Upstream main was refreshed to `9441484d`. The foundation has integration
conflicts in `brush_cmds.rs`, `alt_grab.rs`, and `canvas.rs`; it is not merge-ready.
Merged #2131's ordinary-brush prepared commits must be preserved. Open #3605
touches live symmetric brush updates. The provider contribution remains separate
from the first PR: texjer's #2878 is still open at
`4754d3c6edb9b37a7ad8516f15425abeca84987d`, and must be coordinated before an AI
integration PR is proposed. Existing contribution history and notices are retained.

The original source and synthetic assets retain MIT OR Apache-2.0. The owner's
supplied screenshot is permitted as documentation of this public contribution;
its notice does not assert a broader standalone license for the owner's marks.
Historical measurements retain their actual source revisions, build settings,
and machine paths. Publishing them is not a claim that the current upstream
combination has passed those checks. Large-brush latency, physical pen and other
platform qualification, real AI model tests, and advanced bristles remain open.
