# Third-Party Notices

Toche's Rust implementation draws on design ideas and patterns from the projects
listed below. The repository keeps reference clones in the gitignored
`vendor_reuse/` directory, while Toche implements its integration under `src/`
and commits required build inputs under `assets/`. Applicable licenses,
copyright notices, project links, and integration decisions are preserved here.

## Reuse inventory

| Project | License | Clone commit | Integration | Purpose |
|---------|---------|-------------|-------------|---------|
| [ccusage](https://github.com/ccusage/ccusage) | MIT | `ba99c0d` | Reference: adapt pricing/reporting patterns | Usage ledger concepts, pricing models, reporting |
| [RTK](https://github.com/rtk-ai/rtk) | Apache-2.0 | `5d32d07` | Reference: adapt filter and hook architecture | Command/tool-output reduction, Claude Code hook integration |
| [Graphify](https://github.com/Graphify-Labs/graphify) | MIT | `43b2aff` | External CLI/MCP adapter | Local project graph and query service |
| [andrej-karpathy-skills](https://github.com/multica-ai/andrej-karpathy-skills) | MIT | `2c60614` | Reference: adapt policy rules | Careful-work policy profile (Section 0.6.0) |
| [caveman-claude](https://github.com/juliusbrussee/caveman) | MIT | `0d95a81` | Reference: adapt concision patterns | Concise response profile, token reduction strategies |

## License texts

### ccusage: MIT

```
MIT License
Copyright (c) 2025 ryoppippi
```
Full text: `vendor_reuse/ccusage/apps/ccusage/LICENSE`

### RTK: Apache-2.0

```
Apache License, Version 2.0
Copyright 2024 rtk-ai and rtk-ai Labs
```
The imported filter definitions are documented in
[`assets/filters/README.md`](assets/filters/README.md). The applicable
Apache-2.0 terms are reproduced in Toche's [LICENSE](LICENSE).

### Graphify: MIT

```
MIT License
Copyright (c) 2026 Safi Shamsi
```
Full text: `vendor_reuse/graphify/LICENSE`

### andrej-karpathy-skills: MIT

License type confirmed as MIT per project documentation. No standalone LICENSE file in repository root as of the cloned commit.

### caveman-claude: MIT

```
MIT License
Copyright (c) 2026 Julius Brussee
```
Full text: `vendor_reuse/caveman-claude/LICENSE`

## Integration decisions

Per Section 4 of the Toche plan, the decision for each project:

- **ccusage**: Reference adaptation. Study Rust usage-tracking patterns; adapt pricing models and reporting concepts into Toche's Meter module. Not a library dependency. Toche implements its own SQLite ledger.
- **RTK**: Reference adaptation. Toche implements its own deterministic reducer and imports 63 Apache-2.0 TOML filter definitions from the pinned RTK commit. The RTK binary is not bundled.
- **Graphify**: External adapter. Toche calls Graphify's CLI/MCP interface as an optional integration. Graphify is not compiled into Toche. No Python engine code is ported.
- **andrej-karpathy-skills**: Reference adaptation. Policy rules (think-first, surgical-change, verify-outcome) adapted into Toche's efficiency profiles. No verbatim copy.
- **caveman-claude**: Reference adaptation. Concision patterns and intensity levels adapted into Toche's response profiles. Hook architecture patterns inform Toche's own hook system.

No transitive dependencies from these projects are included in Toche. Each project's own dependency tree is documented in its respective repository.

## Toche license

Toche is distributed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the full text.
