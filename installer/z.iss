; Z installer (Inno Setup 6.3+). Build:  iscc /DVersion=0.1.0 installer\z.iss
; Expects release binaries in ..\target\release\ (Z.exe, zc.exe).
#ifndef Version
  #define Version "0.1.0"
#endif
#define BinDir "..\target\release"

[Setup]
AppId={{6F1D0A52-7C3B-4E8A-9B1E-5A2C7D94E0F1}
AppName=Z
AppVersion={#Version}
AppPublisher=Z contributors
DefaultDirName={autopf}\Z
DefaultGroupName=Z
UninstallDisplayIcon={app}\Z.exe
SetupIconFile=..\assets\icons\z.ico
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=Z-Setup-{#Version}
; One installer for everyone: per-user by default (no admin needed), "all users" offered in a dialog.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Full-height banner and header logo, one bitmap per DPI step (Inno picks the best fit).
WizardImageFile=images\wizard-164x314.bmp,images\wizard-192x386.bmp,images\wizard-246x459.bmp,images\wizard-273x514.bmp,images\wizard-328x628.bmp,images\wizard-355x669.bmp
WizardSmallImageFile=images\small-55x55.bmp,images\small-64x68.bmp,images\small-83x80.bmp,images\small-92x97.bmp,images\small-110x106.bmp,images\small-119x123.bmp,images\small-138x140.bmp
ChangesAssociations=yes
ChangesEnvironment=yes
CloseApplications=yes
RestartApplications=no
DisableProgramGroupPage=yes
ShowLanguageDialog=auto
UsePreviousTasks=yes
UsePreviousAppDir=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"

; ---- What gets installed (everything needed is ticked) ----------------------------------------
[Types]
Name: "full";    Description: "Full installation"
Name: "compact"; Description: "Compact installation (Z only)"
Name: "custom";  Description: "Custom installation"; Flags: iscustom

[Components]
Name: "app"; Description: "Z"; Types: full compact custom; Flags: fixed
Name: "cli"; Description: "Command-line tool (zc)"; Types: full

; ---- What it integrates with (ticked by default unless noted) ---------------------------------
[Tasks]
Name: "ctx_archive"; Description: "Add extract / test commands to the right-click menu of archives"; GroupDescription: "Windows Explorer"
Name: "ctx_all";     Description: "Add ""Add to archive"" to the right-click menu of files and folders"; GroupDescription: "Windows Explorer"
Name: "assoc_zip";   Description: "ZIP (.zip)"; GroupDescription: "Open these file types with Z"
Name: "assoc_7z";    Description: "7-Zip (.7z)"; GroupDescription: "Open these file types with Z"
Name: "assoc_rar";   Description: "RAR (.rar)"; GroupDescription: "Open these file types with Z"
Name: "assoc_tar";   Description: "TAR (.tar .tgz .tbz2 .txz .tzst)"; GroupDescription: "Open these file types with Z"
Name: "assoc_single";Description: "Single-file compressed (.gz .bz2 .xz .zst)"; GroupDescription: "Open these file types with Z"
Name: "addtopath";   Description: "Add zc to PATH"; GroupDescription: "Command line"; Components: cli
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts"; Flags: unchecked
Name: "startmenu";   Description: "Create a Start menu shortcut"; GroupDescription: "Shortcuts"

[Files]
Source: "{#BinDir}\Z.exe";  DestDir: "{app}"; Flags: ignoreversion; Components: app
Source: "{#BinDir}\zc.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: cli
Source: "..\assets\icons\z-archive.ico"; DestDir: "{app}"; Flags: ignoreversion; Components: app
Source: "..\LICENSE";       DestDir: "{app}"; Flags: ignoreversion; Components: app
Source: "..\assets\font\OFL.txt"; DestDir: "{app}"; Flags: ignoreversion; Components: app
; Installer UI font (private to the setup process, never installed on the system)
Source: "..\assets\font\ZSans-Regular.ttf"; DestDir: "{tmp}"; Flags: dontcopy

[Icons]
Name: "{autoprograms}\Z"; Filename: "{app}\Z.exe"; Tasks: startmenu
Name: "{autodesktop}\Z";  Filename: "{app}\Z.exe"; Tasks: desktopicon

#include "registry.inc"

[Registry]
; PATH entry for zc (removed again on uninstall, see [Code])
Root: HKA; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: NeedsAddPath(ExpandConstant('{app}')); Tasks: addtopath

[Run]
Filename: "{app}\Z.exe"; Description: "Launch Z"; Flags: nowait postinstall skipifsilent
Filename: "ms-settings:defaultapps"; Description: "Choose Z as default app for archives"; Flags: shellexec nowait postinstall skipifsilent unchecked

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\Z"

[Code]
const FR_PRIVATE = $10;
function AddFontResourceEx(Name: String; Flags: Cardinal; Reserved: Cardinal): Integer;
  external 'AddFontResourceExW@gdi32.dll stdcall';

const
  clTxt   = $CEC5C1;   // #c1c5ce, the only light colour used
  clHead  = $48141C;   // header strip, same value as the small wizard image background
  clBody  = $320F14;   // page background (dark end of the brand gradient)
  clField = $220B0E;   // text fields and lists

procedure SkinWizard;
var I: Integer; C: TComponent;
begin
  WizardForm.Color := clBody;
  WizardForm.Bevel.Visible := False;
  WizardForm.Bevel1.Visible := False;
  for I := 0 to WizardForm.ComponentCount - 1 do
  begin
    C := WizardForm.Components[I];
    if C is TNewNotebookPage then TNewNotebookPage(C).Color := clBody
    else if C is TPanel then TPanel(C).Color := clBody
    else if C is TNewStaticText then
    begin
      TNewStaticText(C).Font.Name := 'Z Sans';
      TNewStaticText(C).Font.Color := clTxt;
    end
    else if C is TNewCheckBox then
    begin
      TNewCheckBox(C).Font.Name := 'Z Sans';
      TNewCheckBox(C).Font.Color := clTxt;
    end
    else if C is TNewRadioButton then
    begin
      TNewRadioButton(C).Font.Name := 'Z Sans';
      TNewRadioButton(C).Font.Color := clTxt;
    end
    else if C is TNewCheckListBox then
    begin
      TNewCheckListBox(C).Font.Name := 'Z Sans';
      TNewCheckListBox(C).Font.Color := clTxt;
      TNewCheckListBox(C).Color := clField;
    end
    else if C is TNewMemo then
    begin
      TNewMemo(C).Font.Name := 'Z Sans';
      TNewMemo(C).Font.Color := clTxt;
      TNewMemo(C).Color := clField;
    end
    else if C is TRichEditViewer then
    begin
      TRichEditViewer(C).Font.Name := 'Z Sans';
      TRichEditViewer(C).Font.Color := clTxt;
      TRichEditViewer(C).Color := clField;
    end
    else if C is TEdit then
    begin
      TEdit(C).Font.Name := 'Z Sans';
      TEdit(C).Font.Color := clTxt;
      TEdit(C).Color := clField;
    end;
  end;
  WizardForm.MainPanel.Color := clHead;
end;

procedure InitializeWizard;
var f: String;
begin
  // Z Sans for the whole installer (loaded privately for this process, never installed on the system).
  ExtractTemporaryFile('ZSans-Regular.ttf');
  f := ExpandConstant('{tmp}\ZSans-Regular.ttf');
  if AddFontResourceEx(f, FR_PRIVATE, 0) > 0 then
    WizardForm.Font.Name := 'Z Sans';
  SkinWizard;
end;

function NeedsAddPath(Param: string): Boolean;
var Paths, Key: string; Root: Integer;
begin
  if IsAdminInstallMode then begin Root := HKLM; Key := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'; end
  else begin Root := HKCU; Key := 'Environment'; end;
  if not RegQueryStringValue(Root, Key, 'Path', Paths) then begin Result := True; exit; end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(Paths) + ';') = 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var Paths, Key, App: string; Root, P: Integer;
begin
  if CurUninstallStep <> usPostUninstall then exit;
  if IsAdminInstallMode then begin Root := HKLM; Key := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'; end
  else begin Root := HKCU; Key := 'Environment'; end;
  if not RegQueryStringValue(Root, Key, 'Path', Paths) then exit;
  App := ExpandConstant('{app}');
  P := Pos(';' + Uppercase(App), Uppercase(Paths));
  if P > 0 then
  begin
    Delete(Paths, P, Length(App) + 1);
    RegWriteExpandStringValue(Root, Key, 'Path', Paths);
  end;
end;
