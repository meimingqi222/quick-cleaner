// Package rules exposes the reviewed TOML declarations unchanged.
package rules

import "embed"

// Files contains only the source declarations, never generated policy or fixtures.
//
//go:embed *.toml
var Files embed.FS
