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

            var form = new MainForm();
            using (var manager = new ClientManager(form))
            {
                form.ClientManager = manager;
                manager.ReportBoot();

                var _ = form.Handle; // create form without showing
                Application.Run();
            }
        }
    }
}
