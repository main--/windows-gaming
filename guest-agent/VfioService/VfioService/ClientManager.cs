using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Linq;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace VfioService
{
    public class ClientManager : IDisposable
    {
        private readonly TcpClient TcpClient;
        private readonly NetworkStream Stream;
        public object WriteLock { get; } = new object();
        private readonly MainForm MainForm;

        public ClientManager(MainForm mainForm)
        {
            TcpClient = new TcpClient("10.0.2.1", 31337);
            Stream = TcpClient.GetStream();
            MainForm = mainForm;
            new Thread(() =>
            {
                while (true)
                {
                    var nextCommand = (CommandIn)Stream.ReadByte();
                    switch (nextCommand)
                    {
                        case CommandIn.Ping:
                            SendCommand(CommandOut.Pong);
                            break;
                        case CommandIn.RegisterHotKey:
                            var id = ReadInt(Stream);
                            var mods = ReadInt(Stream);
                            var keys = ReadInt(Stream);
                            var result = (string)MainForm.Invoke(new Func<int, int, int, string>(MainForm.RegisterHotKey), id, mods, keys);
                            if (result != null)
                            {
                                // report error
                                lock (WriteLock)
                                {
                                    SendCommand(CommandOut.HotKeyBindingFailed);
                                    var data = Encoding.UTF8.GetBytes(result);
                                    SendData(BitConverter.GetBytes(data.Length));
                                    SendData(data);
                                }
                            }
                            break;
                        case CommandIn.ReleaseModifiers:
                            StuckKeyFix.ReleaseModifiers();
                            break;
                        case CommandIn.Suspend:
                            Application.SetSuspendState(PowerState.Suspend, false, false);
                            break;
                        case CommandIn.GrabClipboard:
                            MainForm.GrabClipboard();
                            break;
                        case CommandIn.RequestClipboardContents:
                            SendClipboardData(Stream.ReadByte());
                            break;
                        case CommandIn.ClipboardContents:
                            var len = ReadInt(Stream);
                            MainForm.SetClipboardResponse(ReadBytes(Stream, len));
                            break;
                    }
                }
            }).Start();
        }

        private void SendClipboardData(int format)
        {
            lock (WriteLock)
            {
                SendCommand(CommandOut.ClipboardContents);
                switch (format)
                {
                    case 0: // utf8 text
                        var clipboardText = (string)MainForm.Invoke(new Func<string>(MainForm.GetClipboardText));
                        if (clipboardText != null)
                        {
                            var data = Encoding.UTF8.GetBytes(clipboardText.Replace("\r\n", "\n"));
                            SendData(BitConverter.GetBytes(data.Length));
                            SendData(data);
                            return;
                        }
                        break;
                        /*
                    case 1: // png image
                        var clipboardImage = (Image)MainForm.Invoke(new Func<Image>(MainForm.GetClipboardImage));
                        if (clipboardImage != null)
                        {
                            MemoryStream ms = new MemoryStream();
                            clipboardImage.Save(ms, ImageFormat.Png);
                            var data = ms.ToArray();

                            SendData(BitConverter.GetBytes(data.Length));
                            SendData(data);
                            return;
                        }
                        break;
                        */
                }

                // unknown or wrong format => send no data
                SendData(BitConverter.GetBytes(0));
            }
        }

        public void Dispose()
        {
            Stream.Dispose();
            ((IDisposable)TcpClient).Dispose();
        }


        public void SendCommand(CommandOut c)
        {
            lock (WriteLock)
            {
                Stream.WriteByte((byte)c);
            }
        }

        public void SendData(byte[] data)
        {
            if (!Monitor.IsEntered(WriteLock))
                throw new InvalidOperationException();

            Stream.Write(data, 0, data.Length);
        }

        private static byte[] ReadBytes(Stream s, int count)
        {
            var buf = new byte[count];
            int read = -1;
            for (int i = 0; i < count; i += read)
                read = s.Read(buf, i, count - i);
            if (read == 0)
                throw new EndOfStreamException();
            return buf;
        }

        private static int ReadInt(Stream s)
        {
            return BitConverter.ToInt32(ReadBytes(s, sizeof(int)), 0);
        }
    }
}
