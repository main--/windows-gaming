@echo off
reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /f /v WindowsGamingGA
taskkill /im VfioService.exe
del %APPDATA%\WindowsGamingGA\VfioService.exe
rmdir %APPDATA%\WindowsGamingGA
echo Uninstall complete.
pause
