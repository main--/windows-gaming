using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace VfioService
{
    static class ClipboardManager
    {
        public static Image GetImage()
        {
            Image img = null;
            Exception threadEx = null;
            Thread staThread = new Thread(
                delegate ()
                {
                    try
                    {
                        if (!Clipboard.ContainsImage())
                            return;

                        img = Clipboard.GetImage();
                    }

                    catch (Exception ex)
                    {
                        threadEx = ex;
                    }
                });
            staThread.SetApartmentState(ApartmentState.STA);
            staThread.Start();
            staThread.Join();

            return img;
        }

        public static string GetText()
        {
            string text = null;
            Exception threadEx = null;
            Thread staThread = new Thread(
                delegate ()
                {
                    try
                    {
                        if (!Clipboard.ContainsText())
                            return;

                        text = Clipboard.GetText(TextDataFormat.UnicodeText);
                    }

                    catch (Exception ex)
                    {
                        threadEx = ex;
                    }
                });
            staThread.SetApartmentState(ApartmentState.STA);
            staThread.Start();
            staThread.Join();

            return text;
        }

        private class Bitmap
        {
        }
    }
}
