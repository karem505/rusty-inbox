; Inno Setup script. Build: iscc /DAppVersion=0.1.0 /DSourceDir=<deploy dir> /DOutDir=<out> /DOutName=<name> rusty-inbox.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\..\dist\rusty-inbox"
#endif
#ifndef OutDir
  #define OutDir "."
#endif
#ifndef OutName
  #define OutName "rusty-inbox-setup"
#endif

[Setup]
AppId={{6E3F4B0C-5A1D-4C8E-9B27-3A5D7C1F9E02}
AppName=Rusty Inbox
AppVersion={#AppVersion}
AppPublisher=karem505
AppPublisherURL=https://github.com/karem505/rusty-inbox
DefaultDirName={localappdata}\Programs\Rusty Inbox
DefaultGroupName=Rusty Inbox
PrivilegesRequired=lowest
OutputDir={#OutDir}
OutputBaseFilename={#OutName}
SetupIconFile=rusty-inbox.ico
UninstallDisplayIcon={app}\rusty-inbox.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
DisableProgramGroupPage=yes
LicenseFile={#SourceDir}\LICENSE-MIT

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "mailto"; Description: "Register as a mailto: handler"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\Rusty Inbox"; Filename: "{app}\rusty-inbox.exe"
Name: "{autodesktop}\Rusty Inbox"; Filename: "{app}\rusty-inbox.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\RustyInbox.Url.mailto"; ValueType: string; ValueData: "URL:mailto"; Flags: uninsdeletekey; Tasks: mailto
Root: HKCU; Subkey: "Software\Classes\RustyInbox.Url.mailto"; ValueName: "URL Protocol"; ValueType: string; ValueData: ""; Tasks: mailto
Root: HKCU; Subkey: "Software\Classes\RustyInbox.Url.mailto\DefaultIcon"; ValueType: string; ValueData: "{app}\rusty-inbox.exe,0"; Tasks: mailto
Root: HKCU; Subkey: "Software\Classes\RustyInbox.Url.mailto\shell\open\command"; ValueType: string; ValueData: """{app}\rusty-inbox.exe"" ""%1"""; Tasks: mailto
Root: HKCU; Subkey: "Software\RustyInbox\Capabilities"; ValueName: "ApplicationName"; ValueType: string; ValueData: "Rusty Inbox"; Flags: uninsdeletekey; Tasks: mailto
Root: HKCU; Subkey: "Software\RustyInbox\Capabilities"; ValueName: "ApplicationDescription"; ValueType: string; ValueData: "Fast native email client"; Tasks: mailto
Root: HKCU; Subkey: "Software\RustyInbox\Capabilities\URLAssociations"; ValueName: "mailto"; ValueType: string; ValueData: "RustyInbox.Url.mailto"; Tasks: mailto
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueName: "Rusty Inbox"; ValueType: string; ValueData: "Software\RustyInbox\Capabilities"; Flags: uninsdeletevalue; Tasks: mailto

[Run]
Filename: "{app}\rusty-inbox.exe"; Description: "Launch Rusty Inbox"; Flags: nowait postinstall skipifsilent
; in-app updates run this installer with /SILENT: start the new version when it is done
Filename: "{app}\rusty-inbox.exe"; Flags: nowait; Check: WizardSilent
