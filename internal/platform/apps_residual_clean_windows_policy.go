package platform

import "strings"

func residualCleanTaskRelative(name string) (string, bool) {
	text := strings.ReplaceAll(strings.TrimLeft(name, `\`), "/", `\`)
	if text == "" || strings.ContainsAny(text, ":\x00\r\n") {
		return "", false
	}
	for _, part := range strings.Split(text, `\`) {
		if part == "" || part == "." || part == ".." {
			return "", false
		}
	}
	return text, true
}

func residualCleanRegistryPath(path string) bool {
	if path == "" || strings.ContainsAny(path, "/\x00\r\n") || strings.HasPrefix(path, `\`) || strings.HasSuffix(path, `\`) {
		return false
	}
	for _, part := range strings.Split(path, `\`) {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}

func residualCleanProcessWithin(image string, roots []string) bool {
	image = strings.ReplaceAll(strings.ToLower(image), "/", `\`)
	for _, root := range roots {
		root = strings.TrimRight(strings.ReplaceAll(strings.ToLower(root), "/", `\`), `\`)
		if len(root) > 3 && (image == root || strings.HasPrefix(image, root+`\`)) {
			return true
		}
	}
	return false
}
