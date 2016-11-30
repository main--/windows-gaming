using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Threading.Tasks;

namespace VfioService
{
    public enum Command : byte
    {
        ReportBoot = 0x01,
        IoExit = 0x02,
        Suspending = 0x03,
    }
}
