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
                            var length = ReadInt(Stream);
                            var hotkey = Encoding.UTF8.GetString(ReadBytes(Stream, length));
                            var result = (string)MainForm.Invoke(new Func<int, string, string>(MainForm.RegisterHotKey), id, hotkey);
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
                        case CommandIn.GetClipboard:
                            var clipboardText = ClipboardManager.GetText();
                            if (clipboardText != null)
                            {
                                lock (WriteLock)
                                {
                                    SendCommand(CommandOut.ClipboardText);
                                    // This removes formatting, which I think is a good idea.
                                    var data = Encoding.UTF8.GetBytes(clipboardText);
                                    SendData(BitConverter.GetBytes(data.Length));
                                    SendData(data);
                                }
                            }

                            var clipboardImage = ClipboardManager.GetImage();
                            if (clipboardImage != null)
                            {
                                lock (WriteLock)
                                {
                                    MemoryStream ms = new MemoryStream();
                                    clipboardImage.Save(ms, ImageFormat.Png);
                                    var data = ms.ToArray();

                                    SendCommand(CommandOut.ClipboardPng);
                                    SendData(BitConverter.GetBytes(data.Length));
                                    SendData(data);
                                }
                            }

                            break;
                        case CommandIn.ClipboardText:
                            var clipboarTextLength = ReadInt(Stream);
                            var clipboardString = Encoding.UTF8.GetString(ReadBytes(Stream, clipboarTextLength));
                            ClipboardManager.Set(clipboardString);
                            break;
                        case CommandIn.ClipboardPng:
                            var clipboarImageLength = ReadInt(Stream);
                            var clipboardImageStream = new MemoryStream(ReadBytes(Stream, clipboarImageLength));
                            var decodedImage = Image.FromStream(clipboardImageStream);
                            ClipboardManager.Set(decodedImage);
                            break;

                    }
                }
            }).Start();
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
