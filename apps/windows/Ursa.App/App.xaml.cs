using Microsoft.UI.Xaml;
using Ursa.App.Services;

namespace Ursa.App;

public partial class App : Application
{
    public static MainWindow? Window { get; private set; }
    public static HostClient Host { get; } = new();
    public static Library Library { get; } = new(Host);

    public App() => InitializeComponent();

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Window = new MainWindow();
        Window.Activate();
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
        var files = new List<Windows.Storage.StorageFile>();
        foreach (string path in argv.Skip(at + 1).Where(File.Exists))
            files.Add(await Windows.Storage.StorageFile.GetFileFromPathAsync(Path.GetFullPath(path)));
        if (files.Count > 0) await Library.UploadAsync(files);
    }
}
