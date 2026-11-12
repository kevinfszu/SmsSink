#requires -Version 5.1
<#
  编译 SmsSink 安装包。

  用法：
    powershell -ExecutionPolicy Bypass -File installer\build.ps1
    powershell -ExecutionPolicy Bypass -File installer\build.ps1 -SkipBuild
    powershell -ExecutionPolicy Bypass -File installer\build.ps1 -Version 0.3.0

  行为：
    1. 版本号默认取 Cargo.toml（唯一真相），也可用 -Version 覆盖
    2. 执行 cargo build --release --locked（-SkipBuild 可跳过）
    3. 校验 exe 的资源版本与目标版本一致，防止改了 Cargo.toml 忘了重编译
    4. 调用 ISCC 编译出 dist\SmsSink-<版本>-setup.exe
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$Version
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$issPath = Join-Path $PSScriptRoot 'SmsSink.iss'
$exePath = "$root\target\release\SmsSink.exe"

# ---------- 版本 ----------
if (-not $Version) {
    $cargo = Get-Content "$root\Cargo.toml" -Raw
    if ($cargo -match '(?m)^version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        throw 'Cargo.toml 里找不到 version 字段'
    }
}
Write-Host "目标版本: $Version" -ForegroundColor Cyan

# ---------- 构建 ----------
if (-not $SkipBuild) {
    Push-Location $root
    try {
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $exePath)) {
    throw "找不到 $exePath，请先执行 cargo build --release"
}

# ---------- 版本一致性 ----------
$fileVersion = (Get-Item $exePath).VersionInfo.FileVersion
$expected = "$Version.0"
if ($fileVersion -ne $expected) {
    throw "exe 资源版本为 $fileVersion，与目标 $expected 不一致；请重新编译或检查 Cargo.toml"
}
Write-Host "exe 资源版本: $fileVersion" -ForegroundColor DarkGray

# ---------- 定位 ISCC ----------
$candidates = @()
if ($env:ISCC) { $candidates += $env:ISCC }
if (${env:ProgramFiles(x86)}) { $candidates += "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
if ($env:ProgramFiles) { $candidates += "$env:ProgramFiles\Inno Setup 6\ISCC.exe" }
if ($env:LOCALAPPDATA) { $candidates += "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe" }

$iscc = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $iscc) {
    throw @'
找不到 ISCC.exe（Inno Setup 6 编译器）。
安装方式任选其一：
  winget install --id JRSoftware.InnoSetup
  choco install innosetup -y
或用环境变量 ISCC 指定完整路径。
'@
}
Write-Host "编译器: $iscc" -ForegroundColor DarkGray

# ---------- 编译 ----------
& $iscc "/DAppVersion=$Version" $issPath
if ($LASTEXITCODE -ne 0) { throw "ISCC 编译失败（退出码 $LASTEXITCODE）" }

$setup = "$root\dist\SmsSink-$Version-setup.exe"
if (-not (Test-Path -LiteralPath $setup)) {
    throw "编译未报错，但没有生成 $setup"
}

$mb = (Get-Item $setup).Length / 1MB
Write-Host ''
Write-Host ("安装包: {0}  ({1:N1} MB)" -f $setup, $mb) -ForegroundColor Green
