# Orb lifecycle

- QuickCleaner has Windows and macOS implementations only. A Linux orb can run
  `gofmt`, `go vet` and the Python guards, but native builds and application
  tests are not supported there. Use a native runner for those checks.
- `.agents/setup` installs the Go version pinned by `go.mod` into
  `~/.local/go`, plus Debian prerequisites, and warms the module cache. It needs
  no secrets: the former GPUI kit checkout and `GPUI_APP_KIT_TOKEN` went away
  with the Rust tree on 2026-11-01.
- Run `.agents/setup` twice to check warm idempotence. `.agents/resume` only checks
  restored tools; it does not download dependencies or start services.
