// Package cli exposes headless product operations. Diagnostic output is not an
// execution capability: every mutation starts from a fresh in-memory scan.
package cli

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"time"
)

const (
	ExitOK         = 0
	ExitError      = 1
	ExitUsage      = 2
	ExitIncomplete = 3
	ExitTimeout    = 124
	ExitCancelled  = 130
)

const help = `QuickCleaner headless commands:
  quick-cleaner cli junk scan [options]
  quick-cleaner cli junk plan [options]
  quick-cleaner cli junk clean [options] [--execute]
  quick-cleaner cli junk categories [--json]
  quick-cleaner cli disk analyze (--root DIR | --volume MOUNT) [options]
  quick-cleaner cli search (--root DIR | --volume MOUNT) --query PATTERN [options]

Use COMMAND --help for options. JSON goes to stdout; diagnostics go to stderr.
Junk clean previews by default. --execute additionally requires an explicit
--path, --recommended or --all selection. Paths must be fresh scan results.
No command requests elevation or terminates processes automatically.
Exit codes: 0 complete, 1 error, 2 usage, 3 partial/unresolved,
124 timeout, 130 cancelled.
`

// Run never initializes a window, changes elevation or acquires a GUI instance.
func Run(ctx context.Context, args []string, out, errOut io.Writer) int {
	if len(args) == 0 || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
		if _, err := fmt.Fprint(out, help); err != nil {
			fmt.Fprintln(errOut, "quick-cleaner:", err)
			return ExitError
		}
		return ExitOK
	}
	var code int
	var err error
	switch args[0] {
	case "junk":
		code, err = runJunk(ctx, args[1:], out, errOut)
	case "disk":
		if len(args) < 2 || args[1] != "analyze" {
			code, err = ExitUsage, errors.New("expected: disk analyze; use --help")
		} else {
			code, err = runTree(ctx, false, args[2:], out, errOut)
		}
	case "search":
		code, err = runTree(ctx, true, args[1:], out, errOut)
	default:
		code, err = ExitUsage, fmt.Errorf("unknown command %q; use --help", args[0])
	}
	if err != nil {
		fmt.Fprintln(errOut, "quick-cleaner:", err)
		if code == ExitOK {
			code = ExitError
		}
	}
	return code
}

func flags(name string, errOut io.Writer) *flag.FlagSet {
	f := flag.NewFlagSet(name, flag.ContinueOnError)
	f.SetOutput(errOut)
	return f
}

func parse(f *flag.FlagSet, args []string) (int, error) {
	if err := f.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return ExitOK, err
		}
		return ExitUsage, err
	}
	if f.NArg() != 0 {
		return ExitUsage, fmt.Errorf("unexpected arguments: %v", f.Args())
	}
	return ExitOK, nil
}

func contextCode(ctx context.Context) int {
	if errors.Is(ctx.Err(), context.DeadlineExceeded) {
		return ExitTimeout
	}
	if ctx.Err() != nil {
		return ExitCancelled
	}
	return ExitOK
}

func writeJSON(out io.Writer, value any) error {
	e := json.NewEncoder(out)
	e.SetIndent("", "  ")
	return e.Encode(value)
}

type commonOptions struct {
	json    bool
	timeout time.Duration
}

func (o *commonOptions) bind(f *flag.FlagSet) {
	f.BoolVar(&o.json, "json", false, "write a versioned JSON report")
	f.DurationVar(&o.timeout, "timeout", 5*time.Minute, "operation timeout, e.g. 30s or 5m")
}

type stringsFlag []string

func (s *stringsFlag) String() string { return fmt.Sprint([]string(*s)) }
func (s *stringsFlag) Set(value string) error {
	if value == "" {
		return errors.New("empty selection")
	}
	*s = append(*s, value)
	return nil
}
