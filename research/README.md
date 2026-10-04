# Alternative implementation research

This is a chronological, append-only index. Each entry records the sources,
methodology, observations, and conclusion as they stood on its review date.
An updated review should be a new dated document and may supersede a conclusion
without erasing the earlier decision record.

| Date | Topic | Conclusion |
|---|---|---|
| 2026-08-25 | [`crap4ts` alternatives](./2026-08-25-alternative-implementations.md) | Build a TypeScript-first implementation from Icaruswings-style boundaries, Gligorkot-style exclusive attribution, and an explicit quality-gate policy. |
| 2026-08-25 | [Rust architecture reassessment](./2026-08-25-rust-architecture-reassessment.md) | Supersede the TypeScript-first implementation choice with a Rust core and CLI plus a thin npm distribution wrapper. |
| 2026-08-26 | [Release status of the inspirational repositories](./2026-08-26-inspiration-release-status.md) | Six of eleven had a registry or GitHub release; at the reviewed commit this repository still needed a real v1 and had Windows CI, npm identity, and first-publication blockers. |
| 2026-10-02 | [Uncle Bob attribution and crap4ts comparison](./2026-10-02-unclebob-crap4ts-comparison.md) | No public unclebob/crap4ts found; compared sebassdc/crap4ts, whose usability features are useful but coverage/symbol correctness is weaker than the local core. |
| 2026-10-03 | [crapper TypeScript comparison](./2026-10-03-crapper-typescript-comparison.md) | Borrow explicit branch scoring, optional-chain complexity, route labels, and changed-file convenience; retain strict coverage evidence and independent nested functions. |
