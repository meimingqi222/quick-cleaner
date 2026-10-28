package platform

import (
	"sort"
	"strings"
)

type volumeMount struct {
	mount, device, label, filesystem string
	total                            uint64
}

// APFS roots can be snapshots (disk3s3s1); strip every numeric sN layer.
func containerDeviceKey(device string) string {
	for {
		i := strings.LastIndexByte(device, 's')
		if i < 0 || i == len(device)-1 {
			return device
		}
		for _, c := range device[i+1:] {
			if c < '0' || c > '9' {
				return device
			}
		}
		device = device[:i]
	}
}

func groupMacVolumes(mounts []volumeMount) []VolumeID {
	byDevice := make(map[string]volumeMount)
	for _, m := range mounts {
		if m.mount != "/" && (m.filesystem != "apfs" && m.filesystem != "hfs" ||
			strings.HasPrefix(m.label, ".") || strings.Contains(m.label, ".hidden")) {
			continue
		}
		key := containerDeviceKey(m.device)
		old, exists := byDevice[key]
		if !exists {
			byDevice[key] = m
			continue
		}
		if m.mount == "/" {
			old.mount = "/"
		}
		switch m.label {
		case "", "Recovery", "Preboot", "VM", "Update":
		default:
			// Rust String::len compares UTF-8 bytes, not character counts.
			if len(m.label) > len(old.label) {
				old.label = m.label
			}
		}
		old.total = m.total
		byDevice[key] = old
	}
	physical := make([]volumeMount, 0, len(byDevice))
	for _, m := range byDevice {
		physical = append(physical, m)
	}
	sort.Slice(physical, func(i, j int) bool {
		if physical[i].total != physical[j].total {
			return physical[i].total > physical[j].total
		}
		// Rust leaves ties unspecified; make equal-capacity fixtures reproducible.
		return physical[i].mount < physical[j].mount
	})
	out := make([]VolumeID, 0, len(physical))
	for _, m := range physical {
		label := m.label
		if label == "" {
			label = m.mount
		}
		chars := []rune(label)
		if len(chars) > 22 {
			label = string(chars[:22]) + "…"
		}
		out = append(out, VolumeFromMountPointWithLabel(m.mount, label))
	}
	return out
}
