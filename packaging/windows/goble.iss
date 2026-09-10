; ---------------------------------------------------------------------------
; Goble — Inno Setup 6 script (per-user install)
; ---------------------------------------------------------------------------
;
; Build with build-installer.ps1 (recommended) or directly:
;
;   ISCC.exe /DMyAppVersion=0.1.0 ^
;            /DSourceBinary=C:\path\to\target\release\goble-app.exe ^
;            /DOutputDir=C:\path\to\dist ^
;            goble.iss
;
; NOTE: LicenseFile is resolved relative to this script, so this file must be
; compiled from a directory that also contains EULA.txt. build-installer.ps1
; stages a copy of both in a temp directory with @VERSION@/@DATE@ substituted
; into the EULA. A plain IDE compile works only if you place a copy of
; packaging/EULA.txt next to goble.iss first.
;
; ---------------------------------------------------------------------------
; AUTO-UPDATER CONTRACT
; ---------------------------------------------------------------------------
; The updater runs the previously installed Setup executable with:
;
;   Goble-<version>-win-x64-setup.exe /SILENT /NORESTART /NOCLOSEAPPLICATIONS \
;       /SUPPRESSMSGBOXES /DIR="<current install dir>" /update=1
;
; /SILENT              progress window only, no questions
; /NORESTART           never reboot; the app relaunches from the new binary
; /NOCLOSEAPPLICATIONS do not force-close Goble (the updater stops it first)
; /SUPPRESSMSGBOXES    never block on a message box
; /DIR=<dir>           install back into the directory detected for this user
; /update=1            custom flag read by [Code] below: skips the licence page
;                      and suppresses the post-install "Launch Goble" checkbox
;
; The install directory is recorded in the registry under
;   HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{AppId}_is1
; (value "InstallLocation"), which is where the updater reads it from.
;
; AppMutex below is the single-instance contract: Goble must create a named
; mutex "Goble-App-SingleInstance" at startup (CreateMutexW) for Setup's
; "close the running application" logic and the updater to detect a live
; instance. Keep the two names in sync.
; ---------------------------------------------------------------------------

#ifndef MyAppName
  #define MyAppName "Goble"
#endif

#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif

#ifndef MyAppPublisher
  #define MyAppPublisher "Goble Contributors"
#endif

#ifndef MyAppURL
  #define MyAppURL "https://github.com/AdrianTuci1/goble"
#endif

#ifndef MyExeName
  #define MyExeName "goble-app.exe"
#endif

#ifndef SourceBinary
  #define SourceBinary "..\..\target\release\goble-app.exe"
#endif

#ifndef AppIconPath
  #define AppIconPath "..\..\crates\goble-desktop-native\resources\icons\icon.ico"
#endif

#ifndef RepoLicense
  #define RepoLicense "..\..\LICENSE"
#endif

#ifndef NoticesSource
  #define NoticesSource "..\..\THIRD-PARTY-NOTICES.md"
#endif

#ifndef OutDir
  #define OutDir "..\..\dist"
#endif

#ifdef ARM64_BUILD
  #define OutputSuffix "arm64"
#else
  #define OutputSuffix "x64"
#endif

[Setup]
; Fixed GUID: never change it, or upgrades install side by side and the
; uninstall registry key the updater reads becomes orphaned.
AppId={{8D3E7B4A-2C51-4F0E-9A6D-7B1C4E2F5A83}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppVersion}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}

; Per-user install: no UAC prompt. {autopf} resolves to
; %LocalAppData%\Programs for a non-elevated install.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes
UninstallDisplayName={#MyAppName}
UninstallDisplayIcon={app}\goble.ico

#ifdef ARM64_BUILD
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

; The terminal opens a pseudo-console with portable-pty, which is ConPTY on
; Windows. ConPTY first shipped in 1809 (build 17763) and the API only settled
; in 1903, so refuse to install below that rather than shipping an app whose
; terminal cannot start. Inno's own default allows Windows 7.
MinVersion=10.0.18362

; Licence page. Keep the file name literal: build-installer.ps1 stages a
; rendered EULA.txt next to the compiled copy of this script.
LicenseFile=EULA.txt

OutputDir={#OutDir}
OutputBaseFilename={#MyAppName}-{#MyAppVersion}-win-{#OutputSuffix}-setup
SetupIconFile={#AppIconPath}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableWelcomePage=no

; Installing over a running instance: ask it to close instead of failing, and
; keep the app closed (the updater relaunches it explicitly).
CloseApplications=yes
RestartApplications=no
CloseApplicationsFilter=*.exe

; Single-instance contract shared with the application. See header comment.
AppMutex=Goble-App-SingleInstance

; No PATH/file-association/environment changes: Goble is self-contained.
ChangesAssociations=no
ChangesEnvironment=no

; Signing. Enabled only when the caller passes /DSIGN_TOOL and defines the
; "signtool" command via /Ssigntool=<command line>. Unsigned local builds
; compile unchanged.
#ifdef SIGN_TOOL
SignTool=signtool
SignedUninstaller=yes
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startupicon"; Description: "Start {#MyAppName} when you sign in"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "{#SourceBinary}"; DestDir: "{app}"; DestName: "{#MyExeName}"; Flags: ignoreversion
Source: "{#AppIconPath}"; DestDir: "{app}"; DestName: "goble.ico"; Flags: ignoreversion
Source: "EULA.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoLicense}"; DestDir: "{app}"; DestName: "LICENSE"; Flags: ignoreversion
#ifndef NoNotices
Source: "{#NoticesSource}"; DestDir: "{app}"; DestName: "THIRD-PARTY-NOTICES.md"; Flags: ignoreversion
#endif

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyExeName}"; IconFilename: "{app}\goble.ico"; Comment: "{#MyAppName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyExeName}"; IconFilename: "{app}\goble.ico"; Tasks: desktopicon
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyExeName}"; Tasks: startupicon

[Run]
Filename: "{app}\{#MyExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent; Check: CanLaunchApp

[UninstallDelete]
; Only the updater's scratch directory. User configuration under
; %LocalAppData%\Goble and the SQLite store are intentionally left in place.
Name: "{localappdata}\{#MyAppName}\updates"; Type: filesandordirs

[Code]
{ Custom /update=1 flag passed by the auto-updater. }
function IsUpdateMode: Boolean;
begin
  Result := ExpandConstant('{param:update|0}') = '1';
end;

{ Skip the licence page on an update: the user already accepted it on the
  original install, and the updater runs unattended. }
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := IsUpdateMode and (PageID = wpLicense);
end;

{ In update mode the updater restarts Goble itself; do not offer the
  post-install launch checkbox. }
function CanLaunchApp: Boolean;
begin
  Result := not IsUpdateMode;
end;
