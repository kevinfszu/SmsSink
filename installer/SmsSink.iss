; SmsSink 安装脚本（Inno Setup 6.3+，界面为简体中文）
;
; 编译方式：
;   powershell -ExecutionPolicy Bypass -File installer\build.ps1
; 直接调用 ISCC 时必须显式传版本，否则下面会 #error：
;   ISCC.exe /DAppVersion=0.2.0 installer\SmsSink.iss
;
; 安装形态：Per-User（%LOCALAPPDATA%\Programs\SmsSink），全程不需要管理员权限。
; 配置与短信记录落在 %APPDATA%\SmsSink，卸载时可选择保留。

#ifndef AppVersion
  #error 请通过 installer\build.ps1 编译，或显式传 /DAppVersion=x.y.z
#endif

#define AppName "SmsSink"
#define AppExeName "SmsSink.exe"
#define AppPublisher "kevinfszu"
#define AppURL "https://github.com/kevinfszu/SmsSink"

[Setup]
; AppId 一经发布不可更改，否则新版本会被当成另一个软件，无法覆盖升级
AppId={{D439655F-12E6-4A15-B7AA-B052ACC50069}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
AppCopyright=Copyright (C) 2026 {#AppPublisher}
VersionInfoVersion={#AppVersion}.0
VersionInfoDescription={#AppName} 安装程序
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}

DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes

; 免 UAC：装到用户目录，卸载也不需要管理员
PrivilegesRequired=lowest

OutputDir=..\dist
OutputBaseFilename={#AppName}-{#AppVersion}-setup
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

; 托盘程序不响应 Restart Manager 的关闭请求，改由 [Code] 里 taskkill
CloseApplications=no
RestartApplications=no

[Languages]
Name: "chs"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "autostart"; Description: "开机时自动启动 {#AppName}（常驻托盘）"; GroupDescription: "启动选项："
Name: "firewall"; Description: "在 Windows 防火墙中放行局域网 TCP 8787（会请求一次管理员授权）"; GroupDescription: "网络访问："; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\scripts\fix-firewall.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "..\scripts\fix-firewall.cmd"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "..\config.example.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Comment: "常驻托盘，左键单击打开管理台"
Name: "{group}\修复防火墙（手机连不上电脑时）"; Filename: "{app}\scripts\fix-firewall.cmd"; Comment: "放行局域网 8787 端口，需要管理员授权"
Name: "{group}\卸载 {#AppName}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "运行 {#AppName}"; Flags: nowait postinstall skipifsilent
Filename: "{sys}\WindowsPowerShell\v1.0\powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{app}\scripts\fix-firewall.ps1"" -ExePath ""{app}\{#AppExeName}"" -Quiet"; StatusMsg: "正在配置 Windows 防火墙…"; Flags: runhidden waituntilterminated; Tasks: firewall

[UninstallDelete]
; 运行期生成的日志不属于安装内容，Inno 默认不会清理
Type: filesandordirs; Name: "{app}\logs"
Type: files; Name: "{app}\*.log"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  RunValue = 'SmsSink';
  StateKey = 'Software\SmsSink';
  ExeName = 'SmsSink.exe';

var
  KeepUserData: Boolean;

{ 结束正在运行的托盘实例。单实例 mutex 会占住 exe，不结束就无法覆盖安装 }
procedure KillRunningApp;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{cmd}'), '/c taskkill /F /IM ' + ExeName + ' >nul 2>&1',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
end;

{ 旧版本把 config.json 写在安装目录，升级或卸载会连配置一起丢。
  这里搬到 %APPDATA%\SmsSink，已有配置则不覆盖 }
procedure MigrateLegacyConfig;
var
  Src, Dst, DstDir: String;
begin
  Src := ExpandConstant('{app}\config.json');
  DstDir := ExpandConstant('{userappdata}\SmsSink');
  Dst := DstDir + '\config.json';
  if FileExists(Src) and not FileExists(Dst) then
  begin
    ForceDirectories(DstDir);
    if CopyFile(Src, Dst, False) then
      DeleteFile(Src);
  end;
end;

{ 自启动写 HKCU\Run。安装路径变了必须重新指向，否则开机启动会指向旧位置 }
procedure ApplyAutoStart;
var
  Old, Target: String;
begin
  Target := '"' + ExpandConstant('{app}\' + ExeName) + '"';
  if WizardIsTaskSelected('autostart') then
    RegWriteStringValue(HKCU, RunKey, RunValue, Target)
  else if RegQueryStringValue(HKCU, RunKey, RunValue, Old) then
  begin
    { 用户这次没勾自启动：只清理指向已不存在程序的旧值，不动仍然有效的设置 }
    if not FileExists(RemoveQuotes(Old)) then
      RegDeleteValue(HKCU, RunKey, RunValue);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  KillRunningApp;
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    MigrateLegacyConfig;
    ApplyAutoStart;
    RegWriteStringValue(HKCU, StateKey, 'InstallDir', ExpandConstant('{app}'));
    RegWriteStringValue(HKCU, StateKey, 'Version', '{#AppVersion}');
    { 记下是否由本安装器加过防火墙规则，卸载时据此决定要不要提权清理 }
    if WizardIsTaskSelected('firewall') then
      RegWriteStringValue(HKCU, StateKey, 'FirewallRule', '1');
  end;
end;

function InitializeUninstall(): Boolean;
begin
  { 默认保留用户数据：即使中途异常也不要误删 }
  KeepUserData := True;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Old: String;
  ResultCode: Integer;
begin
  if CurUninstallStep = usUninstall then
  begin
    KillRunningApp;

    { 只删指向本安装目录的自启动项，用户手工添加的别动 }
    if RegQueryStringValue(HKCU, RunKey, RunValue, Old) then
      if Pos(Lowercase(ExpandConstant('{app}')), Lowercase(Old)) > 0 then
        RegDeleteValue(HKCU, RunKey, RunValue);

    { 放行规则绑定了 exe 路径，程序删掉后规则就没意义了；
      只有当初由安装器加过才清理，避免对没勾选过的用户弹 UAC }
    if RegQueryStringValue(HKCU, StateKey, 'FirewallRule', Old) then
      ShellExec('runas',
        ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
        '-NoProfile -ExecutionPolicy Bypass -File "' +
          ExpandConstant('{app}\scripts\fix-firewall.ps1') + '" -Remove -Quiet',
        '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

    if UninstallSilent then
      KeepUserData := True
    else
      KeepUserData := MsgBox(
        '是否保留配置与短信记录？' + #13#10 + #13#10 +
        '保留位置：' + ExpandConstant('{userappdata}\SmsSink') + #13#10 +
        '选「否」将一并删除，重装后需要重新配置手机端。',
        mbConfirmation, MB_YESNO) = IDYES;
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    if not KeepUserData then
      DelTree(ExpandConstant('{userappdata}\SmsSink'), True, True, True);
    { 先清值再删键：不依赖外部 reg.exe，避免受安全策略或 PATH 影响 }
    RegDeleteValue(HKCU, StateKey, 'InstallDir');
    RegDeleteValue(HKCU, StateKey, 'Version');
    RegDeleteValue(HKCU, StateKey, 'FirewallRule');
    RegDeleteKeyIncludingSubkeys(HKCU, StateKey);
  end;
end;
