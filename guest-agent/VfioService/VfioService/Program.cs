using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace VfioService
{
    static class Program
    {
        [STAThread]
        static void Main()
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            using (var manager = new ClientManager())
            {
                manager.SendCommand(Command.ReportBoot);

                var form = new MainForm(manager);
                var _ = form.Handle; // create form without showing
                Application.Run();
            }
        }
    }
}
