package core

import "testing"

func TestDockerDiscoverySelectionPreservesReferencesAndVersionRules(t *testing.T) {
	images := ParseDockerImages("sha256:aaaaaaaaaaaa|app|1.9|<none>|187MB\n" +
		"bbbbbbbbbbbb|app|1.10|<none>|1.24GB\n" +
		"cccccccccccc|app|1.10-alpine|<none>|823kB\n" +
		"dddddddddddd|busy|latest|<none>|1B\n" +
		"dddddddddddd|busy|<none>|sha256:abc123|1B\n" +
		"eeeeeeeeeeee|<none>|<none>|<none>|2.5TB\n" +
		"ffffffffffff|ubuntu|22.04|<none>|1MB\n" +
		"111111111111|ubuntu|22.4|<none>|1MB\n" +
		"222222222222|ubuntu|20.04|<none>|1MB\n" +
		"aaaaaaaaaaaa|app|1.9|<none>|187MB\ninvalid inventory\n")
	if len(images) != 10 || images[0].ID != "aaaaaaaaaaaa" || images[0].Size != 187000000 || images[1].Size != 1240000000 || images[2].Size != 823000 || images[5].Size != 2500000000000 {
		t.Fatalf("inventory parse: %+v", images)
	}
	for _, ref := range []string{"busy@sha256:abc123", "busy", "sha256:dddddddd"} {
		junk := SelectDockerJunk(images, []string{ref})
		want := map[string]DockerJunkKind{"eeeeeeeeeeee": DockerDangling, "app:1.9": DockerOldVersion, "app:1.10": DockerUnreferenced, "app:1.10-alpine": DockerUnreferenced, "ubuntu:20.04": DockerOldVersion, "ubuntu:22.04": DockerUnreferenced, "ubuntu:22.4": DockerUnreferenced}
		if len(junk) != len(want) {
			t.Fatalf("container ref %q: %+v", ref, junk)
		}
		for _, item := range junk {
			kind, exists := want[item.RmiRef()]
			if !exists || kind != item.Kind {
				t.Fatalf("unexpected selection: %+v", item)
			}
			delete(want, item.RmiRef())
		}
		if len(want) != 0 {
			t.Fatal("duplicate virtual references or missing selections")
		}
	}
	if got := (DockerJunk{Image: images[4]}).RmiRef(); got != "busy@sha256:abc123" {
		t.Fatal(got)
	}
	if got := SelectDockerJunk(images[5:6], []string{"eeeeee"}); len(got) != 0 {
		t.Fatal("dangling image in use must not be selected")
	}
}
