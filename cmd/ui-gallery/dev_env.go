package main

import (
	"github.com/meimingqi222/quick-cleaner/internal/core"
	appui "github.com/meimingqi222/quick-cleaner/internal/ui"
)

func devEnvFixture(shell appui.Dashboard) (*appui.DevEnvPage, appui.DevEnvPageActions) {
	site, version, count := "/Users/Example/Library/Python/3.13/site-packages", "1.2.3", 2
	s := &appui.DevEnvPage{Language: shell.Language, Scanning: shell.Scanning, Removing: shell.Cleaning,
		Selected: map[string]bool{}, Packages: map[string][]core.DevAssetItem{}}
	if shell.HasJunk {
		s.Assets = []core.DevAssetItem{
			{ID: "base", Kind: core.DevAssetKind{Type: core.DevCondaEnv, Name: "base", IsBase: true}, Path: "/Users/Example/miniconda3", Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 600 << 20}},
			{ID: "python", Kind: core.DevAssetKind{Type: core.DevPythonInterpreter, Name: "Python 3.13", Scope: core.SiteUser, SitePackages: &site, PackageCount: &count}, Path: "/Users/Example/Library/Python/3.13", Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 200 << 20}},
			{ID: "node", Kind: core.DevAssetKind{Type: core.DevNodePackage, Name: "example-cli", Manager: "npm", Version: &version, BinShims: []string{"example"}}, Path: "/Users/Example/.npm-global/lib/node_modules/example-cli", Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 42 << 20, ExclusiveReclaimableBytes: 42 << 20}},
			{ID: "tool", Kind: core.DevAssetKind{Type: core.DevPythonTool, Name: "example-tool", ToolKind: core.PythonToolUv, Executables: []string{"example-tool"}}, Path: "/Users/Example/.local/share/uv/tools/example-tool", Source: core.AssetTool, Size: core.AssetStorageSize{LogicalBytes: 80 << 20, ExclusiveReclaimableBytes: 80 << 20}},
		}
		s.ActiveID = "python"
		s.Packages["python"] = []core.DevAssetItem{
			{ID: "requests", Kind: core.DevAssetKind{Type: core.DevPipPackage, Name: "requests", Version: &version}, Path: site + "/requests-1.2.3.dist-info", Source: core.AssetTool},
			{ID: "urllib", Kind: core.DevAssetKind{Type: core.DevPipPackage, Name: "urllib3", Version: &version}, Path: site + "/urllib3-1.2.3.dist-info", Source: core.AssetTool},
		}
		s.TotalLogical, s.TotalReclaimable = 922<<20, 122<<20
	}
	return s, appui.DevEnvPageActions{
		SetSearch: func(q string) { s.Search = q }, SetFilter: func(f appui.DevEnvFilter) { s.Filter = f },
		SelectAsset:    func(id string) { s.ActiveID = id },
		ToggleSelected: func(id string) { s.Selected[id] = !s.Selected[id] },
		ClearSelection: func() { clear(s.Selected) },
		SelectAll: func() {
			for _, item := range s.Assets {
				if core.CanRemoveDevAsset(item) {
					s.Selected[item.ID] = true
				}
			}
			for _, packages := range s.Packages {
				for _, item := range packages {
					if core.CanRemoveDevAsset(item) {
						s.Selected[item.ID] = true
					}
				}
			}
		},
		InvertSelection: func() {
			for _, item := range s.Assets {
				if core.CanRemoveDevAsset(item) {
					s.Selected[item.ID] = !s.Selected[item.ID]
				}
			}
			for _, packages := range s.Packages {
				for _, item := range packages {
					if core.CanRemoveDevAsset(item) {
						s.Selected[item.ID] = !s.Selected[item.ID]
					}
				}
			}
		},
		SelectSitePackages: func(id string) {
			for _, item := range s.Packages[id] {
				if core.CanRemoveDevAsset(item) {
					s.Selected[item.ID] = true
				}
			}
		},
		InvertSitePackages: func(id string) {
			for _, item := range s.Packages[id] {
				if core.CanRemoveDevAsset(item) {
					s.Selected[item.ID] = !s.Selected[item.ID]
				}
			}
		},
	}
}
