# QuickCleaner

[English](README.md) | [中文说明](README_zh.md)

QuickCleaner is written in **Go + MyGo**. The original Rust + GPUI
implementation was retired and removed on 2026-11-01; the Go version kept its
UI and interactions. The product entry point is
`go run ./cmd/quick-cleaner --no-elevate`; a separate non-destructive UI gallery
is available at `go run ./cmd/ui-gallery`. Native acceptance is still in
progress; see [migration status](GO_MIGRATION.md).

> **Native Windows / macOS disk space analyzer & system cleaner**  
> Built with **Go + MyGo**, zero WebView / Electron overhead.

---

## Key Features

### 1. NTFS $MFT Raw Sector Disk Analyzer (Disk Lens)
- **Fast Whole-Drive Indexing**: Directly parses the NTFS Master File Table (`$MFT`) without recursive filesystem walks. Indexes millions of files in 3-5 seconds.
- **Hierarchical Storage Lens**: Interactive charts and drillable directory trees to quickly pinpoint space-hogging large files and bloated folders.
- **Multi-Volume Support**: Switch between NTFS volumes (C:, D:, E:, etc.), automatically detecting total capacity and usage breakdowns.

### 2. CleanFlow Smart Junk Cleaning
- **Comprehensive Coverage Across 10 Categories**:
  - System temporary files / User temp files / Thumbnail caches
  - Major browser caches (Chrome, Edge, Firefox, etc.)
  - Developer package manager caches (Cargo, npm, pnpm, Yarn, pip, Gradle, Go build, etc.)
  - Logs and crash dumps (Minidumps, Windows Error Reporting)
  - Safe Recycle Bin cleanup (invokes Windows native `SHEmptyRecycleBinW`, preserving volume metadata)
  - **AI Assistant & Agent Caches** (Claude, Cursor, Antigravity session & model caches)
  - **Project Build Artifacts & Dependencies** (`node_modules`, `target`, `.venv`, `bin/obj` in code directories; safely unselected by default)
- **Clear Contents, Retain Directory Anchors**: Cleans internal junk files while keeping essential directory nodes intact, ensuring stable system and application behavior.

### 3. Application Manager & Deep Residual Cleanup
- **Complete Installed Software Inventory**: Enumerates 64-bit and 32-bit Windows registry uninstall keys, displaying installed size, installation date, and last usage.
- **Full Lifecycle Residual Tracking**:
  - **Pre-uninstall snapshot**: Collects associated ProgramData, AppData, registry paths, services, and autostart traces before uninstallation;
  - **Official Uninstaller Execution**: Runs standard vendor uninstallers and monitors completion;
  - **Post-uninstall verification**: Compares remaining files/registry keys, surfacing orphaned configuration and cache files for one-click deep cleanup.

### 4. Robust Safety & Protection System
- **Deletion is permanent — nothing goes to the Recycle Bin.** This is a deliberate decision, not an oversight: QuickCleaner elevates itself through UAC, and when the administrator account differs from the signed-in user, `FOF_ALLOWUNDO` would drop files into the *administrator's* Recycle Bin where the user can never find them. On top of that, moving tens of gigabytes of cache into the Recycle Bin frees no space at all. What you delete here is gone, so every confirmation dialog says so explicitly.
- **Four steps before anything is removed**: scan → preview the exact paths → tick what you want → confirm. Developer-oriented categories (AI caches, build artifacts, agent worktrees) are never pre-selected.
- **Core System Protection**: One single source of truth for path rules (`internal/core/safety.go`) guards volume roots, `%SystemRoot%`, `System32`, `WinSxS`, `Program Files`, `ProgramData`, user home and NTFS metadata. They stay browsable and analyzable, but cannot be deleted. Deletion never follows symlinks or junctions.
- **Smart UAC Elevation**: Automatically prompts for administrator privileges on startup to unlock direct raw sector NTFS $MFT parsing and deep cleaning. Supports `--no-elevate` flag for standard user execution.

### 5. Modern Native Desktop Experience & Internationalization
- **Native UI**: Built with the MyGo immediate-mode UI toolkit; no WebView or Electron runtime.
- **Two-Phase Scan**: The system-junk categories appear in about a second so the app is usable immediately; the expensive whole-disk hunt for build artifacts continues in the background and merges in when it finishes. (Measured on the dev machine: 33s before, first results in under 3s now.)
- **Remembers Your Choices**: Language is detected from the Windows display language on first launch (Chinese system → Chinese, anything else → English) and persisted to `%APPDATA%\QuickCleaner\settings.json` once you pick one.
- **Complete Multilingual Support**: Every string — including the status bar, scan result labels and residual source badges — is bilingual (English / `中文`), switchable live from the sidebar pill. Scan results carry both languages with them, so switching never triggers a re-scan.
- **Refined Material / Fluent Aesthetic**: Free of excessive decorations or generic AI tropes, featuring a clean color token system and PerMonitorV2 High-DPI screen scaling.

---

## Architecture

QuickCleaner follows a strict unidirectional layered architecture,
`ui → core → platform`:

```
quick-cleaner/
├── cmd/
│   ├── quick-cleaner/          # Product entry point & UAC self-elevation
│   └── ui-gallery/             # Non-destructive UI fixtures
├── internal/
│   ├── app/                    # Controller: state, workers, actions
│   ├── core/                   # Domain logic: scanning, cleaning, safety.go, rules
│   ├── platform/               # OS layer (Windows $MFT, registry, processes; macOS)
│   └── ui/                     # MyGo views, theme and i18n
├── rules/                      # Declarative cleanup rules (TOML)
└── tools/                      # indexbench, winres, regression-notes
```

---

## 🚀 Building & Running

### Prerequisites
- **Go** 1.27.1 or newer (no cgo)
- **Windows 10 / 11** (run as administrator for direct `$MFT` reading) or **macOS**

### Build, test and package

```bash
git clone https://github.com/meimingqi222/quick-cleaner.git
cd quick-cleaner

go test ./...
go run ./cmd/quick-cleaner --no-elevate      # not a dry run: actions still modify files
go run ./tools/winres                        # after changing internal/app Version
pwsh ./scripts/package-windows.ps1           # dist/quick-cleaner-windows-x86_64.zip
./scripts/package-macos.sh dist universal    # dist/QuickCleaner.app (on macOS)
```

---

## Quality & Testing

`.github/workflows/go.yml` runs `gofmt`, `go vet`, `go test -race` and
`go build` on Windows, macOS and Linux, and checks the Windows resources.
The suite covers NTFS `$MFT` decoding, path and registry safety rules, residual
matching, rule validation, and cleanup progress reporting.

```bash
go test -race ./...
```

---

## License

This project is licensed under the [MIT License](LICENSE).
