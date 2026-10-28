# Agent Note: DMG 挂载卷里的副本不是安装，常驻说明不是警告

Status: implemented

## Problem

下述应用名、Bundle ID 和路径已匿名化。

卸载 Example 后清理残留，点「彻底清除所选」面板原样不动，上方还压着两条橙色横幅：

```
勾选的项目在删除前会再确认一次：应用确实已卸载、文件没有被替换、当前没被占用；任一项不通过就整批不删。
残留清理授权无法验证；未清理任何残留。
```

两个独立缺陷。

**一、DMG 挂载副本被当成「仍安装」。** 清理前提是「应用已卸载」，判据是
`BundleIsStillInstalled`。实测这台机器上该函数对 `com.example.removed` 返回
`present=true`——但 `/Applications/Example.app` 并不存在，命中的是
`/Volumes/Example/Example.app`：用户从 DMG 拖进应用程序后，安装盘一直挂着。
LaunchServices 把挂载卷里那份登记成安装，`~/.Trash/Example.app` 被正确过滤掉了，
只剩下挂载卷路径，于是整批拒绝删除。

根源是移植时把 `macRegistrationIDs`（LaunchServices 登记）加成了**肯定**判据。
Rust 参照 `bundle_is_still_installed` 只看三样：Input Methods 根、
`mdfind` 按 Bundle ID 的查询、索引可用性哨兵。它从不用 LaunchServices 登记
作为「已安装」的证据，也不是从 `/Volumes` 命中的。

**二、常驻说明被画成警告。** `ResidualNotice` 是每个残留面板都有的固定说明
（「删除前会再确认一次……」），内容与具体项目无关，却用 `cautionContainer`
橙色底 + `caution` 字色渲染在标题下方。三个置信度「确定」的项目上方挂着两条
橙色横幅，看起来像有三件事等用户处理。Rust 的残留面板只在该提示时出现橙色横幅
（`occupancy.is_occupied()`、系统扩展），没有第三条常驻说明。

## Decision

**卷身份判定。** `macRegisteredLive` 增加一个可注入的卷探针
（`macVolumeProbe`），默认实现 `macOnDataVolume` 比较目标与 `/`、
`/System/Volumes/Data` 的 `st_dev`：落在系统盘数据卷以外的副本（DMG、外接盘、
网络卷）不计入「仍安装」。卷身份测不出时返回 `known=false`，调用方保持原判断
——「测不出」绝不能向放行删除的方向猜。

**常驻说明降级。** 从标题下方的橙色横幅移到列表下方的一行灰字小字（11px、
`outline` 色）。真正的失败提示 `Notice` 与占用横幅仍保持警告样式。

## Alternatives considered

- **按路径前缀排除 `/Volumes/`。** 会漏掉外接盘、网络卷、`/private/var/…/`
  里挂载的镜像；也会误伤真有安装放在非标准卷上的情况。卷身份是更本的原判据。
- **直接用 `mdfind` 结果，忽略 LaunchServices 登记。** 会削弱既有防护：登记
  路径是判断「另一份安装还在」的证据之一，删掉它等于放松校验。
- **把「授权无法验证」的失败文案改得更客气。** 治标：面板仍不关、用户仍不知道
  为什么点不动。真正的问题是判据错，不是措辞。
- **删掉 `ResidualNotice`。** 这道说明对「勾选不等于一定删」这件事有价值，
  保留内容、只改呈现层级。

## Consequences

从 DMG 安装、安装盘仍挂着的应用，卸载后残留可以正常清理。真正装在数据卷上的
应用仍被拦住，Mount 里的副本不再挡住清理。残留面板只剩真实警示是橙色，
常驻说明退成脚注。

## Verification

- `internal/platform/apps_bundle_test.go::TestMacRegisteredLiveIgnoresBundlesOnOtherVolumes`
- `internal/ui/apps_page_test.go::TestResidualGuaranteeIsNotDrawnAsAWarning`

Proved: 卷身份那一处是制造型证据——只把挂载卷过滤那一行撤掉、其余不动，
`TestMacRegisteredLiveIgnoresBundlesOnOtherVolumes` 立刻红成
`mounted image copy counted as an installation: map[com.example.Dmg:{}] true`，
见 `docs/agent-notes-evidence/2026-10-27-dmg-volume-red.log`；恢复同一处后同 focused
转绿，见 `docs/agent-notes-evidence/2026-10-27-dmg-volume-green.log`。

横幅那一处自然红跑：修复前断言直接列出 `Texts()` 里那条橙色横幅文案，见
`docs/agent-notes-evidence/2026-10-27-residual-banner-red.log`；改为列表下方灰字后
同 focused 转绿，见 `docs/agent-notes-evidence/2026-10-27-residual-banner-green.log`。
测试用 `Find` 的 Y 坐标断言说明排在列表之后，避免以后又被搬回标题下方。

真机只读核对：修复后 `BundleIsStillInstalled(com.example.removed)` 返回
`present=false known=true`（挂载的 DMG 仍挂着），`com.apple.Safari` 仍为
`present=true known=true`——松紧两个方向都对。

`gofmt`、`go vet ./...`、`go build ./...`、`GOOS=windows go build ./...` 通过；
`go test ./internal/ui ./internal/app` 全绿。`internal/platform` 只剩既有环境失败
`TestMacWalkSpillReclaimsDeadProcessFilesOnly`（本机缺 `/bin/true`，与本次无关）。

Pitfalls checked: 逐条核对 P1–P89。P31/P60 的「孤儿所有者未知拒绝」不变——
卷身份测不出时仍按「仍安装」处理，方向仍是 fail closed。P37 的逐项身份与占用
复核不变。P65 的「失败不授权回退删除」不变：本次只修正判据，不新增任何删除路径。
P88 的「面板说明不能塞进共享 Notice」保持：`ResidualNotice` 仍是面板专属字段，
本次只改它的呈现层级。新增 P90。

## Follow-up: 缓存里的 bundle 备份也不是安装

同一天又撞到同一个判据的第二种形态。卸载 另一款应用（
`com.example.migrated`）后清理残留，依旧报「残留清理授权无法验证」，面板
77 项一行都删不掉。

实测该 id 的 LaunchServices 登记里**只有一条路径**：

```
~/Library/Caches/com.example.migrated.BundleMigration/backups/
    Example-backup.app
```

这是应用自己升级 bundle 时留下的备份（923 MB 的缓存目录），带同一个
CFBundleIdentifier，被 Spotlight 索引、也被 LaunchServices 登记。
`mdfind` 与登记两条分支都会命中它，于是「应用仍安装」。

坑在于这套判据的三处入口各自过滤自己的：`macRegisteredLive` 一直只排除 Trash
目录，卷身份那次修的是挂载卷，缓存目录漏在中间。判据应当是「这个路径能不能装
应用」，于是收敛成两个判据：

- `macInstalledLocation(path, home)`：bundle 形态 + 不在 `~/Library/Caches`；
  `macAnyInstallation` 用它筛 `mdfind` 的输出。
- `macRegisteredLive` 里补同样的缓存排除；HOME 从 `trash` 的父目录推出，
  推不出来就不排除（更保守）。

两处都只排除**能装应用的位置**，其余一律仍算安装——把活着的应用判成已卸载，才是
会删掉用户配置的那个方向。

- `internal/platform/apps_bundle_test.go::TestMacRegisteredLiveIgnoresCachedBundleCopies`
- `internal/platform/apps_bundle_test.go::TestMacAnyInstallationSkipsCachedCopies`
- `internal/platform/apps_bundle_test.go::TestMacInstalledLocationRejectsCachedBundleCopies`

Proved: 制造型——只撤掉 `macRegisteredLive` 里那三行缓存排除，其余不动，
`TestMacRegisteredLiveIgnoresCachedBundleCopies` 立刻红成
`cached bundle backup counted as a live installation: map[com.example.migrated:{}] true`，
见 `docs/agent-notes-evidence/2026-10-27-cached-bundle-red.log`；恢复后同 focused 转绿，
见 `docs/agent-notes-evidence/2026-10-27-cached-bundle-green.log`。

真机只读核对（DMG 已弹出、缓存备份仍在）：`com.example.migrated` →
`present=false known=true`；同机器上真正装着的 `com.example.installed`
（`/Applications/InstalledExample.app`）→ `present=true known=true` 不受影响。


2026-10-28 审查补齐 Spotlight 分支：原有卷过滤只接入 `macRegisteredLive`，
`macAnyInstallation` 所用的 `macInstalledLocation` 仍会接受其他卷上的副本。
现在两条分支都调用同一卷探针；已知其他卷被排除，本机卷及未知卷保持安装保护。
不以 `/Volumes` 字符串前缀猜测卷身份。

- `internal/platform/apps_bundle_test.go::TestMacSpotlightInstallationFiltersOtherVolumes`

Proved: 修复前注入已知其他卷探针，Spotlight 路径仍返回安装，测试在
`Spotlight mounted-volume copy counted as an installation` 断言自然红跑，见
`docs/agent-notes-evidence/2026-10-28-spotlight-volume-red.log`；修复后同 focused
绿跑见 `docs/agent-notes-evidence/2026-10-28-spotlight-volume-green.log`。
