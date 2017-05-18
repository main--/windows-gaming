using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Data;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace VfioService
{
    public partial class MainForm : Form
    {
        [Flags]
        public enum HotkeyModifiers : uint
        {
            Alt = 0x0001,
            Control = 0x0002,
            Shift = 0x0004,
            Windows = 0x0008,
            Norepeat = 0x4000,
        }

        public enum VirtualKeyCodes : uint
        {
            Insert = 0x2d,
        }

        [DllImport("User32.dll")]
        private static extern bool RegisterHotKey(IntPtr hwnd, int id, HotkeyModifiers modifiers, VirtualKeyCodes vk);

        private readonly ClientManager ClientManager;

        public MainForm(ClientManager clientManager)
        {
            ClientManager = clientManager;
            if (!RegisterHotKey(Handle, HkIoExit, HotkeyModifiers.Control | HotkeyModifiers.Alt
                | HotkeyModifiers.Norepeat, VirtualKeyCodes.Insert))
                throw new Win32Exception();

            InitializeComponent();
        }

        private const int WmPowerBroadcast = 0x0218;
        private const int WmHotkey = 0x0312;

        private const int PbtApmSuspend = 0x04;
        private const int PbtApmResume = 0x12;

        private const int HkIoExit = 1;

        protected override void WndProc(ref Message m)
        {
            switch (m.Msg)
            {
                case WmHotkey:
                    switch (m.WParam.ToInt64())
                    {
                        case HkIoExit:
                            ClientManager.SendCommand(CommandOut.IoExit);
                            break;
                    }
                    break;
                case WmPowerBroadcast:
                    switch (m.WParam.ToInt64())
                    {
                        case PbtApmSuspend:
                            ClientManager.SendCommand(CommandOut.Suspending);
                            break;
                        case PbtApmResume:
                            ClientManager.SendCommand(CommandOut.ReportBoot);
                            break;
                    }
                    break;
            }

            base.WndProc(ref m);
        }
    }
}
