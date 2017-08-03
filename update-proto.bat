@ECHO off

WHERE protoc >nul 2>nul
IF %ERRORLEVEL% NEQ 0 (
  ECHO Please install protoc and make sure it's in your PATH
  EXIT
)

mkdir csproto

protoc clientpipe-proto/src/protocol.proto --csharp_out=csproto

copy csproto\Protocol.cs guest-agent\VfioService\VfioService\

rmdir /q /s csproto
