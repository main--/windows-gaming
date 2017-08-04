using ClientpipeProtocol;
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
using System.Threading;
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
            Ctrl = 0x0002,
            Shift = 0x0004,
            Win = 0x0008,
            NoRepeat = 0x4000,
        }

        [DllImport("User32.dll", SetLastError = true)]
        private static extern bool RegisterHotKey(IntPtr hwnd, int id, HotkeyModifiers modifiers, Keys vk);

        public ClientManager ClientManager { get; set; }

        private readonly SynchronizationContext SyncContext;

        public MainForm()
        {
            InitializeComponent();

            SyncContext = SynchronizationContext.Current;
        }

        public string RegisterHotKey(int id, uint mods, uint keys)
        {
            HotkeyModifiers modifiers = (HotkeyModifiers)mods;
            Keys? key = (Keys)keys;

            if (!RegisterHotKey(Handle, id, modifiers, key.Value))
            {
                var exception = new Win32Exception();
                if ((uint)exception.HResult == 0x80004005)
                    // Hot key is already registered, so we can ignore.
                    return null;
                return "bind error: " + exception;
            }

            return null;
        }
        
        public string GetClipboardText()
        {
            if (!Clipboard.ContainsText())
                return null;

            return Clipboard.GetText(TextDataFormat.UnicodeText);
        }

        public byte[] GetClipboardImage()
        {
            if (!Clipboard.ContainsImage())
                return null;

            using (var ms = new MemoryStream())
            {
                Clipboard.GetImage().Save(ms, System.Drawing.Imaging.ImageFormat.Png);
                return ms.ToArray();
            }
        }
        
        public IEnumerable<ClipboardType> GetClipboardTypes()
        {
            List<ClipboardType> types = new List<ClipboardType>();

            if (Clipboard.ContainsImage())
                types.Add(ClipboardType.Image);

            if (Clipboard.ContainsText())
                types.Add(ClipboardType.Text);

            return types;
        }

        private const int WmPowerBroadcast = 0x0218;
        private const int WmHotkey = 0x0312;

        private const int PbtApmSuspend = 0x04;
        private const int PbtApmResume = 0x12;

        protected override void WndProc(ref Message m)
        {
            switch (m.Msg)
            {
                case WmHotkey:
                    lock (ClientManager.WriteLock)
                    {
                        ClientManager.SendHotkey((uint)m.WParam.ToInt64());
                    }
                    break;
                case WmPowerBroadcast:
                    switch (m.WParam.ToInt64())
                    {
                        case PbtApmSuspend:
                            ClientManager.SendSuspending();
                            break;
                        case PbtApmResume:
                            ClientManager.ReportBoot();
                            break;
                    }
                    break;
            }

            WndProcClipboard(ref m);
            base.WndProc(ref m);
        }
    }
}
