# Memoria Launcher 自动化单文件打包脚本
param(
    [string]$TargetDir = "$HOME\.cargo-targets\mc-launcher"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
Set-Location $ScriptDir

Write-Host "==> 检查构建环境..." -ForegroundColor Cyan

# 检查 cargo 工具链
if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "未找到 cargo 命令，请先安装 Rust 工具链 (https://rustup.rs)。"
    exit 1
}

# 提示本地登录配置状态
$ConfigFile = Join-Path $ScriptDir "build-config.local.json"
if (Test-Path $ConfigFile) {
    Write-Host "已检测到 build-config.local.json，将自动内置配置的应用 ID。" -ForegroundColor Green
} else {
    Write-Host "未找到 build-config.local.json，构建将不内置默认应用 ID（可在应用内手动设置）。" -ForegroundColor Yellow
}

# 隔离构建缓存目录，避免受限 ACL/沙盒环境导致构建脚本子进程异常
if (-not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = $TargetDir
}
Write-Host "构建输出目录: $env:CARGO_TARGET_DIR" -ForegroundColor DarkGray

Write-Host "==> 开始编译 Release 启动器与内置 MCP 服务..." -ForegroundColor Cyan
& cargo build --release --locked -p desktop -p mc-mcp
if ($LASTEXITCODE -ne 0) {
    Write-Error "编译失败，退出码: $LASTEXITCODE"
    exit $LASTEXITCODE
}

# 发布输出目录
$DistDir = Join-Path $ScriptDir "dist"
if (!(Test-Path $DistDir)) {
    New-Item -ItemType Directory -Path $DistDir | Out-Null
}

$SourceExe = Join-Path $env:CARGO_TARGET_DIR "release\desktop.exe"
$DestExe = Join-Path $DistDir "MemoriaLauncher.exe"

Copy-Item -Path $SourceExe -Destination $DestExe -Force
$McpSourceExe = Join-Path $env:CARGO_TARGET_DIR "release\mc-mcp.exe"
$McpDestExe = Join-Path $DistDir "mc-mcp.exe"
Copy-Item -Path $McpSourceExe -Destination $McpDestExe -Force

$FileInfo = Get-Item $DestExe
$FileHash = (Get-FileHash -Path $DestExe -Algorithm SHA256).Hash
$SizeMB = [math]::Round($FileInfo.Length / 1MB, 2)

Write-Host "`n打包完成！" -ForegroundColor Green
Write-Host "输出文件: $DestExe"
Write-Host "文件大小: $SizeMB MB ($($FileInfo.Length) 字节)"
Write-Host "SHA256:   $FileHash"
Write-Host "MCP 服务: $McpDestExe"
