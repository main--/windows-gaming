using System;
using System.Runtime.InteropServices;

namespace VfioService
{
    public class StuckKeyFix
    {
        [DllImport("User32.dll", SetLastError = true)]
        private static extern int SendInput(int nInputs, [In, Out, MarshalAs(UnmanagedType.LPArray)] Input[] pInputs, int cbSize);

        [DllImport("User32.dll")]
        private static extern IntPtr GetMessageExtraInfo();

        private enum VirtualKeyCode : ushort
        {
            Shift = 0x10,
            Control = 0x11,
            Menu = 0x12,
        }

        private const int INPUT_KEYBOARD = 0x1;
        private const int KEYEVENTF_KEYUP = 0x2;

        [StructLayout(LayoutKind.Explicit, Size = 28)]
        private struct Input
        {
            [FieldOffset(00)]
            UInt32 type;
            [FieldOffset(04)]
            VirtualKeyCode wVk;
            [FieldOffset(06)]
            UInt16 wScan;
            [FieldOffset(08)]
            UInt32 dwFlags;
            [FieldOffset(12)]
            UInt32 time;
            [FieldOffset(16)]
            IntPtr dwExtraInfo;

            public Input(VirtualKeyCode key)
            {
                type = INPUT_KEYBOARD;
                wVk = key;
                wScan = 0;
                dwFlags = KEYEVENTF_KEYUP;
                time = 0;
                dwExtraInfo = GetMessageExtraInfo();
            }
        }

        public static void ReleaseModifiers()
        {
            var inputs = new Input[]
            {
                new Input(VirtualKeyCode.Shift),
                new Input(VirtualKeyCode.Control),
                new Input(VirtualKeyCode.Menu),
            };

            SendInput(inputs.Length, inputs, Marshal.SizeOf(typeof(Input)));
        }
    }
}
