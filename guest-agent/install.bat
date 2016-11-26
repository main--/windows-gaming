@echo off
mkdir %APPDATA%\WindowsGamingGA
copy VfioService.exe %APPDATA%\WindowsGamingGA
reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Run /f /v WindowsGamingGA /t REG_SZ /d "%APPDATA%\WindowsGamingGA\VfioService.exe"
echo Installation complete.
pause