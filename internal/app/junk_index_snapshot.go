package app

import "github.com/meimingqi222/quick-cleaner/internal/core"

func prioritizeJunkIndex(trees []*core.SizeTree, fresh *core.SizeTree) []*core.SizeTree {
	return append([]*core.SizeTree{fresh}, trees...)
}

func (r *Root) rememberJunkIndex(tree *core.SizeTree) {
	mount := tree.Volume().MountPoint()
	r.searchTrees[mount] = tree
}
