package ui

import "fmt"

type searchPageCopy struct {
	heading, subheading, placeholder, building, noIndex, searching, noResults string
	indexing, group, open, reveal, clear, hint, limit, top, matched           string
	columns                                                                   [4]string
}

func searchPageStrings(l Language) searchPageCopy {
	if l == English {
		return searchPageCopy{
			heading: "File Search", subheading: "Full-disk index, instant file search",
			placeholder: "Type a filename (supports * and ? wildcards)…",
			building:    "Building search index, search will be available when done…", noIndex: "Failed to build search index",
			searching: "Searching…", noResults: "No matching files found", indexing: "Building full-disk search index…",
			group: "By Type", open: "Open", reveal: "Reveal", clear: "Clear search",
			hint: "Double-click to open · Click header to sort", limit: "Max 500 items",
			top: "Top %d largest items · type to filter", matched: "Matched %d items",
			columns: [4]string{"Name", "Path", "Size", "Action"},
		}
	}
	return searchPageCopy{
		heading: "文件搜索", subheading: "全盘索引，秒级检索任意文件",
		placeholder: "输入文件名关键词，支持 * 和 ? 通配符…",
		building:    "正在构建搜索索引，完成后即可搜索…", noIndex: "无法构建搜索索引", searching: "正在后台高速检索…",
		noResults: "未找到匹配文件", indexing: "正在构建全盘搜索索引…", group: "按类型聚合", open: "打开", reveal: "定位", clear: "清空搜索",
		hint: "双击条目直接打开 · 点击表头自定义排序", limit: "单次最多展示 500 项",
		top: "全盘最大的 %d 项 · 输入关键字精确检索", matched: "匹配到 %d 个文件 / 文件夹",
		columns: [4]string{"名称", "路径", "大小", "操作"},
	}
}

func searchResultCount(l Language, count int) string {
	if l != English {
		return fmt.Sprintf("%d 条结果", count)
	}
	if count == 1 {
		return "1 result"
	}
	return fmt.Sprintf("%d results", count)
}
