//go:build !windows

package app

import "github.com/egoist/mygo"

func prepareStartupWindow(*mygo.WindowOptions) {}

func finishStartupWindow(*mygo.Window) {}
