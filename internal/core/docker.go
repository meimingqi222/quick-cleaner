package core

import (
	"errors"
	"math"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

const dockerNone = "<none>"
const dockerImagesFormat = "{{.ID}}|{{.Repository}}|{{.Tag}}|{{.Digest}}|{{.Size}}"

type DockerImage struct {
	ID, Repository, Tag, Digest string
	Size                        uint64
}

type DockerJunkKind uint8

const (
	DockerDangling DockerJunkKind = iota
	DockerOldVersion
	DockerUnreferenced
)

type DockerJunk struct {
	Image DockerImage
	Kind  DockerJunkKind
}

func (j DockerJunk) RmiRef() string {
	if j.Image.Tag == dockerNone {
		if j.Image.Repository != dockerNone && j.Image.Digest != dockerNone {
			return j.Image.Repository + "@" + j.Image.Digest
		}
		return j.Image.ID
	}
	return j.Image.Repository + ":" + j.Image.Tag
}

func ParseDockerSize(text string) uint64 {
	text = strings.TrimSpace(text)
	i := strings.IndexFunc(text, func(c rune) bool { return !(c >= '0' && c <= '9' || c == '.') })
	if i < 0 {
		i = len(text)
	}
	value, _ := strconv.ParseFloat(text[:i], 64)
	multiplier := float64(1)
	switch strings.TrimSpace(text[i:]) {
	case "kB", "KB":
		multiplier = 1e3
	case "MB":
		multiplier = 1e6
	case "GB":
		multiplier = 1e9
	case "TB":
		multiplier = 1e12
	}
	value *= multiplier
	if value >= math.MaxUint64 {
		return math.MaxUint64
	}
	return uint64(value)
}

func ParseDockerImages(stdout string) []DockerImage {
	var out []DockerImage
	for line := range strings.SplitSeq(stdout, "\n") {
		fields := strings.Split(strings.TrimSuffix(line, "\r"), "|")
		if len(fields) < 5 || fields[0] == "" || fields[1] == "" {
			continue
		}
		id := fields[0]
		for strings.HasPrefix(id, "sha256:") {
			id = strings.TrimPrefix(id, "sha256:")
		}
		out = append(out, DockerImage{ID: id, Repository: fields[1], Tag: fields[2], Digest: fields[3], Size: ParseDockerSize(fields[4])})
	}
	return out
}

func dockerVersionTag(tag string) ([]uint64, bool) {
	var version []uint64
	for part := range strings.SplitSeq(tag, ".") {
		n, err := strconv.ParseUint(strings.TrimPrefix(part, "+"), 10, 64)
		if err != nil {
			return nil, false
		}
		version = append(version, n)
	}
	return version, true
}

func dockerImageReferenced(image DockerImage, refs []string) bool {
	for _, ref := range refs {
		ref = strings.TrimSpace(ref)
		if ref == "" {
			continue
		}
		if ref == image.Repository+":"+image.Tag || image.Tag == "latest" && ref == image.Repository || image.Digest != dockerNone && ref == image.Repository+"@"+image.Digest {
			return true
		}
		id := strings.TrimPrefix(ref, "sha256:")
		if len(id) >= 6 && len(image.ID) >= 6 && (strings.HasPrefix(image.ID, id) || strings.HasPrefix(id, image.ID)) {
			return true
		}
	}
	return false
}

func SelectDockerJunk(images []DockerImage, refs []string) []DockerJunk {
	referenced := map[string]bool{}
	for _, image := range images {
		if dockerImageReferenced(image, refs) {
			referenced[image.ID] = true
		}
	}
	var out []DockerJunk
	taken := map[string]bool{}
	push := func(image DockerImage, kind DockerJunkKind) {
		item := DockerJunk{Image: image, Kind: kind}
		if !taken[item.RmiRef()] {
			taken[item.RmiRef()] = true
			out = append(out, item)
		}
	}
	byRepo := map[string][]DockerImage{}
	var repos []string
	for _, image := range images {
		if image.Repository == dockerNone {
			if !referenced[image.ID] {
				push(image, DockerDangling)
			}
			continue
		}
		if _, exists := byRepo[image.Repository]; !exists {
			repos = append(repos, image.Repository)
		}
		byRepo[image.Repository] = append(byRepo[image.Repository], image)
	}
	for _, repo := range repos {
		type versioned struct {
			image DockerImage
			parts []uint64
		}
		var versions []versioned
		for _, image := range byRepo[repo] {
			if version, ok := dockerVersionTag(image.Tag); ok {
				versions = append(versions, versioned{image, version})
			}
		}
		sort.SliceStable(versions, func(i, j int) bool { return slices.Compare(versions[i].parts, versions[j].parts) < 0 })
		if len(versions) < 2 {
			continue
		}
		newest := versions[len(versions)-1].parts
		for _, version := range versions {
			if !slices.Equal(version.parts, newest) && !referenced[version.image.ID] {
				push(version.image, DockerOldVersion)
			}
		}
	}
	for _, image := range images {
		if image.Repository != dockerNone && image.Tag != dockerNone && !referenced[image.ID] {
			push(image, DockerUnreferenced)
		}
	}
	return out
}

func DockerScanTargets(s *RuleSnapshot) []ScanTarget {
	inventory, ok := RunToolWithTimeout("docker", []string{"images", "--format", dockerImagesFormat}, 5*time.Second)
	if !ok || !inventory.OK {
		return nil
	}
	images := ParseDockerImages(strings.ToValidUTF8(string(inventory.Stdout), "�"))
	if len(images) == 0 {
		return nil
	}
	var refs []string
	containers, ok := RunToolWithTimeout("docker", []string{"container", "ls", "-a", "--format", "{{.Image}}"}, 5*time.Second)
	if ok && containers.OK {
		for line := range strings.SplitSeq(strings.ToValidUTF8(string(containers.Stdout), "�"), "\n") {
			if ref := strings.TrimSpace(line); ref != "" {
				refs = append(refs, ref)
			}
		}
	}
	var targets []ScanTarget
	for _, junk := range SelectDockerJunk(images, refs) {
		image, reference := junk.Image, junk.RmiRef()
		var label Text
		switch junk.Kind {
		case DockerDangling:
			id := []rune(image.ID)
			short := string(id[:min(12, len(id))])
			label = NewText("悬空镜像 "+short+"（构建残留）", "Dangling image "+short+" (build residue)")
		case DockerOldVersion:
			label = NewText(reference+"（旧版本）", reference+" (old version)")
		case DockerUnreferenced:
			label = NewText(reference+"（未被容器使用）", reference+" (unused by any container)")
		}
		target := ProviderTarget(s, "engine", "docker_image", DockerImagePath(reference), label, DockerImages, Operation{Kind: OpDocker, Reference: reference}, true)
		target.SizeHint = &image.Size
		targets = append(targets, target)
	}
	return targets
}

func removeDockerImageAction(reference string) (bool, error) {
	if !ValidDockerReference(reference) {
		return false, errors.New("Invalid Docker reference")
	}
	result, ok := RunToolWithTimeout("docker", []string{"image", "rm", "--", reference}, 120*time.Second)
	if !ok {
		return false, errors.New("Docker removal unavailable or timed out")
	}
	if !result.OK {
		return false, errors.New(strings.TrimSpace(string(result.Stderr)))
	}
	return strings.Contains(string(result.Stdout), "Deleted:"), nil
}

func dockerReferenceAbsence(reference string) Evidence {
	if !ValidDockerReference(reference) {
		return Unknown
	}
	result, ok := RunToolWithTimeout("docker", []string{"images", "--no-trunc", "--format", dockerImagesFormat}, 120*time.Second)
	if !ok || !result.OK || !utf8.Valid(result.Stdout) {
		return Unknown
	}
	stdout := string(result.Stdout)
	images := ParseDockerImages(stdout)
	lines := 0
	for line := range strings.SplitSeq(stdout, "\n") {
		if strings.TrimSpace(line) != "" {
			lines++
		}
	}
	if len(images) != lines {
		return Unknown
	}
	id := strings.TrimPrefix(reference, "sha256:")
	idReference := len(id) >= 12 && len(id) <= 64 && strings.IndexFunc(id, func(c rune) bool { return !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c >= 'A' && c <= 'F') }) < 0
	if !strings.ContainsAny(reference, ":@") {
		reference += ":latest"
	}
	for _, image := range images {
		if idReference && strings.HasPrefix(image.ID, id) || image.Repository+":"+image.Tag == reference || image.Repository+"@"+image.Digest == reference {
			return Absent
		}
	}
	return Confirmed
}
