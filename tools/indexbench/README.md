# 无界面索引性能测试

在仓库根目录执行。命令在临时目录复制选中卷的 base、delta 和覆盖 sidecar，测试结束后移除副本；不改写正式缓存，不执行任何清理，不启动 GUI。

```bash
# 快速测缓存解析和发现扫描（默认两轮；每轮重新读缓存）
go run ./tools/indexbench

# macOS：调用产品 MacDiskIndex.Load，测事件回放、增量更新/重建和落盘
# 两轮复用同一个 owner，第二轮反映同进程重复扫描
go run ./tools/indexbench -mode refresh -rounds 2

# 包含 GUI 相同的发现规划及未知目录实时回退
go run ./tools/indexbench -mode refresh -product-discovery -rounds 2

# 只定位加载阶段，省去发现扫描
go run ./tools/indexbench -mode refresh -discover=false

# 输出 JSON Lines 及 CPU profile；产品分段日志在 stderr
# 可将 stderr 重定向到本地日志保留诊断证据
go run ./tools/indexbench -mode refresh -json -cpu-profile /tmp/qc-index.cpu

go tool pprof -top /tmp/qc-index.cpu
# 若当前 Go 分发不带 pprof 工具，可使用独立工具：
# go run cmd/pprof -top /tmp/qc-index.cpu
```

`load` 是前台索引返回时间，包含事件回放、应用变更；保存由同一个 owner 在后台有序执行。`persist wait` / JSON 的 `persist_wait` 是发现及复核结束后，等待剩余后台保存和压实重载完成的时间，不能视为保存本身的总耗时。下一轮及删除临时副本前均等待保存完成。分段日志包含 cache-read、event-replay、subtree-scans、clone、rebuild-arrays、background-persist、background-rebind、full-scan 和 spill-build。

默认 `discover` 只测单树索引发现；`-product-discovery` 测 GUI 相同的发现规划，包含未覆盖目录的实时回退，但不含 GUI 渲染、固定扫描与原生 provider。`verify` 为结果路径存在性复核。MiB 是整个进程在阶段窗口内的累计分配量，包含并发后台保存，不是峰值内存或该函数独有分配。结果指纹可用于同一索引上的前后对照。

`-root` 默认 `/`；`-cache-dir` 指定源缓存；`-timeout` 默认每轮 5 分钟；`-mode refresh -fresh` 在空临时缓存中测首次构建。中断可按 Ctrl+C。实际结果受文件变化、文件系统缓存、FDA/TCC 权限和机器负载影响；refresh 模式写的是副本，过滤的缓存目录也是该副本，所以原缓存目录中的并发写入仍可能成为普通事件。对比时应避免其他 QuickCleaner 实例同时扫描。

回归测试不依赖真实用户目录：

```bash
go test ./tools/indexbench
go test ./internal/core -run 'TestMacDiskIndex|TestBuildDiscoverySharesFrozenRule|TestMappedDiscoveryFlagsAndTags' -count=1
```
