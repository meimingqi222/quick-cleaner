package core

import (
	"context"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
)

func buildProjectFixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	if !IsDevelopmentInstallationPath(root) {
		return root
	}
	// Windows TEMP normally lives in AppData, which production discovery must
	// reject. Put only these isolated project fixtures outside that boundary.
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatal(err)
	}
	root, err = os.MkdirTemp(home, ".quickcleaner-test-project-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := os.RemoveAll(root); err != nil {
			t.Error(err)
		}
	})
	return root
}

func TestBuildDiscoveryRequiresProjectEvidenceInWalkAndIndex(t *testing.T) {
	root := buildProjectFixture(t)
	write := func(relative, data string) {
		t.Helper()
		path := filepath.Join(root, relative)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(data), 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, prefix := range []string{"project", ".nvm/versions/node/lib", "Tool.app/Contents/Resources/app", "Library/Application Support/Tool/runtime", ".local/share/uv/tools/cli"} {
		write(prefix+"/package.json", `{}`)
		write(prefix+"/node_modules/pkg/target/data", "not-a-separate-target")
		write(prefix+"/node_modules/pkg/Cargo.toml", "[package]")
	}
	write("rust/Cargo.toml", "[package]")
	write("rust/target/data", "rust output")
	write("ordinary/target/data", "user data")
	write("dotnet/Project.CSPROJ", "project")
	write("dotnet/bin/data", "dotnet output")
	write("frontend/package.json", `{"scripts":{"build":"tool build"}}`)
	write("frontend/src/source", "source")
	write("frontend/dist/data", "frontend output")
	write("not-frontend/package.json", `{}`)
	write("not-frontend/dist/data", "user data")
	write("python/pyproject.toml", "project")
	write("python/.venv/pyvenv.cfg", "home = /python")
	write("unverified/pyproject.toml", "project")
	write("unverified/.venv/data", "not-proven")
	write("malformed/package.json", `[]`)
	write("malformed/node_modules/data", "not-proven")
	write("tagged/CACHEDIR.TAG", "Signature: 8a477f597d28d172789f06886806bc55\ncomment")
	write("forged/CACHEDIR.TAG", "Signature: deadbeef")
	write("forged/user-data", "keep")
	write(".git/target/data", "keep")
	write(".git/Cargo.toml", "keep")
	write("missing-manifest/node_modules/CACHEDIR.TAG", "Signature: 8a477f597d28d172789f06886806bc55")
	write("empty/.pytest_cache/CACHEDIR.TAG", "")
	snapshot, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	var entries []TreeSnapshotEntry
	if err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		md, err := entry.Info()
		if err != nil {
			return err
		}
		size := uint64(0)
		if !md.IsDir() {
			size = allocatedBytes(md)
		}
		entries = append(entries, TreeSnapshotEntry{Path: path, IsDir: md.IsDir(), Size: size})
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	tree := SizeTreeFromSnapshot(VolumeFromMountPoint(root), entries)
	expected := []string{"dotnet/bin", "frontend/dist", "project/node_modules", "python/.venv", "rust/target", "tagged"}
	for _, channel := range []struct {
		name string
		tree *SizeTree
	}{{"walk", nil}, {"index", tree}} {
		t.Run(channel.name, func(t *testing.T) {
			items := DiscoverBuildArtifacts(context.Background(), snapshot, []CodeRoot{{root, 10}, {filepath.Join(root, "project"), 10}}, channel.tree)
			var got []string
			for _, item := range items {
				rel, err := filepath.Rel(root, item.Path)
				if err != nil {
					t.Fatal(err)
				}
				got = append(got, filepath.ToSlash(rel))
				if item.Recommended || item.Category != DevBuild || item.Operation.Kind != OpTree || item.Disposal != Permanent || item.Identity == nil || item.Size == 0 || len(item.Plans) != 1 || item.Plans[0].Validate() != nil {
					t.Fatalf("scan contract: %+v", item)
				}
				if item.Label.Get(Chinese) == item.Label.Get(English) || !strings.Contains(item.Label.Get(English), ` · …\`) {
					t.Fatalf("build label: %+v", item.Label)
				}
			}
			sort.Strings(got)
			if !reflect.DeepEqual(got, expected) {
				t.Fatalf("build candidates = %v; want %v", got, expected)
			}
			ctx, cancel := context.WithCancel(context.Background())
			cancel()
			if items := DiscoverBuildArtifacts(ctx, snapshot, []CodeRoot{{root, 10}}, channel.tree); len(items) != 0 {
				t.Fatal("cancelled discovery did new work")
			}
		})
	}
	// Tree evidence cannot substitute for the live project's package manifest.
	if err := os.WriteFile(filepath.Join(root, "project/package.json"), []byte("broken"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, item := range DiscoverBuildArtifacts(context.Background(), snapshot, nil, tree) {
		if item.Path == filepath.Join(root, "project/node_modules") {
			t.Fatal("stale indexed project evidence authorized dependencies")
		}
	}
}

func TestCodeRootsAndDiscoveryDepth(t *testing.T) {
	home := buildProjectFixture(t)
	for _, path := range []string{"code/project/target", "nested/deep/project/target"} {
		path := filepath.Join(home, path)
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(filepath.Dir(path), "Cargo.toml"), []byte("[package]"), 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(path, "data"), []byte("output"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if roots := CodeRoots("", nil); len(roots) != 0 {
		t.Fatalf("untrusted process home substituted: %v", roots)
	}
	if roots := CodeRoots(home, nil); !reflect.DeepEqual(roots, []CodeRoot{{home, 2}, {filepath.Join(home, "code"), 6}}) {
		t.Fatalf("root depths: %v", roots)
	}
	s, err := CurrentRules()
	if err != nil {
		t.Fatal(err)
	}
	items := DiscoverBuildArtifacts(context.Background(), s, CodeRoots(home, nil), nil)
	if len(items) != 1 || items[0].Path != filepath.Join(home, "code/project/target") {
		t.Fatalf("bounded roots: %+v", items)
	}
}
