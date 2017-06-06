﻿using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Threading.Tasks;

namespace VfioService
{
    public enum CommandOut : byte
    {
        ReportBoot = 0x01,
        IoExit = 0x02,
        Suspending = 0x03,
        Pong = 0x04,
        HotKey = 0x05,
        HotKeyBindingFailed = 0x06,
    }

    public enum CommandIn : byte
    {
        Ping = 0x01,
        RegisterHotKey = 0x02,
    }
}
