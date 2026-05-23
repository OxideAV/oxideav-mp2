# oxideav-mp2

A pure-Rust **MPEG-1 / MPEG-2 LSF Audio Layer II** (MP2 / MUSICAM)
codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Orphan-rebuild scaffold (2026-05-24).** The prior implementation was
retired under the workspace
[clean-room policy](https://github.com/OxideAV/oxideav-workspace/blob/master/docs/IMPLEMENTOR_ROUND.md):
the provenance of its bit-allocation and synthesis-window data tables
could not be defended as clean-room — module doc-comments recorded that
those tables had been transcribed from external library source rather
than derived solely from the ISO/IEC specification, which violates the
clean-room provenance requirement. Master history was fully erased per
the Hat-3 cold-enforcement procedure.

The implementation will be re-built from scratch against the staged
ISO/IEC 11172-3 / 13818-3 Layer II specification (numeric tables read
only from the standard) in a future clean-room round.

## License

MIT — see [LICENSE](./LICENSE).
