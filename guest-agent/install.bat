@echo off
copy Loader.exe %APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\VfioLoader.exe
start VfioService.exe
echo Installation complete.
pause
