using System.Text.Json;
using Velopack;
using Velopack.Sources;

namespace Ursa.App.Services;

/// <summary>
/// Updates from this repository's GitHub releases, through Velopack. CI publishes each
/// <c>windows-vX.Y.Z</c> release with a feed per architecture (<c>releases.win-x64.json</c>,
/// <c>releases.win-arm64.json</c>) and the full and delta packages beside it; an installed copy
/// follows the channel it was installed from.
/// </summary>
/// <remarks>
/// Ursa checks shortly after it starts and then every few hours, downloads an update in the
/// background, and installs it when Ursa closes (or at once, from Settings). Copies that were
/// not installed (a developer build run from bin\) never update.
/// </remarks>
public static class Updater
{
    public const string Repository = "https://github.com/unrealJune/ursa";
    private static readonly TimeSpan Interval = TimeSpan.FromHours(6);

    private static readonly UpdateManager manager = new(new WindowsReleases(Repository));
    private static readonly SemaphoreSlim busy = new(1, 1);
    private static VelopackAsset? ready;
    private static Timer? timer;

    /// <summary>Raised (on any thread) when <see cref="Status"/> or <see cref="ReadyVersion"/> change.</summary>
    public static event Action? Changed;

    public static bool Supported => manager.IsInstalled;

    /// <summary>The version downloaded and waiting to install, if any.</summary>
    public static string? ReadyVersion => ready?.Version.ToString();

    /// <summary>What the last check found, for Settings.</summary>
    public static string Status { get; private set; } =
        manager.IsInstalled ? "Ursa checks for updates automatically." : "This copy of Ursa was not installed, so it does not update itself.";

    /// <summary>Starts the background checks: the first one a little after start-up, then every few hours.</summary>
    public static void Start()
    {
        if (!manager.IsInstalled || timer is not null) return;
        ready = manager.UpdatePendingRestart;
        timer = new Timer(_ => _ = CheckAsync(), null, TimeSpan.FromSeconds(30), Interval);
    }

    /// <summary>Checks for an update and downloads it. Never throws; the outcome lands in <see cref="Status"/>.</summary>
    public static async Task CheckAsync()
    {
        if (!manager.IsInstalled || !await busy.WaitAsync(0)) return;
        try
        {
            Set("Checking for updates…");
            UpdateInfo? info = await manager.CheckForUpdatesAsync();
            if (info is null)
            {
                Set(ready is null ? $"Ursa is up to date ({Checked()})." : $"Ursa {ReadyVersion} installs when you close Ursa.");
                return;
            }
            string version = info.TargetFullRelease.Version.ToString();
            await manager.DownloadUpdatesAsync(info, p => Set($"Downloading Ursa {version}… {p}%"));
            ready = info.TargetFullRelease;
            Telemetry.Info($"update {version} downloaded");
            Set($"Ursa {version} installs when you close Ursa.");
        }
        catch (Exception e)
        {
            Telemetry.Warn($"update check failed: {e.GetType().Name}: {e.Message}");
            Set($"Could not check for updates ({Checked()}): {e.Message}");
        }
        finally
        {
            busy.Release();
        }
    }

    /// <summary>Installs the downloaded update now and starts the new version.</summary>
    public static async Task RestartNowAsync()
    {
        if (ready is null) return;
        // ursa-host.exe lives in the folder being replaced.
        await App.Host.StopAsync();
        Telemetry.Flush();
        manager.ApplyUpdatesAndRestart(ready);
    }

    /// <summary>Called as Ursa closes: installs a downloaded update silently once this process has gone.</summary>
    public static void ApplyOnExit()
    {
        if (ready is null) return;
        try
        {
            manager.WaitExitThenApplyUpdates(ready, silent: true, restart: false);
        }
        catch (Exception e)
        {
            CrashLog.Write("update on exit", e);
        }
    }

    private static string Checked() => $"checked {DateTime.Now:t}";

    private static void Set(string status)
    {
        Status = status;
        Changed?.Invoke();
    }

    /// <summary>
    /// Velopack's GitHub source reads the newest ten releases, and this repository also releases
    /// the Android app and the node: a run of those would hide every Windows release. This one
    /// reads a hundred and keeps the Windows ones (named "Ursa for Windows X.Y.Z" by CI).
    /// </summary>
    private sealed class WindowsReleases(string repoUrl) : GithubSource(repoUrl, null, false)
    {
        private const string NamePrefix = "Ursa for Windows ";
        private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);

        protected override async Task<GithubRelease[]> GetReleases(bool includePrereleases)
        {
            var uri = new Uri(GetApiBaseUrl(RepoUri), $"repos{RepoUri.AbsolutePath}/releases?per_page=100");
            string body = await Downloader.DownloadString(uri.ToString(), GetRequestHeaders("application/vnd.github.v3+json"));
            var releases = JsonSerializer.Deserialize<List<GithubRelease>>(body, Json) ?? [];
            return releases
                .Where(r => r.Name?.StartsWith(NamePrefix, StringComparison.Ordinal) == true)
                .Where(r => includePrereleases || !r.Prerelease)
                .OrderByDescending(r => r.PublishedAt)
                // The newest, and enough before it for Velopack to find a delta chain.
                .Take(5)
                .ToArray();
        }
    }
}
