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
        GrabClipboard = 0x0a,
        RequestClipboardContents = 0x0b,
        ClipboardContents = 0x0c,
    }

    public enum CommandIn : byte
    {
        Ping = 0x01,
        RegisterHotKey = 0x05,
        ReleaseModifiers = 0x03,
        Suspend = 0x04,
        GrabClipboard = 0x0a,
        RequestClipboardContents = 0x0b,
        ClipboardContents = 0x0c,
    }
}
