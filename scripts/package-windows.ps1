# Windows 打包脚本：编译 Go 产品并生成与 Rust 版同名、同结构的发布包
#
# 用法：
#   ./scripts/package-windows.ps1 [-OutputDir dist]
#
# 产物：
#   dist/quick-cleaner-windows-x86_64.zip         根目录是 quick-cleaner.exe
#   dist/quick-cleaner-windows-x86_64.zip.sha256  "<hex>  <文件名>"
# 旧版（Rust 与 Go）更新器都按这两个名字和 zip 里的 quick-cleaner.exe 认领
# 更新包，不能改。
#
# 程序图标、版本信息与清单来自 cmd/quick-cleaner/rsrc_windows_amd64.syso，
# 由 tools/winres 按 internal/app/root.go 的版本号生成。
#
# 签名：同时设置 WINDOWS_SIGN_PFX（.pfx 路径）与 WINDOWS_SIGN_PASSWORD 时，
# 用 signtool 做 SHA-256 Authenticode 签名并加 RFC 3161 时间戳；否则不签名。

param(
    [string]$OutputDir = "dist"
)

$ErrorActionPreference = "Stop"
$rootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $rootDir

function Invoke-Checked([string]$what, [scriptblock]$block) {
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)" }
}

function Find-SignTool {
    $found = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($found) { return $found.Source }
    $kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    $tool = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Name -eq "x64" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $tool) { throw "signtool.exe not found; install the Windows SDK" }
    return $tool.FullName
}

$name = "quick-cleaner-windows-x86_64"
$stage = Join-Path $OutputDir $name
$zip = Join-Path $OutputDir "$name.zip"
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Write-Host "==> 1/4 生成 Windows 资源（图标、版本信息、清单）" -ForegroundColor Cyan
Invoke-Checked "winres" { go run ./tools/winres }

Write-Host "==> 2/4 编译 GUI 子系统可执行文件" -ForegroundColor Cyan
$env:CGO_ENABLED = "0"
$env:GOOS = "windows"
$env:GOARCH = "amd64"
# -H windowsgui 与 Rust 的 windows_subsystem = "windows" 对应：启动时不建控制台。
Invoke-Checked "go build" { go build -trimpath -ldflags "-H windowsgui -s -w" -o "$stage/quick-cleaner.exe" ./cmd/quick-cleaner }
Remove-Item Env:GOOS, Env:GOARCH, Env:CGO_ENABLED

if ($env:WINDOWS_SIGN_PFX -and $env:WINDOWS_SIGN_PASSWORD) {
    Write-Host "==> 3/4 Authenticode 签名" -ForegroundColor Cyan
    $signtool = Find-SignTool
    Invoke-Checked "signtool sign" {
        & $signtool sign /fd SHA256 /td SHA256 /tr "http://timestamp.digicert.com" `
            /f $env:WINDOWS_SIGN_PFX /p $env:WINDOWS_SIGN_PASSWORD `
            /d "QuickCleaner" "$stage/quick-cleaner.exe"
    }
    Invoke-Checked "signtool verify" { & $signtool verify /pa /v "$stage/quick-cleaner.exe" }
} else {
    Write-Host "==> 3/4 未提供签名证书，跳过 Authenticode 签名" -ForegroundColor Yellow
}

Write-Host "==> 4/4 打包与校验和" -ForegroundColor Cyan
foreach ($doc in "README.md", "README_zh.md", "LICENSE") {
    if (Test-Path $doc) { Copy-Item $doc "$stage/" }
}
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path "$stage/*" -DestinationPath $zip
$hash = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLower()
# 不带 BOM：更新器逐字解析 "<hex>  <name>"。
[System.IO.File]::WriteAllText((Resolve-Path $OutputDir).Path + "\$name.zip.sha256", "$hash  $name.zip`n", (New-Object System.Text.UTF8Encoding $false))
Remove-Item -Recurse -Force $stage
Write-Host "完成: $zip" -ForegroundColor Green
