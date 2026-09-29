; Inno Setup script for SEU 劳动教育课程推送助手.
; Build with:  .\package.ps1
; Produces a single per-user setup.exe (no admin required).

#ifndef MyAppVersion
  #define MyAppVersion "1.1.0"
#endif

#define MyAppName "SEU 劳动教育课程推送助手"
#define MyAppShortName "SEU劳动教育助手"
#define MyAppPublisher "zz6zz666"
#define MyAppURL "https://github.com/zz6zz666/SEU-Labor-Course-Pusher"
#define MyAppExeName "seu-labor.exe"

[Setup]
AppId={{7E9C3B2A-5F1D-4C6E-9A2B-3D4E5F6A7B8C}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}

; Per-user install: no UAC prompt, installs under %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\{#MyAppShortName}
DisableDirPage=yes
DisableProgramGroupPage=yes
DefaultGroupName={#MyAppShortName}
UsePreviousAppDir=yes

OutputDir=..\release
OutputBaseFilename=seu-labor-setup-{#MyAppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763

[Languages]
Name: "chinese"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "startup"; Description: "开机自动启动"; GroupDescription: "启动选项:"
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加快捷方式:"; Flags: unchecked

[Files]
Source: "..\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppShortName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppShortName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; Mirror the app's own autostart entry (HKCU Run\SEULaborPusher).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
    ValueName: "SEULaborPusher"; ValueData: """{app}\{#MyAppExeName}"""; \
    Tasks: startup; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "-wizard"; \
    Description: "启动并打开设置向导"; Flags: nowait postinstall skipifsilent

[Code]
const
  OldRunValue = 'cn.edu.seu.laborpusher';
  OldExeName  = 'SEU劳动教育课程推送助手.exe';
  OldUninst   = 'Uninstall SEU劳动教育课程推送助手.exe';
  OldShortcut = 'SEU劳动教育课程推送助手.lnk';
  OldFolder   = 'seu-labor-daemon';

procedure KillProcesses();
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/f /im "{#MyAppExeName}"', '',
       SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/f /im "' + OldExeName + '"', '',
       SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Sleep(500);
end;

// Removes a previously installed Electron (1.0.0) build so 1.1.0 cleanly
// replaces it instead of coexisting. The shared %APPDATA% data directory
// (config / cookies / state) is intentionally left untouched.
procedure RemoveOldElectron();
var
  OldDir, Cmd, DisplayName, SubKey: String;
  ResultCode, I: Integer;
  Names: TArrayOfString;
begin
  KillProcesses();

  OldDir := '';
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
                         OldRunValue, Cmd) then
    OldDir := ExtractFileDir(RemoveQuotes(Cmd));
  if (OldDir = '') or (not DirExists(OldDir)) then begin
    OldDir := ExpandConstant('{localappdata}\Programs\' + OldFolder);
    if not DirExists(OldDir) then
      OldDir := '';
  end;

  if OldDir <> '' then begin
    Cmd := OldDir + '\' + OldUninst;
    if FileExists(Cmd) then
      Exec(Cmd, '/S', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  end;

  RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', OldRunValue);
  DeleteFile(ExpandConstant('{userprograms}\' + OldShortcut));
  DeleteFile(ExpandConstant('{userdesktop}\' + OldShortcut));

  if RegGetSubkeyNames(HKCU,
       'Software\Microsoft\Windows\CurrentVersion\Uninstall', Names) then
    for I := 0 to GetArrayLength(Names) - 1 do begin
      SubKey := 'Software\Microsoft\Windows\CurrentVersion\Uninstall\' + Names[I];
      if RegQueryStringValue(HKCU, SubKey, 'DisplayName', DisplayName) then
        if Pos('SEU劳动教育课程推送助手', DisplayName) = 1 then
          RegDeleteKeyIncludingSubkeys(HKCU, SubKey);
    end;

  if OldDir <> '' then
    for I := 1 to 6 do begin
      DelTree(OldDir, True, True, True);
      if not DirExists(OldDir) then
        Break;
      Sleep(800);
    end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    RemoveOldElectron();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    KillProcesses();
end;
