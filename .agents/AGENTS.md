# Orb lifecycle

- QuickCleaner has Windows and macOS implementations only. A Linux orb can run
  formatting and Python guards, but installing GTK does not make native builds or
  application tests supported. Use a native runner for those checks.
- `.agents/setup` follows CI's Rust stable channel, installs rustfmt/clippy and
  Debian prerequisites, prepares the sibling GPUI kit at the manifest revision,
  and caches Cargo.lock dependencies for both Mac architectures and Windows MSVC.
- Cold setup requires the **project-scoped secret** `GPUI_APP_KIT_TOKEN`, with
  read-only Contents access to `meimingqi222/gpui-app-kit`. Supply it through Amp
  Settings, never a source file or transcript. Setup does not receive a personal
  GitHub login. Headers are process-local and are not saved to Git configuration.
- Run `.agents/setup` twice to check warm idempotence. `.agents/resume` only checks
  restored tools; it does not download dependencies or start services.
