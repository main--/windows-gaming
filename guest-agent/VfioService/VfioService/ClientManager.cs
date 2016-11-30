using System;
using System.Collections.Generic;
using System.Linq;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace VfioService
{
    public class ClientManager : IDisposable
    {
        private readonly TcpClient TcpClient;
        private readonly NetworkStream Stream;

        public ClientManager()
        {
            TcpClient = new TcpClient("10.0.2.1", 31337);
            Stream = TcpClient.GetStream();
            new Thread(() => {
                while (true) {
                    switch ((CommandIn)Stream.ReadByte()) {
                    case CommandIn.Ping:
                        SendCommand(CommandOut.Pong);
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
            Stream.WriteByte((byte)c);
        }
    }
}
