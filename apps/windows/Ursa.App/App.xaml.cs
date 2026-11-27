using Microsoft.UI.Xaml;
using Ursa.App.Services;

namespace Ursa.App;

public partial class App : Application
{
    public static MainWindow? Window { get; private set; }
    public static HostClient Host { get; } = new();
    public static Library Library { get; } = new(Host);

    public App()
    {
        Telemetry.Start();
        InitializeComponent();
        // A crash otherwise leaves only "0xc0000409 in ntdll" in the event log. This records the
        // managed exception behind it; a failure inside WinUI's native code still goes unrecorded.
        UnhandledException += (_, e) => CrashLog.Write("UI thread", e.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, e) => CrashLog.Write("thread", e.ExceptionObject as Exception);
        TaskScheduler.UnobservedTaskException += (_, e) => CrashLog.Write("unobserved task", e.Exception);
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Window = new MainWindow();
        Window.Activate();
        Updater.Start();
        _ = StartAsync();
    }

    /// <summary>
    /// Starts the library, then uploads any files named on the command line
    /// (<c>Ursa.exe --upload a.jpg b.mov</c>), the hook for Explorer's "Send to".
    /// </summary>
    private static async Task StartAsync()
    {
        await Library.StartAsync();
        string[] argv = Environment.GetCommandLineArgs();
        int at = Array.IndexOf(argv, "--upload");
        if (at < 0 || !Library.IsConnected) return;
        // Folders go up whole, as folders of the library.
        var items = new List<Windows.Storage.IStorageItem>();
        foreach (string path in argv.Skip(at + 1))
        {
            if (File.Exists(path)) items.Add(await Windows.Storage.StorageFile.GetFileFromPathAsync(Path.GetFullPath(path)));
            else if (Directory.Exists(path)) items.Add(await Windows.Storage.StorageFolder.GetFolderFromPathAsync(Path.GetFullPath(path)));
        }
        if (items.Count > 0)
        {
            try { await Library.UploadIntoAsync(items, null); }
            catch (Exception e) { Library.Error = e.Message; }
        }
    }
}
