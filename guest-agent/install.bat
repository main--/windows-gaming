@echo off
if "%1" == "update" timeout /T 3 /NOBREAK
mkdir %APPDATA%\WindowsGamingGA
copy VfioService.exe %APPDATA%\WindowsGamingGA
copy Google.Protobuf.dll %APPDATA%\WindowsGamingGA
reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /f /v WindowsGamingGA /t REG_SZ /d "%APPDATA%\WindowsGamingGA\VfioService.exe"
%APPDATA%\WindowsGamingGA\VfioService.exe
echo Installation complete.
if NOT "%1" == "update" pause
