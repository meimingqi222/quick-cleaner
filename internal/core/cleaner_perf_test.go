package core

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func BenchmarkDeleteTreeSmallFiles(b *testing.B) {
	for _, directories := range []int{1, 32} {
		b.Run(fmt.Sprintf("dirs%d", directories), func(b *testing.B) {
			base := b.TempDir()
			for i := 0; i < b.N; i++ {
				b.StopTimer()
				root := filepath.Join(base, fmt.Sprint(i))
				for d := 0; d < directories; d++ {
					dir := filepath.Join(root, fmt.Sprint(d))
					if err := os.MkdirAll(dir, 0700); err != nil {
						b.Fatal(err)
					}
					for f := 0; f < 2048/directories; f++ {
						if err := os.WriteFile(filepath.Join(dir, fmt.Sprint(f)), []byte("cache"), 0600); err != nil {
							b.Fatal(err)
						}
					}
				}
				p := NewCleanProgress(2048, 0)
				b.StartTimer()
				result := DeleteTree(root, p)
				b.StopTimer()
				if result != CleanOK || p.Files.Load() != 2048 {
					b.Fatalf("result=%v progress=%+v", result, p.Snapshot())
				}
				if _, err := os.Lstat(root); !os.IsNotExist(err) {
					b.Fatalf("root remains: %v", err)
				}
			}
		})
	}
}
