using System;
using System.IO;
using System.Diagnostics;
using System.Linq;

namespace Loader
{
    static class Program
    {
        static void Main()
        {
            var info = DriveInfo.GetDrives().Single(x => x.DriveType == DriveType.CDRom && x.VolumeLabel == "windows-gaming-g");
            var service = info.RootDirectory.GetFiles("VfioService.exe").Single();

            var psi = new ProcessStartInfo
            {
                FileName = service.FullName,
                UseShellExecute = false,
                WorkingDirectory = info.RootDirectory.FullName,
            };
            Process.Start(psi);
        }
    }
}
