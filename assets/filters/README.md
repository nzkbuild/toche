# Built-in RTK filter definitions

This directory contains the TOML tool-output filter definitions used by
Toche's reduction pipeline.

- `rtk/` contains 63 definitions imported without modification from RTK.
- `toche/` contains Toche-owned definitions for commands required by its tests
  and public behavior.

The files were imported without modification from:

- Project: [rtk-ai/rtk](https://github.com/rtk-ai/rtk)
- Commit: `5d32d0736f686b69d1e8b9dc45c007d4eb77a0a2`
- Source path: `src/filters/*.toml`
- License: Apache License 2.0

Toche's `build.rs` validates and combines all 65 committed definitions at build
time. Keeping the required inputs in the public repository makes clean clones,
CI jobs, and release builds reproducible without the gitignored reference clone.

The applicable Apache-2.0 terms are available in the repository's root
[`LICENSE`](../../LICENSE), and RTK attribution is preserved in
[`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md).
