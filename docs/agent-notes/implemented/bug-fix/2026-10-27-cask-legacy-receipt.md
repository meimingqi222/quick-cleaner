# Agent Note: Caskroom 里的旧式 cask 不能拦住每一次卸载

Status: implemented

## Problem

下述应用与 cask 名称已匿名化。

在 macOS 上卸载 某应用（由 Homebrew 之外的方式安装），页面报：

```
「某应用」官方卸载未正常完成：Homebrew evidence missing: /opt/homebrew/Caskroom/<旧记录>/.metadata/INSTALL_RECEIPT.json
```

用户既没装过 该应用的 Homebrew 版，也没打算碰 <旧记录>。选择被卸载的应用与报错里
提到的 cask 毫无关系。

原因在归属盘点的粒度：macOS 的卸载路线先要回答「这次要卸的应用归不归 Homebrew 管」，
实现方式是遍历 `/opt/homebrew` 与 `/usr/local` 下的**整个** `Caskroom`，逐条读
`.metadata/<version>/<timestamp>/Casks` 清单和 `.metadata/INSTALL_RECEIPT.json`，再拿
清单里的 `app` artifact 与所选应用路径比对。其中任何一条记录读不出来，整次盘点就返回
错误，而错误会被原样当成「卸载失败」——于是 Caskroom 里任意一条异常记录都会拦住**所有**
应用的卸载，无论它们是不是 Homebrew 装的。

现场的 `Caskroom` 里有多条这样的记录。它们只有清单，没有 `INSTALL_RECEIPT.json`——这些是 Homebrew 早期版本装的
cask，当时的安装器不写收据。

同一段代码里还有第二个错误，只是被第一个挡住了：拿到 `bin/brew` 的处理器路径时，先用
`EvalSymlinks` 解成真实路径，再拿它去和**未规范化**的 prefix 做封闭性判断。macOS 上
`/var` 是 `/private/var` 的别名，夹具根和某些用户目录都落在这层别名下，于是「真实目标
vs 别名根」的文本前缀比较直接判成 `evidence escapes root`。

## Decision

**收据缺失是「更早的安装」，不是「记录损坏」。** 两条实现（Rust 参照树
`src/platform/macos/cask_uninstall.rs` 与 Go 产品 `internal/core/apps_cask_uninstall.go`）
都改成：`INSTALL_RECEIPT.json` 不存在时按空收据继续读数，其余照旧。这条记录仍然入表，
仍然凭清单里的 `app` artifact 认领归属（所以真正属于它的应用依然会被它挡住，不会被别的
cask 误卸），但它拿不到 `source` 证据，`Supported` 恒为 false，**永不授权卸载**。

收据存在但读不出来——损坏、非法 UTF-8、被指向外部——仍然整条报错。这是区分「旧格式」
与「记录被改坏」的界线，不能一起放宽。

**封闭性判断的两侧都要是真实路径。** 处理器路径先按原样（未解别名）做一次封闭检查，
这一次负责拒绝 prefix 以下任何一跳链接；解成真实路径后再拿同样规范化过的 prefix 复核
一次。任何一次不通过都仍然拒绝执行。

## Alternatives considered

- **跳过没有收据的 cask 目录。** 简单，但会让真正属于它的应用失去归属证据：少一条记录，
  就等于少一道「这个应用归 Homebrew 管，别走废纸篓」的拦截。宁可留一条无授权的记录。
- **把整个盘点错误降级成 warning 继续。** 会把「记录被改坏」也一起放过，等于在唯一能证明
  归属的地方停止校验。
- **直接删掉 Caskroom 里的旧记录。** 是用户数据，且 `PITFALLS` P65 已明确禁止直接删
  Caskroom。
- **用应用名或 Bundle ID 猜 token。** P65 已禁止：会错配同名程序。

## Consequences

旧记录现在能正常入表：有 `app` artifact 的记录认领其应用
（`Supported=false`，所以仍要求走 Homebrew 官方流程），没有 artifact 的记录认领为空。卸载与它们无关的应用不再报错。

对新装的 cask 没有任何行为变化：它们都有收据，走原路径。

## Verification

- `internal/core/apps_cask_uninstall_test.go::TestAppCaskUninstallerIgnoresReceiptlessLegacyCasks`
- `src/platform/macos/cask_uninstall.rs::receiptless_legacy_cask_stays_listed_without_uninstall_authority`

Proved: 修复前在 `TestAppCaskUninstallerIgnoresReceiptlessLegacyCasks` 上自然红跑，报的正是
用户截图里那一句 `Homebrew evidence missing: .../Caskroom/legacy/.metadata/INSTALL_RECEIPT.json`，
见 `docs/agent-notes-evidence/2026-10-27-cask-legacy-receipt-red.log`；修复后同 focused 转绿，
见 `docs/agent-notes-evidence/2026-10-27-cask-legacy-receipt-green.log`。

别名根那一处是制造型证据：只把第二次封闭性判断里的 `prefix` 换回未规范化的
`install.Prefix`、其余不动，`TestAppCaskUninstallerReceiptBoundContract` 的 success 子用例
立刻红成 `true evidence escapes root calls=0`，见
`docs/agent-notes-evidence/2026-10-27-cask-alias-root-red.log`；恢复同一处后同 focused 转绿，
见 `docs/agent-notes-evidence/2026-10-27-cask-alias-root-green.log`。

Rust 侧同 focused 绿证据见
`docs/agent-notes-evidence/2026-10-27-cask-legacy-receipt-rust-green.log`。

完整验证（macOS）：`gofmt -l`、`go vet ./...`、`go build ./...` 通过；
`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 通过；
`cargo test --lib` 为 839 passed / 0 failed / 12 ignored。

`go test ./...` 在本机仍有 7 个失败，全部与本次改动无关，且撤掉本次改动后同样失败：本机
`/var` 指向 `/private/var` 使 `TestScanDirectory*`、`TestMacDiskIndex*` 等夹具路径不在期望
形态上，以及环境缺 `/bin/true`。改动前 `TestAppCaskUninstallerReceiptBoundContract` 也是
红的，属于同一根因（别名根），已随本次修好。

Pitfalls checked: 逐条核对 P1–P88。P65 的四条禁止（用应用名猜 cask、用家族前缀扩大
所有权、直接删 Caskroom、在失败/未知时退回废纸篓）一条都没有触碰——本次只放宽「记录格式
更旧」这一种情形，失败路径仍然返回错误、仍然不授权回退删除。P20 的「已尝试生态命令禁止
裸删回退」由未被改动的 `runMacAppUninstallerWith` 继续覆盖。P86 的失败文案路线未改动。
新增 P89。
