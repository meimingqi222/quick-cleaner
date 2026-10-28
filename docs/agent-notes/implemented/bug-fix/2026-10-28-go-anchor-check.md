# Agent Note: Go 测试锚点按源文件声明校验

Status: implemented

## Problem

go-port 迁移后，`docs/agent-notes` 里有 94 个锚点指向 `internal/**/*_test.go::TestXxx`。
`scripts/check-anchors-vs-tests.py` 只把 `.py` 锚点当作「文件内函数声明」校验，其余锚点
一律拿去和 `cargo test --list` 产物比对；`verify-anchors` 作业也只下载 Windows/macOS 的
Rust 清单。于是这 94 个真实存在的 Go 测试全部判为缺失，CI 恒红
（run 40669908139：533 锚点 / 94 缺失）。

## Decision

`.go` 锚点与 `.py` 锚点走同一规则：锚点必须在引用文件里命中函数声明（Go 为
`^func <name>(`，Python 为 `^\s*def <name>(`），否则报缺失。Rust 锚点继续与 cargo
测试清单并集比对。`main()` 增加可注入的 `root` 参数，让检查规则能对临时夹具做回归测试。

## Alternatives considered

在 `verify-anchors` 作业里安装 Go 并跑 `go test -list ./...`：Go 的平台门控测试文件只在
各自系统列出，要取三平台并集；同包同名测试在按包输出的清单里还要再做文件归属映射。相比
Python 先例，这引入新的工具链与平台矩阵，却只把「文件里有声明」提升到「真的编进了二进制」。
本次要挡的是改名后旧名留在注释里，文件内声明校验已经覆盖，因此不引入。

把 note 锚点改回 Rust 测试：Go 迁移后这些行为只由 Go 测试覆盖，改绑等于放弃证据。

## Consequences

Go 测试锚点现在与 Python 同级：校验「引用文件里确有该测试函数」，不依赖平台构建。测试改名
而旧名只在注释或别处出现时，CI 立即失败。将来再接入其他语言的测试锚点，必须先在
`FILE_DECLARATION` 里登记该语言的声明形式，否则会重演本次全量误报。

## Verification

- `tools/regression-notes/tests/test_anchor_checks.py::test_go_anchor_must_name_a_declared_function`
- `tools/regression-notes/tests/test_anchor_checks.py::test_go_anchor_named_only_in_a_comment_is_missing`
- `tools/regression-notes/tests/test_anchor_checks.py::test_python_anchor_still_names_a_def`
- `tools/regression-notes/tests/test_anchor_checks.py::test_rust_anchor_still_needs_a_listed_test`

Proved: 修复前用 CI 同一份清单跑出 533 锚点 / 94 缺失
（`docs/agent-notes-evidence/2026-10-28-go-anchor-check-red.log`）；修复后同一份清单
0 缺失（`docs/agent-notes-evidence/2026-10-28-go-anchor-check-green.log`）。
