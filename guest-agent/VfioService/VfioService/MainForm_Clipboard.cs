using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace VfioService
{
    public partial class MainForm
    {
        private TaskCompletionSource<byte[]> ClipboardResponse;

        public void SetClipboardResponse(byte[] data)
        {
            ClipboardResponse.SetResult(data);
        }

        public void GrabClipboard()
        {
            SyncContext.Post(GrabClipboardInternal, null);
        }

        private void GrabClipboardInternal(object _)
        {
            while (!OpenClipboard(this.Handle))
                Thread.Yield(); // potential infinite loop but unfixable AFAIK

            if (!EmptyClipboard())
                throw new Win32Exception();

            SetClipboardData(CF_UNICODETEXT, IntPtr.Zero);

            if (!CloseClipboard())
                throw new Win32Exception();
        }

        private const uint CF_UNICODETEXT = 13;
        private const uint GMEM_MOVABLE = 2;
        private const int WM_RENDERFORMAT = 0x0305;
        private const int WM_RENDERALLFORMATS = 0x0306;
        private const int WM_DESTROYCLIPBOARD = 0x0307;

        [DllImport("Kernel32.dll", SetLastError = true)]
        private static extern IntPtr GlobalAlloc(uint flags, UIntPtr dwBytes);
        [DllImport("Kernel32.dll", SetLastError = true)]
        private static extern IntPtr GlobalLock(IntPtr handle);
        [DllImport("Kernel32.dll", SetLastError = true)]
        private static extern bool GlobalUnlock(IntPtr handle);

        [DllImport("User32.dll", SetLastError = true)]
        private static extern bool OpenClipboard(IntPtr hWndNewOwner);
        [DllImport("User32.dll", SetLastError = true)]
        private static extern bool EmptyClipboard();
        [DllImport("User32.dll", SetLastError = true)]
        private static extern bool CloseClipboard();
        [DllImport("User32.dll", SetLastError = true)]
        private static extern IntPtr SetClipboardData(uint format, IntPtr handle);

        private void WndProcClipboard(ref Message m)
        {
            switch (m.Msg)
            {
                case WM_DESTROYCLIPBOARD:
                    ClientManager.SendCommand(CommandOut.GrabClipboard);
                    break;
                case WM_RENDERFORMAT:
                case WM_RENDERALLFORMATS:
                    lock (ClientManager.WriteLock)
                    {
                        ClientManager.SendCommand(CommandOut.RequestClipboardContents);
                        ClientManager.SendData(new byte[] { 0 }); // format 0 is utf8 text
                    }

                    // wait for the data to arrive - this has to be blocking :(
                    ClipboardResponse = new TaskCompletionSource<byte[]>();
                    var result = ClipboardResponse.Task.Result;

                    // TODO: do this efficiently
                    // substrings by newlines, convert pieces etc
                    var resultString = Encoding.UTF8.GetString(result).Replace("\n", "\r\n");
                    var resultSize = Encoding.Unicode.GetByteCount(resultString);

                    var handle = GlobalAlloc(GMEM_MOVABLE, (UIntPtr)(resultSize + 2));
                    if (handle == IntPtr.Zero)
                        throw new Win32Exception();
                    var buf = GlobalLock(handle);
                    if (buf == IntPtr.Zero)
                        throw new Win32Exception();

                    unsafe
                    {
                        fixed (char* resultChars = resultString)
                        {
                            int bytesWritten = Encoding.Unicode.GetBytes(resultChars, resultString.Length, (byte*)buf, resultSize);
                            *(char*)(buf + bytesWritten) = '\0'; // terminating NUL
                        }
                    }

                    GlobalUnlock(handle);

                    if (SetClipboardData(CF_UNICODETEXT, handle) == IntPtr.Zero)
                        throw new Win32Exception();

                    // Calling this inline would DEADLOCK (!!!) our application 
                    SynchronizationContext.Current.Post(_ => GrabClipboard(), null);
                    break;
            }
        }
    }
}
