# crap4ts

A TypeScript-first CRAP metric quality gate for TypeScript projects.

The intended pipeline is:

```text
CLI -> source selection -> coverage adapter -> TypeScript analysis
    -> exclusive coverage attribution -> CRAP scoring -> report and gate
```

The implementation is being specified from comparative research rather than
by selecting one existing port as a template. Version 1 will be implemented in
TypeScript, with parser and coverage boundaries that allow a future Rust-backed
adapter if measurement shows that one is useful.

## Alternative implementation research

Alternative research is deliberately dated. Later reviews add new entries
instead of silently rewriting the historical basis for an architectural
decision.

- [2026-08-25 — existing `crap4ts` implementations: methodology, comparison,
  and conclusion](./research/2026-08-25-alternative-implementations.md)
- [Research index](./research/README.md)

## Status

The implementation specification is tracked in the project issue tracker.

## License

[MIT](./LICENSE)

