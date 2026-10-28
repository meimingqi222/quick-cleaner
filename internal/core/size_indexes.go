package core

import (
	"context"
	"os"
)

type sizeIndexesKey struct{}

// WithSizeIndexes attaches immutable snapshots for advisory size estimates only.
// Discovery, ownership, target identity and deletion checks must remain live.
func WithSizeIndexes(ctx context.Context, trees []*SizeTree) context.Context {
	if len(trees) == 0 {
		return ctx
	}
	return context.WithValue(ctx, sizeIndexesKey{}, indexQuery{trees: append([]*SizeTree(nil), trees...)})
}

func indexedSize(ctx context.Context, path string) (uint64, bool) {
	if ctx.Err() != nil {
		return 0, false
	}
	query, ok := ctx.Value(sizeIndexesKey{}).(indexQuery)
	if !ok {
		return 0, false
	}
	tree := query.treeFor(path)
	if tree == nil || !tree.CoversSubtree(path) {
		return 0, false
	}
	idx, ok := tree.FindNodeByPath(path)
	if !ok {
		return 0, false
	}
	return tree.SizeOf(idx), true
}

// Only existing, non-link directories may use a cached aggregate.
func indexedDirectorySize(ctx context.Context, path string) (uint64, bool) {
	info, err := os.Lstat(path)
	if err != nil || !info.IsDir() || ruleEvidenceLink(info) {
		return 0, false
	}
	return indexedSize(ctx, path)
}

// Verification observes the filesystem after a mutation, not a prior snapshot.
func withoutSizeIndexes(ctx context.Context) context.Context {
	return context.WithValue(ctx, sizeIndexesKey{}, indexQuery{})
}
