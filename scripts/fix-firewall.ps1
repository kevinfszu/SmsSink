#requires -Version 5.1
<#
  修复「手机端 Webhook 测试：请求失败 / 网络连接超时」。

  根因：电脑开启「移动热点」时，热点虚拟网卡（Microsoft Wi-Fi Direct
  Virtual Adapter，IP 通常为 192.168.137.1）被 Windows 归入「公用网络」，
  而入站放行规则通常只勾选了「专用网络」，来自手机的 TCP 连接因此被静默丢弃。

  用法：
    .\scripts\fix-firewall.ps1                          开发环境，自动找 target\release
    .\scripts\fix-firewall.ps1 -ExePath "C:\...\SmsSink.exe"   指定程序
    .\scripts\fix-firewall.ps1 -Remove                 删除放行规则（卸载时用）

  参数：
    -ExePath   目标程序路径。省略时按「安装目录 → 开发目录」顺序自动探测
    -Remove    删除 SmsSink 的入站规则后退出
    -Quiet     静默模式：不打印进度、不暂停等待回车（供安装器调用）
    -NoElevate 不提权，以当前权限尽力执行（失败不弹窗）

  退出码：0 成功 / 1 执行失败 / 2 找不到目标程序
#>
[CmdletBinding()]
param(
    [string]$ExePath,
    [switch]$Remove,
    [switch]$Quiet,
    [switch]$NoElevate
)

$ErrorActionPreference = 'Stop'
$name = 'SmsSink · 局域网入站'
# 改名前的作品名，迁移时复用同一条规则而不是再建一条
$legacy = 'SMS 接码器 · 局域网入站'

function Say([string]$Message, [string]$Color = 'Gray') {
    if (-not $Quiet) { Write-Host $Message -ForegroundColor $Color }
}

# ---------- 提权 ----------
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin -and -not $NoElevate) {
    Say '需要管理员权限，正在申请提权（请在弹窗中点「是」）…' 'Yellow'
    $psArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    if ($ExePath) {
        $psArgs += @('-ExePath', "`"$ExePath`"")
    }
    if ($Remove) { $psArgs += '-Remove' }
    if ($Quiet) { $psArgs += '-Quiet' }
    # 透传子进程退出码，安装器据此判断是否成功
    $child = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $psArgs -Wait -PassThru
    exit $child.ExitCode
}

try {
    # ---------- 删除模式 ----------
    if ($Remove) {
        $hit = 0
        foreach ($n in @($name, $legacy)) {
            $rules = Get-NetFirewallRule -DisplayName $n -ErrorAction SilentlyContinue
            if ($rules) {
                $rules | Remove-NetFirewallRule -ErrorAction SilentlyContinue
                $hit++
                Say "已删除放行规则: $n" 'Green'
            }
        }
        if ($hit -eq 0) { Say '未找到 SmsSink 的放行规则，无需清理。' 'DarkGray' }
        exit 0
    }

    # ---------- 定位目标程序 ----------
    # 未指定时依次尝试：安装布局（{app}\scripts\..）→ 开发布局（scripts\..\target\release）
    if (-not $ExePath) {
        $candidates = @(
            (Join-Path $PSScriptRoot '..\SmsSink.exe'),
            (Join-Path $PSScriptRoot '..\target\release\SmsSink.exe')
        )
        $ExePath = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }

    if (-not $ExePath -or -not (Test-Path -LiteralPath $ExePath)) {
        Say "找不到 SmsSink.exe，请用 -ExePath 指定，或先在项目根目录执行 cargo build --release" 'Red'
        exit 2
    }
    $exe = [IO.Path]::GetFullPath($ExePath)

    Say "目标程序: $exe" 'Cyan'

    # 旧规则名就地重命名，避免同一条放行规则留下两份
    $legacyRules = Get-NetFirewallRule -DisplayName $legacy -ErrorAction SilentlyContinue
    if ($legacyRules) {
        $legacyRules | Set-NetFirewallRule -NewDisplayName $name -ErrorAction SilentlyContinue
        Say '已把旧规则名迁移为 SmsSink · 局域网入站' 'DarkGray'
    }

    $existing = Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue
    if ($existing) {
        $existing | Set-NetFirewallRule -Profile Any -RemoteAddress LocalSubnet -Enabled True
        # 程序路径会随安装位置变化，升级后要把规则重新绑到新 exe
        $existing | Set-NetFirewallApplicationFilter -Program $exe -ErrorAction SilentlyContinue
        Say '已更新规则 → 所有网络配置文件 / 仅本地子网 / 绑定当前程序路径' 'Green'
    } else {
        New-NetFirewallRule -DisplayName $name -Direction Inbound -Protocol TCP `
            -LocalPort 8787 -Action Allow -Profile Any -RemoteAddress LocalSubnet `
            -Program $exe | Out-Null
        Say '已新建规则 → TCP 8787 / 所有网络配置文件 / 仅本地子网' 'Green'
    }

    # 顺带清理指向已删除目录的失效放行规则（排除刚绑定的当前程序）
    Get-NetFirewallRule -Direction Inbound -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -match '^smssink' } |
        ForEach-Object {
            $app = (Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $_ -ErrorAction SilentlyContinue).Program
            if ($app -and $app -ne $exe -and -not (Test-Path -LiteralPath $app)) {
                $_ | Remove-NetFirewallRule -ErrorAction SilentlyContinue
                Say "已清理失效规则: $($_.DisplayName) -> $app" 'DarkGray'
            }
        }

    if (-not $Quiet) {
        Write-Host ''
        Get-NetFirewallRule -DisplayName $name |
            Select-Object DisplayName, Enabled, Profile, Action | Format-Table -AutoSize
        Say '完成。请回到手机端重新点「测试」。' 'Green'
        Say '排障自检：手机浏览器打开 http://<电脑IP>:8787/ping' 'Gray'
        Write-Host ''
        Read-Host '按回车关闭窗口'
    }
    exit 0
} catch {
    Say "执行失败: $($_.Exception.Message)" 'Red'
    exit 1
}
