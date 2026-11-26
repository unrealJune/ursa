namespace Ursa.App.Services;

/// <summary>Appends unhandled exceptions to crash.log in the app's data (%LOCALAPPDATA%\Ursa, or URSA_DATA_DIR).</summary>
public static class CrashLog
{
    public static string PathName { get; } = System.IO.Path.Combine(LocalStore.Root, "crash.log");

    public static void Write(string where, Exception? e)
    {
        try
        {
            Directory.CreateDirectory(System.IO.Path.GetDirectoryName(PathName)!);
            File.AppendAllText(PathName, $"{DateTimeOffset.Now:O} unhandled ({where}): {e}\r\n\r\n");
        }
        catch { /* nothing left to report to */ }
        // The process is about to end: send it now, not with the next batch.
        Telemetry.Error($"unhandled ({where}): {e?.GetType().Name}: {e?.Message}", e);
        Telemetry.Flush();
    }
}
