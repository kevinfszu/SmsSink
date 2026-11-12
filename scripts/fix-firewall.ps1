#requires -Version 5.1
<#
  修复「手机端 Webhook 测试：请求失败 / 网络连接超时」。

  根因：电脑开启「移动热点」时，热点虚拟网卡（Microsoft Wi-Fi Direct
  Virtual Adapter，IP 通常为 192.168.137.1）被 Windows 归入「公用网络」，
  而入站放行规则通常只勾选了「专用网络」，来自手机的 TCP 连接因此被静默丢弃。

  用法：右键本文件 →「使用 PowerShell 运行」（会自行申请管理员权限），
        或在管理员 PowerShell 中执行： .\scripts\fix-firewall.ps1
#>

$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host '需要管理员权限，正在申请提权（请在弹窗中点「是」）…' -ForegroundColor Yellow
    $psArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $psArgs -Wait
    exit
}

$exe = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\target\release\SmsSink.exe'))
$name = 'SmsSink · 局域网入站'

Write-Host "目标程序: $exe" -ForegroundColor Cyan
if (-not (Test-Path -LiteralPath $exe)) {
    throw "找不到 $exe`n请先在项目根目录执行 cargo build --release"
}

# 旧版规则名（改名前的作品名）：就地重命名，避免同一条放行规则留下两份
$legacy = 'SMS 接码器 · 局域网入站'
$legacyRules = Get-NetFirewallRule -DisplayName $legacy -ErrorAction SilentlyContinue
if ($legacyRules) {
    $legacyRules | Set-NetFirewallRule -NewDisplayName $name -ErrorAction SilentlyContinue
    Write-Host '已把旧规则名迁移为 SmsSink · 局域网入站' -ForegroundColor DarkGray
}

$existing = Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue
if ($existing) {
    $existing | Set-NetFirewallRule -Profile Any -RemoteAddress LocalSubnet -Enabled True
    Write-Host '已更新规则 → 覆盖所有网络配置文件 / 仅放行本地子网' -ForegroundColor Green
} else {
    New-NetFirewallRule -DisplayName $name -Direction Inbound -Protocol TCP `
        -LocalPort 8787 -Action Allow -Profile Any -RemoteAddress LocalSubnet `
        -Program $exe | Out-Null
    Write-Host '已新建规则 → TCP 8787 / 所有网络配置文件 / 仅本地子网' -ForegroundColor Green
}

# 顺带清理指向已删除旧目录的失效放行规则
Get-NetFirewallRule -Direction Inbound -ErrorAction SilentlyContinue |
    Where-Object { $_.DisplayName -match '^smssink' } |
    ForEach-Object {
        $app = (Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $_ -ErrorAction SilentlyContinue).Program
        if ($app -and $app -notlike '*001-Program*' -and -not (Test-Path -LiteralPath $app)) {
            $_ | Remove-NetFirewallRule -ErrorAction SilentlyContinue
            Write-Host "已清理失效规则: $($_.DisplayName) -> $app" -ForegroundColor DarkGray
        }
    }

Write-Host ''
Get-NetFirewallRule -DisplayName $name |
    Select-Object DisplayName, Enabled, Profile, Action | Format-Table -AutoSize
Write-Host '完成。请回到手机端重新点「测试」。' -ForegroundColor Green
Write-Host '排障自检：手机浏览器打开 http://<电脑IP>:8787/ping' -ForegroundColor Gray
Write-Host ''
Read-Host '按回车关闭窗口'
