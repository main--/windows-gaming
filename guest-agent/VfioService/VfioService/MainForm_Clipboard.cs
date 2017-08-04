using ClientpipeProtocol;
using Google.Protobuf;
using System;
using System.ComponentModel;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace VfioService
{
    public partial class MainForm
    {
        private TaskCompletionSource<ByteString> ClipboardResponse;

        private bool IsGrabbingClipboard = false;

        public bool ShouldGrabClipboard { get; set; } = true;
        public void SetClipboardResponse(ByteString data)
        {
            ClipboardResponse.SetResult(data);
        }

        public void GrabClipboard()
        {
            ShouldGrabClipboard = true;
            // Request the Clipboard immideately.
            ClipboardTimer_Tick(null, null);
        }

        private void ClipboardTimer_Tick(object sender, EventArgs e)
        {
            if (!ShouldGrabClipboard)
                return;

            ClientManager.RequestClipboardContents(ClipboardType.None);
        }

        public void SetClipboardFormats(ClipboardType[] formats)
        {
            var win32formats = formats.Select(a =>
            {
                switch(a)
                {
                    case ClipboardType.Text:
                        return CF_UNICODETEXT;
                    case ClipboardType.Image:
                        return CF_BITMAP;
                    default:
                        return (uint)0;
                }
            }).Where(a => a != 0).ToArray();

            SyncContext.Post(GrabClipboardWith, win32formats);
        }

        private void GrabClipboardWith(object clipboardFormatsObject)
        {
            var clipboardFormats = (uint[])clipboardFormatsObject;

            IsGrabbingClipboard = true;

            while (!OpenClipboard(this.Handle))
                Thread.Yield(); // potential infinite loop but unfixable AFAIK

            if (!EmptyClipboard())
                throw new Win32Exception();

            foreach(var format in clipboardFormats)
                SetClipboardData(format, IntPtr.Zero);

            if (!CloseClipboard())
                throw new Win32Exception();

            IsGrabbingClipboard = false;
        }

        private const uint CF_UNICODETEXT = 13;
        private const uint CF_BITMAP = 2;
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
                    if (!IsGrabbingClipboard)
                    {
                        ShouldGrabClipboard = false;
                        ClientManager.GrabClipboard();
                    }
                    break;
                case WM_RENDERFORMAT:
                    var responseType = ClipboardType.None;
                    switch ((uint)m.WParam.ToInt64())
                    {
                        case CF_UNICODETEXT:
                            responseType = ClipboardType.Text;
                            break;
                        case CF_BITMAP:
                            responseType = ClipboardType.Image;
                            break;
                        default:
                            break;
                    }

                    if (responseType == ClipboardType.None)
                        break;

                    RenderFormat(responseType);

                    break;

                case WM_RENDERALLFORMATS:
                    break;
            }
        }

        private void RenderFormat(ClipboardType format)
        {
            ClientManager.RequestClipboardContents(format);

            // wait for the data to arrive - this has to be blocking :(
            ClipboardResponse = new TaskCompletionSource<ByteString>();
            var result = ClipboardResponse.Task.Result;

            if (format == ClipboardType.Text)
            {
                var resultString = result.ToStringUtf8().Replace("\n", Environment.NewLine);

                // TODO: do this efficiently
                // substrings by newlines, convert pieces etc
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
                SyncContext.Post(_ => GrabClipboard(), null);
            }
            else if (format == ClipboardType.Image)
            {
                using (var mstream = new MemoryStream(result.ToByteArray()))
                using (var bmp = new Bitmap(mstream))
                {
                    var res = SetClipboardData(CF_BITMAP, bmp.GetHbitmap());
                }
            }
        }
    }
}
