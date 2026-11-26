; Inno Setup script for Superlight Tray (per-user install, no admin rights needed).
; Build:  iscc /DAppVersion=0.1.0 /DSourceExe=<path to superlight-tray.exe> /DAssets=<dir> /O<outdir> superlight-tray.iss
; The assets (icon + header logo) come from: cargo run -p superlight-tray --example installer_assets -- <dir>

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\superlight-tray.exe"
#endif
#ifndef Assets
  #define Assets "assets"
#endif

[Setup]
AppId={{6B1E3F52-9C4D-4E8A-B7A2-5D0C1F7E9A31}
AppName=Superlight Tray
AppVersion={#AppVersion}
AppVerName=Superlight Tray {#AppVersion}
AppPublisher=Menisk7
AppPublisherURL=https://github.com/Menisk7/superlight-tray
AppComments=Battery level and DPI for the Logitech PRO X Superlight
DefaultDirName={autopf}\Superlight Tray
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputBaseFilename=superlight-tray-{#AppVersion}-setup
SetupIconFile={#Assets}\superlight-tray.ico
WizardSmallImageFile={#Assets}\wizard-small.bmp
UninstallDisplayIcon={app}\superlight-tray.exe
UninstallDisplayName=Superlight Tray
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
LicenseFile=..\..\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start Superlight Tray when I sign in to Windows"; GroupDescription: "Options:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Options:"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Superlight Tray"; Filename: "{app}\superlight-tray.exe"
Name: "{autodesktop}\Superlight Tray"; Filename: "{app}\superlight-tray.exe"; Tasks: desktopicon

[Registry]
; Same value the app's own "Start with Windows" menu item uses, so they stay in sync.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "SuperlightTray"; ValueData: """{app}\superlight-tray.exe"""; Tasks: autostart; Flags: uninsdeletevalue
; Always remove it on uninstall, even if it was turned on later from the tray menu.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "SuperlightTray"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueType: none; ValueName: "SuperlightTray"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\superlight-tray.exe"; Description: "Start Superlight Tray now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM superlight-tray.exe"; Flags: runhidden; RunOnceId: "StopTray"

[Code]
// Stop a running tray before files are replaced (upgrade / reinstall).
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM superlight-tray.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := '';
end;

// Settings, log and mouse-profile backups live in %APPDATA%\superlight-tray.
// Keep them by default: the backups are the way back if a profile write ever goes wrong.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{userappdata}\superlight-tray');
    if DirExists(DataDir) and not UninstallSilent then
      if MsgBox('Also delete your Superlight Tray settings, log and mouse-profile backups?' + #13#10#13#10 +
                DataDir + #13#10#13#10 +
                'Choose No to keep them (recommended if you may reinstall).',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
