namespace Ursa.App.Services;

/// <summary>Appends unhandled exceptions to %LOCALAPPDATA%\Ursa\crash.log.</summary>
public static class CrashLog
{
    public static string PathName { get; } =
        System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Ursa", "crash.log");

    public static void Write(string where, Exception? e)
    {
        try
        {
            Directory.CreateDirectory(System.IO.Path.GetDirectoryName(PathName)!);
            File.AppendAllText(PathName, $"{DateTimeOffset.Now:O} unhandled ({where}): {e}\r\n\r\n");
        }
        catch { /* nothing left to report to */ }
    }
}
