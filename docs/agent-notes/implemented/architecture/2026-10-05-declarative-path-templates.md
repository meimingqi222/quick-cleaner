# Agent Note: Declared path templates drive development layouts and bounded manifests

Status: implemented

## Problem

The development catalogue already listed CLI agents, local agent caches, VS Code forks and version layouts in TOML, but four shapes were still built in Rust: the `logs*.sqlite` databases inside each declared agent directory, the editor AI session leaves at `<host>/User/globalStorage/<extension>/tasks`, the retired editor extensions recorded in each `<root>/extensions/.obsolete` JSON file, and the orphaned editor workspaces recorded in `<host>/User/workspaceStorage/<hash>/workspace.json`. Every new host, extension or agent therefore required editing `categories/dev.rs`, and the editor layouts hard-coded both the relative structure and the iteration order.

## Decision

Schema 8 extends `directory_selection` with bounded, declarative forms, all interpreted from the same immutable snapshot:

- `paths` entries may contain whole-segment placeholders `{key}`. `key` resolves against `rule.catalogs` first and `rule.lists` second; catalogs contribute their `path` segment plus their localized label, lists contribute the literal name in both languages. Partial interpolation (`{a}-suffix`), the reserved tokens `name`/`relative`, unknown sources, more than two placeholders, more than eight segments and any product above 64 expansions are rejected at load time. List values that could smuggle a separator or traversal (`../`, `a/b`) fail `name()` validation.
- `select.kind = "files"` takes precise files among a declared path's children filtered by literal `prefixes`/`suffixes`; it requires the `file` operation and never expands into a tree.
- `select.kind = "path"` takes the declared path itself. `directories_only` has to agree with the typed operation (`contents` for directories, `file` for files), so a layout cannot claim a scope its operation does not authorize.
- `select.kind = "manifest_children"` takes the declared path's children that a JSON object manifest inside that path marks with the configured boolean. Manifest rows are evidence of a name, never of a path: only single-segment names that still exist as confined directories become targets.
- `select.kind = "orphaned_children"` takes the child directories whose manifest records a local path that no longer exists. The pointer, URI prefix and scope root are declared; the resolved path must sit under the declared root and be absent. Encoded URIs (`%`) and other schemes are never resolved.

`DirectoryRule.root` gains `roaming` (`%APPDATA%` / `~/Library/Application Support`). All declared roots resolve once per scan through the shared `Enumeration`; the orphan scope root is resolved separately from the anchor. Missing roots stay absent, links are rejected, core safety, identity, occupancy and preservation checks are unchanged, and a declared path is probed rather than enumerated. Manifests are read through `facts::confined_file` + the existing 4 MiB evidence cap, cached once per session, capped at 512 probe-equivalents, and their rows are charged against the same 16384-entry budget as directory inventories; an over-budget manifest grants nothing rather than a partial list.

The development rule now declares all four layouts, so `categories/dev.rs` holds no JSON reader and no directory joining at all: content signatures (Chromium leaves, agent browser profiles, electron-updater artifacts), the exact worktree algorithm and the catalog-driven loops remain. Labels, categories, recommendations and disposal match the removed Rust constructors.

## Alternatives considered

Leaving the loops in Rust keeps duplicate directory joining and cross-product iteration per host. Enumerating the editor's `globalStorage` children with a name prefix would read every extension directory and treat `tasks2` as a match. Expressing the lists through Rust constants or generated TOML copies preserves two sources of truth. Treating manifest contents as paths or as inventory (`read_dir` intersection) either grants traversal or loses the case where the record is newer than the last listing. Scripts, expressions, recursion and data-driven expansion of list values are all rejected: only declared names and declared manifest fields, bounded by a fixed product and a fixed read budget, become candidates.

## Consequences

New hosts, extensions, agent directories, precise cache files and manifest-driven layouts now need TOML and a fixture, not a code branch. The recorded gold fixture `rules/fixtures/development-layout-baseline.json` pins target, recommendation, operation, disposal and both labels for all four selections; two agent directories and one workspace-storage inventory are read once per scan, a declared path costs a probe, and manifest rows are charged to the shared entry budget. `rules/fixtures/development-layout-extra.toml` and `rules/fixtures/manifest-layout-extra.toml` prove host × extension, `logs*.sqlite`, `.obsolete` and orphaned-workspace layouts appear with no Rust change.

The provider-policy test now asserts both directions of the migration: a provider-discovered updater artifact still follows its named policy (recommendation and disposal), while the migrated editor manifest layout keeps its rule-declared policy regardless of engine policy changes.

Hand editing rule TOML has one non-obvious failure mode: a `#` comment that ends up on the same line as a following `[[table]]` header swallows that header, and the loader then reports a duplicate key in an unrelated table (observed as `duplicate key 'path' in table 'catalogs.vscode_family'`). Always keep a blank line after a comment block that precedes a table, and run `cargo run --example rules -- check` after any rule edit; whole-bundle validation is what turns that class of mistake into a loud failure.

Remaining scope normalization, native facts, UI review, release closure, performance comparison and macOS acceptance are unchanged.

## Verification

- `src/core/categories/dev.rs::development_directory_layouts_preserve_baseline_and_share_probes`
- `src/core/categories/dev.rs::agent_log_databases_are_listed_but_never_recommended`
- `src/core/categories/dev.rs::vscode_task_storage_follows_declared_hosts_and_extensions`
- `src/core/categories/dev.rs::obsolete_entries_without_directories_are_skipped`
- `src/core/categories/dev.rs::obsolete_entries_with_path_separators_are_rejected`
- `src/core/categories/dev.rs::obsolete_scan_covers_vscode_forks_not_just_vscode`
- `src/core/categories/dev.rs::orphaned_workspaces_follow_recorded_local_paths_only`
- `src/core/categories/mod.rs::runtime_provider_policy_changes_discovery_and_keeps_frozen_extent`
- `src/core/rules/directories.rs::path_templates_stay_declarative_and_bounded`
- `src/core/rules/directories.rs::manifest_selections_stay_literal_and_bounded`
- `src/core/rules/directories.rs::manifest_reads_and_rows_share_the_session_budget`
- `src/core/rules/directories.rs::templates_expand_declared_sources_once_per_scan`

This architecture migration claims no organic bug-fix red proof. The already-built `rules.exe` also interpreted the fixture rules without a Rust edit: `rules/fixtures/development-layout-extra.toml` reported two `contents` plans plus one exact `file` plan, and `rules/fixtures/manifest-layout-extra.toml` reported one manifest-selected extension directory plus one orphaned workspace directory, both with `blocked=[]`. Reports are preserved as docs/agent-notes-evidence/2026-10-05-development-layout-extra-explain.json and docs/agent-notes-evidence/2026-10-05-manifest-layout-extra-explain.json. Actual unified gate results and limitations are recorded in RULES_REFACTOR_STATUS.
