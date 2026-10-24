package core

import (
	"errors"
	"strings"
	"time"
	"unicode/utf8"
)

const ownerCommandTimeout = 120 * time.Second

func IsGoModcache(path string) bool { return strings.HasSuffix(Norm(path), `\go\pkg\mod`) }
func IsPnpmStore(path string) bool {
	n := Norm(path)
	return strings.HasSuffix(n, `\pnpm\store`) || strings.HasSuffix(n, `\.pnpm-store`)
}

type preparedOwner struct {
	operation Operation
	target    string
}

func prepareOwner(target string, operation Operation) *preparedOwner {
	var tool string
	var args []string
	switch operation.Kind {
	case OpGo:
		tool = "go"
		args = []string{"env", "GOMODCACHE"}
	case OpPnpm:
		tool = "pnpm"
		args = []string{"store", "path"}
	default:
		return nil
	}
	result, ok := RunToolWithTimeout(tool, args, 5*time.Second)
	if !ok || !result.OK || !utf8.Valid(result.Stdout) {
		return nil
	}
	path := strings.TrimSpace(string(result.Stdout))
	owner, selected := Norm(path), Norm(target)
	if path == "" || !(owner == selected || operation.Kind == OpPnpm && strings.HasPrefix(owner, selected+`\`)) {
		return nil
	}
	return &preparedOwner{operation: operation, target: target}
}

func (o *preparedOwner) apply() error {
	var tool string
	var args []string
	switch o.operation.Kind {
	case OpGo:
		tool = "go"
		args = []string{"clean", "-modcache"}
	case OpPnpm:
		tool = "pnpm"
		args = []string{"store", "prune"}
	default:
		return errors.New("Unsupported owner capability")
	}
	result, ok := RunToolWithTimeout(tool, args, ownerCommandTimeout)
	if !ok {
		return errors.New("Owner cleanup unavailable or timed out after preparation")
	}
	if !result.OK {
		return errors.New("Owner cleanup failed: " + strings.TrimSpace(string(result.Stderr)))
	}
	return nil
}

func (o *preparedOwner) completion() Evidence {
	switch o.operation.Kind {
	case OpGo:
		return FilesystemCompletion(CompletionCondition{Kind: "path_absent", Path: o.target})
	case OpPnpm:
		result, ok := RunToolWithTimeout("pnpm", []string{"store", "status"}, ownerCommandTimeout)
		if !ok {
			return Unknown
		}
		if result.OK {
			return Confirmed
		}
		return Absent
	}
	return Unknown
}
