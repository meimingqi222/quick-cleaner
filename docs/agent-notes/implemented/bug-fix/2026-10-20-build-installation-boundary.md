# Agent Note: Installed dependencies are not rebuildable project garbage

Status: implemented

## Problem

The build marker matched every node_modules by name. A real cleanup offered
`.nvm/versions/node/v22.22.1/lib/node_modules` (2.19 GB) as build garbage. OpenCode
was executing a binary inside it, so occupancy happened to prevent deletion. Closing
OpenCode would remove that incidental protection and expose npm and other installed
CLIs to whole-directory deletion. A package.json alone cannot distinguish a project
from an application or a global package installation.

## Decision

The installation predicate lives in core safety and is consumed by one shared
candidate check in all three discovery channels: walk, macOS SizeTree and Windows
MFT. App bundles, application-support trees, global Node installations and managed
tool/interpreter environments are excluded from generic build discovery. Nested
node_modules and environment trees remain opaque even without project evidence.
This predicate classifies discovery; it does not prohibit the owner's dedicated
uninstall route or authorize a different deletion operation.

Node and framework outputs need a readable JSON object in the sibling package.json.
A dist directory also needs source and an explicit build script. Project venv names
need a project manifest and pyvenv.cfg. CACHEDIR.TAG cannot override installation
ownership. Rejected marker candidates are not recursively reinterpreted as projects.
All candidates remain manually selected, with the existing identity and safety gates.

## Alternatives considered

Adding only package.json would still accept installed tools. Ending the occupying
process would make the unsafe classification actionable. Copying installation
exclusions into each scanner would allow the three channels to drift. Treating
installation exclusions as a global delete blacklist would break official tool
removal. All rejected.

## Consequences

Some unverified directories no longer appear as build garbage. Global packages remain
in the developer environment manager, whose inventory, prefix and official-command
preflight stay authoritative. No package manager, installed program or real user
cache was deleted during validation. Filesystem checks occur only for marker/tag
candidates; the indexed traversal does not stat every directory's ancestors.

## Verification

- `src/core/devscan.rs::installation_dependencies_are_not_build_garbage`
- `src/core/devscan.rs::build_candidates_require_project_and_environment_evidence`
- `src/core/devscan.rs::collect_tree_finds_same_markers_as_collect`

Proved: before adding the candidate guard, installation_dependencies_are_not_build_garbage
failed because the walk results included global and application-owned node_modules
and an unverified venv instead of only the project dependency directory. Output:
`docs/agent-notes-evidence/2026-10-20-build-installation-red.log`. The same focused test
passes after the fix, including the macOS tree assertion:
`docs/agent-notes-evidence/2026-10-20-build-installation-green.log`.

Pitfalls checked: P15 raw database evidence is unchanged; P19/P25/P28 identity and
plan constraints remain intact; P20/P38 global assets keep their owner-command route;
P22/P24 managed worktrees remain protected; P29 recursive occupancy checks remain;
P45 venv deletion still goes through the existing cleaner. No official-uninstall,
residual-name, ACL, pending-reboot or command-line parsing logic was changed here.
