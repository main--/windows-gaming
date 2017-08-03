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
using ClientpipeProtocol;
using Google.Protobuf;

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
                    var outCmd = GaCmdOut.Parser.ParseDelimitedFrom(Stream);

                    switch (outCmd.MessageCase)
                    {
                        case GaCmdOut.MessageOneofCase.Ping:
                            Send(new GaCmdIn { Pong = new Unit() });
                            break;
                        case GaCmdOut.MessageOneofCase.RegisterHotKey:
                            HandleRegisterHotkey(outCmd.RegisterHotKey);
                            break;
                        case GaCmdOut.MessageOneofCase.ReleaseModifiers:
                            StuckKeyFix.ReleaseModifiers();
                            break;
                        case GaCmdOut.MessageOneofCase.Suspend:
                            Application.SetSuspendState(PowerState.Suspend, false, false);
                            break;
                        case GaCmdOut.MessageOneofCase.Clipboard:
                            HandleClipboardMessage(outCmd.Clipboard);
                            break;

                    }
                }
            }).Start();
        }

        private void HandleClipboardMessage(ClipboardMessage msg)
        {
            switch (msg.MessageCase)
            {
                case ClipboardMessage.MessageOneofCase.GrabClipboard:
                    MainForm.GrabClipboard();
                    break;
                case ClipboardMessage.MessageOneofCase.RequestClipboardContents:
                    SendClipboardData();
                    break;
                case ClipboardMessage.MessageOneofCase.ClipboardContents:
                    MainForm.SetClipboardResponse(msg.ClipboardContents.ToStringUtf8().Replace("\n", Environment.NewLine));
                    break;

            }
        }

        private void HandleRegisterHotkey(RegisterHotKey hotkey)
        {
            var result = (string)MainForm.Invoke(new Func<int, uint, uint, string>(MainForm.RegisterHotKey), (int)hotkey.Id, hotkey.Modifiers, hotkey.Key);
            if (result != null)
            {
                Send(new GaCmdIn {
                    HotKeyBindingFailed = result
                });
            }
        }

        private void SendClipboardData()
        {
            var clipboardText = (string)MainForm.Invoke(new Func<string>(MainForm.GetClipboardText));
            if (clipboardText != null)
            {
                var message = new GaCmdIn();
                message.Clipboard = new ClipboardMessage();
                message.Clipboard.ClipboardContents = ByteString.CopyFromUtf8(clipboardText.Replace(Environment.NewLine, "\n"));
                Send(message);
            }
        }

        public void SendHotkey(uint hotkey)
        {
            Send(new GaCmdIn
            {
                HotKey = hotkey
            });
        }

        public void SendSuspending()
        {
            Send(new GaCmdIn
            {
                Suspending = new Unit()
            });
        }


        public void ReportBoot()
        {
            Send(new GaCmdIn
            {
                ReportBoot = new Unit(),
            });
        }

        public void GrabClipboard()
        {
            Send(new GaCmdIn
            {
                Clipboard = new ClipboardMessage
                {
                    GrabClipboard = new Unit()
                }
            });
        }

        public void RequestClipboardContents()
        {
            Send(new GaCmdIn
            {
                Clipboard = new ClipboardMessage
                {
                    RequestClipboardContents = false
                }
            });
        }

        private void Send(GaCmdIn toSend)
        {
            lock (WriteLock)
            {
                toSend.WriteDelimitedTo(Stream);
                Stream.Flush();
            }
        }

        public void Dispose()
        {
            Stream.Dispose();
            ((IDisposable)TcpClient).Dispose();
        }
    }
}
