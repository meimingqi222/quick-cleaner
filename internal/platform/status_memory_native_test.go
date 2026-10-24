//go:build windows || darwin || linux

package platform

import (
	"context"
	"errors"
	"testing"
)

func TestStatusMemoryNativeRead(t *testing.T) {
	reading, err := ReadStatusMemory(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if reading.Total == 0 || reading.Used > reading.Total || reading.SwapUsed > reading.SwapTotal {
		t.Fatalf("invalid native reading: %+v", reading)
	}
	uptime, err := SystemUptimeSecs(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if uptime == 0 {
		t.Fatal("expected positive uptime")
	}
}

func TestStatusMemoryCanceledContext(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := ReadStatusMemory(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("memory: %v", err)
	}
	if _, err := SystemUptimeSecs(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("uptime: %v", err)
	}
}
