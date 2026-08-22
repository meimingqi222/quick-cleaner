# QuickCleaner

[English](README.md) | [中文说明](README_zh.md)

> **Native, multi-threaded Windows disk space analyzer & system cleaner**  
> Built with **Rust + GPUI**, zero WebView / Electron overhead, millisecond-level responsiveness, and GPU-accelerated rendering.

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
- **Core System Protection**: One single source of truth for path rules (`core/safety.rs`) guards volume roots, `%SystemRoot%`, `System32`, `WinSxS`, `Program Files`, `ProgramData`, user home and NTFS metadata. They stay browsable and analyzable, but cannot be deleted. Deletion never follows symlinks or junctions.
- **Smart UAC Elevation**: Automatically prompts for administrator privileges on startup to unlock direct raw sector NTFS $MFT parsing and deep cleaning. Supports `--no-elevate` flag for standard user execution.

### 5. Modern Native Desktop Experience & Internationalization
- **GPUI High-Performance Native UI**: Powered by Zed's GPUI rendering engine in pure Rust. Delivers sub-millisecond cold starts and smooth 60/120fps micro-animations.
- **Two-Phase Scan**: The system-junk categories appear in about a second so the app is usable immediately; the expensive whole-disk hunt for build artifacts continues in the background and merges in when it finishes. (Measured on the dev machine: 33s before, first results in under 3s now.)
- **Remembers Your Choices**: Language is detected from the Windows display language on first launch (Chinese system → Chinese, anything else → English) and persisted to `%APPDATA%\QuickCleaner\settings.json` once you pick one.
- **Complete Multilingual Support**: Every string — including the status bar, scan result labels and residual source badges — is bilingual (English / `中文`), switchable live from the sidebar pill. Scan results carry both languages with them, so switching never triggers a re-scan.
- **Refined Material / Fluent Aesthetic**: Free of excessive decorations or generic AI tropes, featuring a clean color token system and PerMonitorV2 High-DPI screen scaling.

---

## Architecture

QuickCleaner follows a strict unidirectional layered architecture:

```
quick-cleaner/
├── examples/
│   └── mftscan.rs              # Windows-only $MFT verification (not shipped)
├── src/
│   ├── main.rs                 # Entry point & UAC self-elevation handler
│   ├── lib.rs                  # Module declarations
│   ├── core/                   # Pure domain logic (independent of GPUI & OS specifics)
│   │   ├── i18n.rs             # Core multilingual definitions (Language enum)
│   │   ├── categories.rs       # 10 junk categories & scan definitions
│   │   ├── devscan.rs          # Discovery scan for build artifacts (MFT / walk)
│   │   ├── scanner.rs          # Parallel directory scanner with walkdir + rayon
│   │   ├── cleaner.rs          # Cleanup executor with atomic progress tracking
│   │   ├── safety.rs           # Single source of truth for path safety rules
│   │   ├── apps.rs             # App models and residual analyzers
│   │   ├── disk.rs             # Disk tree models & selection state machine
│   │   ├── log.rs              # File logging (%APPDATA%\QuickCleaner\quick-cleaner.log)
│   │   └── model.rs            # Data formatting & shared tri-state models
│   │
│   ├── platform/               # OS abstraction layer (enforced by platform_contract!)
│   │   ├── mod.rs              # Unified platform facade
│   │   ├── windows/            # Windows native implementation ($MFT, Registry, Process, UAC)
│   │   └── macos/              # macOS baseline cross-platform compatibility
│   │
│   └── ui/                     # GPUI views and interaction layer
│       ├── mod.rs              # Root state machine & derived cache management
│       ├── i18n.rs             # UI-level internationalization dictionaries
│       ├── theme.rs            # Color tokens and design system metrics
│       ├── components/         # Reusable widgets (buttons, cards, scrollbars, dialogs)
│       └── views/              # Main views (dashboard, junk, apps, disk)
```

---

## 🚀 Building & Running

### Prerequisites
- **Rust** 1.75 or newer (the `stable-x86_64-pc-windows-msvc` toolchain is recommended)
- **Windows 10 / 11** (Run with administrator privileges for full direct `$MFT` reading support)

### Local Compilation & Development

```bash
# 1. Clone the repository
git clone https://github.com/meimingqi222/quick-cleaner.git
cd quick-cleaner

# 2. Run automated test suite (NTFS parser, safety boundaries, residual matching, etc.)
cargo test

# 3. Launch debug build
cargo run

# 4. Build optimized release binary
cargo build --release
```

The compiled binary will be located at `target/release/quick-cleaner.exe`.

### Windows `$MFT` verification (dev only)

Not a product binary, and not built on macOS. Enable the `mftscan` feature to compare a volume against WizTree:

```bash
cargo run --example mftscan --features mftscan -- C 20
```

---

## Quality & Testing

QuickCleaner maintains a comprehensive automated testing suite (102 unit tests, enforced in CI together with `cargo clippy --all-targets -- -D warnings`) covering:
- NTFS `$MFT` record decoding, fixup tail validation, non-resident data fragment reconstruction, and tree assembly;
- Path and registry security rules, preventing destructive operations on critical system directories;
- App uninstaller residual fuzzy-matching and confidence scoring algorithms;
- Parallel multi-threaded scanning and atomic cleanup progress reporting.

Run all tests with:
```bash
cargo test
```

---

## License

This project is licensed under the [MIT License](LICENSE).
