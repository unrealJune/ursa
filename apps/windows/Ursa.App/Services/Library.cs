using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Storage;

namespace Ursa.App.Services;

/// <summary>
/// The app's state: the host, the node connection, the library and what is being transferred.
/// Pages bind to it; every change lands on the UI thread.
/// </summary>
public sealed class Library(HostClient host) : INotifyPropertyChanged
{
    private DispatcherQueue? ui;
    private readonly SemaphoreSlim thumbGate = new(4, 4);

    public ObservableCollection<GalleryItem> Items { get; } = [];

    // -------------------------------------------------------------------------------------
    // State the UI shows

    private bool ready, connected, hasAccount, busy;
    private string statusText = "Starting", detailText = "", deviceId = "", accountId = "", accountName = "";
    private string? error;
    private int homeNodeCount;

    public bool IsReady { get => ready; private set => Set(ref ready, value); }
    /// <summary>At least one node that takes this account's data answered.</summary>
    public bool IsConnected { get => connected; private set => Set(ref connected, value); }
    public bool HasAccount { get => hasAccount; private set { Set(ref hasAccount, value); Changed(nameof(NeedsNode)); Changed(nameof(IsConfigured)); } }
    /// <summary>The account has no node of its own yet: onboarding requires one.</summary>
    public bool NeedsNode => HasAccount && homeNodeCount == 0;
    public bool IsConfigured => HasAccount && homeNodeCount > 0;
    public bool IsBusy { get => busy; private set => Set(ref busy, value); }
    public string StatusText { get => statusText; private set => Set(ref statusText, value); }
    public string DetailText { get => detailText; private set => Set(ref detailText, value); }
    public string DeviceId { get => deviceId; private set => Set(ref deviceId, value); }
    public string AccountId { get => accountId; private set => Set(ref accountId, value); }
    public string AccountName { get => accountName; private set => Set(ref accountName, value); }
    public string? Error
    {
        get => error;
        set
        {
            // What the error bar shows is what the collector gets.
            if (value is not null && value != error) Telemetry.Warn($"shown: {value}");
            Set(ref error, value);
        }
    }
    /// <summary>Why opening a stored account failed (its nodes unreachable), if it did.</summary>
    public string? OpenError { get; private set; }

    public ObservableCollection<DeviceRow> Devices { get; } = [];
    public ObservableCollection<PoolRow> Pool { get; } = [];
    public ObservableCollection<FriendRow> Friends { get; } = [];

    public event Action? AccountChanged;

    // Transfers: one bar for everything in flight.
    private int transfersActive, transfersDone, transfersTotal;
    private long bytesDone, bytesTotal;
    private string transferText = "";
    public bool IsTransferring => transfersActive > 0;
    public double TransferPercent => bytesTotal > 0 ? 100.0 * bytesDone / bytesTotal : 0;
    public string TransferText { get => transferText; private set => Set(ref transferText, value); }

    // -------------------------------------------------------------------------------------
    // Start, account, pool

    public async Task StartAsync()
    {
        ui ??= DispatcherQueue.GetForCurrentThread();
        host.Exited += reason => OnUi(() => { IsReady = false; IsConnected = false; StatusText = "Stopped"; Error = reason; });
        try
        {
            host.Start();
            var settings = LocalStore.LoadSettings();
            var init = await host.CallAsync("init", new
            {
                data_dir = LocalStore.HostDataDir,
                account_key = LocalStore.LoadAccountKey(),
                relays = settings.Relays,
                relay_token = settings.RelayToken,
                device_name = Environment.MachineName,
                platform = "windows",
            });
            DeviceId = init.GetProperty("device_id").GetString() ?? "";
            OpenError = init.TryGetProperty("open_error", out var oe) ? oe.GetString() : null;
            IsReady = true;
            await RefreshAccountAsync();
            if (HasAccount)
            {
                await RefreshPoolAsync();
                if (IsConnected)
                {
                    await RefreshAsync();
                    _ = ResumeSwitchAsync();
                }
            }
        }
        catch (Exception e)
        {
            StatusText = "Not running";
            Error = e.Message;
        }
        AccountChanged?.Invoke();
    }

    /// <summary>Restarts the host, e.g. after the relays changed.</summary>
    public async Task RestartAsync()
    {
        IsReady = false;
        await host.StopAsync();
        await StartAsync();
    }

    /// <param name="fromNodes">Re-read the account from its nodes first, for what other devices
    /// changed (a phone that joined, photos it uploaded); otherwise what this device last read.</param>
    public async Task RefreshAccountAsync(bool fromNodes = false)
    {
        var s = await host.CallAsync(fromNodes && HasAccount ? "refresh" : "status");
        HasAccount = s.GetProperty("has_account").GetBoolean();
        if (!HasAccount)
        {
            StatusText = "No account";
            DetailText = "";
            return;
        }
        AccountId = s.GetProperty("account_id").GetString() ?? "";
        AccountName = s.GetProperty("name").GetString() ?? "";
        homeNodeCount = s.GetProperty("home_nodes").GetArrayLength();
        Changed(nameof(NeedsNode));
        Changed(nameof(IsConfigured));
        Devices.Clear();
        foreach (var d in s.GetProperty("devices").EnumerateArray())
            Devices.Add(new DeviceRow(d.GetProperty("name").GetString() ?? "", d.GetProperty("platform").GetString() ?? "",
                d.GetProperty("device").GetString() ?? "", d.GetProperty("device").GetString() == DeviceId));
        Friends.Clear();
        foreach (var f in s.GetProperty("friends").EnumerateArray())
            Friends.Add(new FriendRow(f.GetProperty("name").GetString() ?? "", f.GetProperty("account").GetString() ?? "", f.GetProperty("nodes").GetInt32()));
    }

    public async Task RefreshPoolAsync()
    {
        var r = await host.CallAsync("pool");
        Pool.Clear();
        int writable = 0, direct = 0;
        foreach (var n in r.GetProperty("nodes").EnumerateArray())
        {
            var row = PoolRow.FromJson(n);
            Pool.Add(row);
            if (row.Reachable && row.Role != "none") writable++;
            if (row.Path == "direct") direct++;
        }
        IsConnected = writable > 0;
        if (!HasAccount) { StatusText = "No account"; DetailText = ""; }
        else if (homeNodeCount == 0) { StatusText = "No node"; DetailText = "Add a node you own in Settings"; }
        else if (IsConnected) { StatusText = "Connected"; DetailText = $"{writable} of {Pool.Count} nodes reachable · {direct} direct"; }
        else { StatusText = "Offline"; DetailText = "None of your nodes answered"; }
    }

    /// <summary>Account (re-read from its nodes), then pool: the refresh a person asks for.</summary>
    public async Task RefreshStatusAsync()
    {
        await RefreshAccountAsync(fromNodes: true);
        if (HasAccount) await RefreshPoolAsync();
    }

    private async Task AdoptAccountAsync(JsonElement r)
    {
        LocalStore.SaveAccountKey(r.GetProperty("account_key").GetString()!);
        // A link hands over the other device's relays and token, and the host already runs on
        // them: keep them for the next start (and for Settings › Relays).
        if (r.TryGetProperty("relays", out var relays) && relays.ValueKind == JsonValueKind.Array)
        {
            var settings = LocalStore.LoadSettings();
            settings.Relays = [.. relays.EnumerateArray().Select(u => u.GetString() ?? "").Where(u => u.Length > 0)];
            settings.RelayToken = r.TryGetProperty("relay_token", out var t) && t.ValueKind == JsonValueKind.String ? t.GetString() : null;
            LocalStore.SaveSettings(settings);
        }
        OpenError = null;
        await RefreshAccountAsync();
        await RefreshPoolAsync();
        if (IsConnected) await RefreshAsync();
        AccountChanged?.Invoke();
    }

    public async Task CreateAccountAsync(string name) =>
        await AdoptAccountAsync(await host.CallAsync("create", new { name }));

    public async Task RecoverFromPhraseAsync(string phrase) =>
        await AdoptAccountAsync(await host.CallAsync("recover", new { phrase }));

    public async Task RecoverFromKeyFileAsync(string keyFileJson, string passphrase) =>
        await AdoptAccountAsync(await host.CallAsync("recover", new { key_file = keyFileJson, passphrase }));

    /// <summary>Joins an account by a link code from another device; <paramref name="onConfirmation"/> gets the 6-digit code to show.</summary>
    public async Task JoinAsync(string code, Action<string> onConfirmation) =>
        await AdoptAccountAsync(await host.CallAsync("link_join", new { code = code.Trim() }, onEvent: (name, e) =>
        {
            if (name == "link_confirm") OnUi(() => onConfirmation(e.GetProperty("code").GetString() ?? ""));
        }));

    /// <summary>
    /// Offers a link code to a new device. <paramref name="onCode"/> gets the code to show;
    /// <paramref name="ask"/> is asked whether to let a device in, given its name, platform and
    /// the confirmation code both screens show. Returns the linked device's name, or null.
    /// </summary>
    public async Task<string?> OfferLinkAsync(Action<string> onCode, Func<string, string, string, Task<bool>> ask)
    {
        var r = await host.CallAsync("link_offer", onEvent: (name, e) =>
        {
            if (name == "link_code") OnUi(() => onCode(e.GetProperty("code").GetString() ?? ""));
            else if (name == "link_request")
            {
                string who = e.GetProperty("name").GetString() ?? "", platform = e.GetProperty("platform").GetString() ?? "", code = e.GetProperty("code").GetString() ?? "";
                OnUi(async () =>
                {
                    bool ok = await ask(who, platform, code);
                    try { await host.CallAsync("link_decide", new { accept = ok }); } catch { }
                });
            }
        });
        await RefreshAccountAsync();
        return r.TryGetProperty("linked", out var l) && l.ValueKind == JsonValueKind.String ? l.GetString() : null;
    }

    public async Task CancelLinkAsync()
    {
        try { await host.CallAsync("link_cancel"); } catch { }
    }

    public async Task ClaimNodeAsync(string link, string label)
    {
        await host.CallAsync("claim", new { link = link.Trim(), label });
        await RefreshAccountAsync();
        await RefreshPoolAsync();
        if (IsConnected) await RefreshAsync();
        AccountChanged?.Invoke();
    }

    public async Task<string> InviteAsync() => (await host.CallAsync("invite")).GetProperty("invite").GetString() ?? "";

    public async Task BefriendAsync(string invite, long quotaGb)
    {
        await host.CallAsync("befriend", new { invite = invite.Trim(), quota_gb = quotaGb });
        await RefreshAccountAsync();
        await RefreshPoolAsync();
    }

    public async Task<string> RecoveryPhraseAsync() => (await host.CallAsync("recovery_phrase")).GetProperty("phrase").GetString() ?? "";

    public async Task<string> KeyFileAsync(string passphrase) =>
        (await host.CallAsync("key_file", new { passphrase })).GetProperty("key_file").GetString() ?? "";

    /// <summary>The storage report; <paramref name="extra"/> adds a shape being considered to the choices.</summary>
    public async Task<StorageReport> StorageAsync((int K, int N)? extra = null) =>
        StorageReport.FromJson(await host.CallAsync("storage", extra is { } x ? new { k = x.K, n = x.N } : null));

    // -------------------------------------------------------------------------------------
    // Switching protection. Runs here, not in a page: it keeps going while you use the rest of
    // the app, and the choice is saved first, so a switch cut short by closing the app picks up
    // again at the next start.

    private bool switching;
    private double switchPercent;
    private string? switchError;
    public bool IsSwitching { get => switching; private set => Set(ref switching, value); }
    public double SwitchPercent { get => switchPercent; private set => Set(ref switchPercent, value); }
    public string? SwitchError { get => switchError; set => Set(ref switchError, value); }
    /// <summary>Raised on the UI thread when a switch ends, finished or not.</summary>
    public event Action? SwitchEnded;

    /// <summary>Chooses <paramref name="k"/>-of-<paramref name="n"/> and moves the library to it in the background.</summary>
    public async Task StartSwitchAsync(int k, int n)
    {
        if (IsSwitching) return;
        await host.CallAsync("set_protection", new { k, n });
        _ = RecodeInBackgroundAsync();
    }

    /// <summary>
    /// At start: finishes a switch that was cut short (and clears what it left), then sweeps
    /// pieces nothing uses that are old enough to be sure of. Quiet: the next start tries again.
    /// </summary>
    private async Task ResumeSwitchAsync()
    {
        try
        {
            var r = await StorageAsync();
            if (r.PendingFiles > 0 || r.LeftoverPieces > 0) await RecodeInBackgroundAsync();
            await host.CallAsync("gc");
        }
        catch { /* nodes down: the next start tries again */ }
    }

    private async Task RecodeInBackgroundAsync()
    {
        if (IsSwitching) return;
        IsSwitching = true;
        SwitchPercent = 0;
        SwitchError = null;
        try
        {
            var r = (await host.CallAsync("recode", progress: new Progress<(long Done, long Total)>(p =>
            {
                if (p.Total > 0) SwitchPercent = 100.0 * p.Done / p.Total;
            }))).GetProperty("report");
            int failed = r.GetProperty("failures").GetArrayLength();
            if (failed > 0) SwitchError = $"{failed} file(s) could not be moved. Switch again when every node is up.";
        }
        catch (Exception e)
        {
            SwitchError = e.Message;
        }
        finally
        {
            IsSwitching = false;
            OnUi(() => SwitchEnded?.Invoke());
        }
    }

    public async Task SetGiveAsync(long bytes) => await host.CallAsync("set_give", new { bytes });

    public async Task<JsonElement> RepairAsync(IProgress<(long Done, long Total)>? progress = null) =>
        (await host.CallAsync("repair", progress: progress)).GetProperty("report");

    public async Task RefreshAsync()
    {
        IsBusy = true;
        try
        {
            var r = await host.CallAsync("list");
            var fresh = r.GetProperty("items").EnumerateArray().Select(GalleryItem.FromJson).ToList();
            // Brought in line item by item, never cleared and refilled: a reset of the whole
            // collection under the justified rows (LinedFlowLayout) is the suspect in a crash that
            // ended the app when a phone's uploads arrived. Items already shown keep their object,
            // and with it their thumbnail and selection.
            var added = new List<GalleryItem>();
            for (int i = 0; i < fresh.Count; i++)
            {
                string id = fresh[i].FileId;
                int at = -1;
                for (int j = i; j < Items.Count; j++)
                    if (Items[j].FileId == id) { at = j; break; }
                if (at == i) continue;
                if (at > i) Items.Move(at, i);
                else { Items.Insert(i, fresh[i]); added.Add(fresh[i]); }
            }
            while (Items.Count > fresh.Count) Items.RemoveAt(Items.Count - 1);
            _ = LoadThumbnailsAsync(added.Where(i => i.HasThumb).Concat(Items.Where(i => i.HasThumb && i.Thumbnail is null)).Distinct().ToList());
            Error = null;
        }
        finally
        {
            IsBusy = false;
        }
    }

    private async Task LoadThumbnailsAsync(IEnumerable<GalleryItem> items)
    {
        await Task.WhenAll(items.Select(async item =>
        {
            await thumbGate.WaitAsync();
            try
            {
                var r = await host.CallAsync("thumb", new { file_id = item.FileId });
                string path = r.GetProperty("path").GetString()!;
                OnUi(() => item.Thumbnail = new BitmapImage(new Uri(path)) { DecodePixelHeight = 360 });
            }
            catch { /* the glyph stays */ }
            finally { thumbGate.Release(); }
        }));
    }

    // -------------------------------------------------------------------------------------
    // Transfers

    private void BeginTransfer(long bytes)
    {
        transfersActive++; transfersTotal++; bytesTotal += bytes;
        UpdateTransferText();
    }

    private void EndTransfer(long bytes, long counted)
    {
        transfersActive--; transfersDone++;
        bytesDone += bytes - counted;
        if (transfersActive == 0) { transfersDone = transfersTotal = 0; bytesDone = bytesTotal = 0; }
        UpdateTransferText();
    }

    private void UpdateTransferText()
    {
        TransferText = transfersActive == 0 ? "" :
            $"{transfersDone + 1} of {transfersTotal} · {GalleryItem.FormatBytes(bytesDone)} of {GalleryItem.FormatBytes(bytesTotal)}";
        Changed(nameof(IsTransferring));
        Changed(nameof(TransferPercent));
    }

    /// <summary>Reports a transfer's progress into the shared bar; returns how much it counted.</summary>
    private Progress<(long Done, long Total)> Track(Func<long> counted, Action<long> setCounted) => new(p =>
    {
        long delta = p.Done - counted();
        setCounted(p.Done);
        bytesDone += delta;
        UpdateTransferText();
    });

    public async Task UploadAsync(IReadOnlyList<StorageFile> files)
    {
        foreach (var file in files)
        {
            long size = (long)(await file.GetBasicPropertiesAsync()).Size;
            BeginTransfer(size);
        }
        // A few at a time: each upload already keeps several chunks in flight.
        using var gate = new SemaphoreSlim(2, 2);
        var failures = new List<string>();
        await Task.WhenAll(files.Select(async file =>
        {
            await gate.WaitAsync();
            long size = 0, counted = 0;
            try
            {
                size = (long)(await file.GetBasicPropertiesAsync()).Size;
                var info = await Thumbnailer.InspectAsync(file);
                var r = await host.CallAsync("upload", new
                {
                    path = file.Path,
                    name = file.Name,
                    mime = info.Mime,
                    created_ms = info.CreatedMs,
                    modified_ms = info.ModifiedMs,
                    width = info.Width,
                    height = info.Height,
                    thumb_path = info.ThumbPath,
                }, Track(() => counted, v => counted = v));
                if (info.ThumbPath is not null) TryDelete(info.ThumbPath);
                var item = GalleryItem.FromJson(r.GetProperty("item"));
                Insert(item);
                if (item.HasThumb) _ = LoadThumbnailsAsync([item]);
            }
            catch (Exception e)
            {
                lock (failures) failures.Add($"{file.Name}: {e.Message}");
            }
            finally
            {
                EndTransfer(size, counted);
                gate.Release();
            }
        }));
        Error = failures.Count == 0 ? null : $"{failures.Count} upload(s) failed. {failures[0]}";
    }

    private void Insert(GalleryItem item)
    {
        int at = 0;
        while (at < Items.Count && Items[at].CreatedMs > item.CreatedMs) at++;
        Items.Insert(at, item);
    }

    public async Task DownloadAsync(GalleryItem item, string outPath)
    {
        long counted = 0;
        BeginTransfer(item.Size);
        try
        {
            await host.CallAsync("download", new { file_id = item.FileId, out_path = outPath }, Track(() => counted, v => counted = v));
        }
        finally
        {
            EndTransfer(item.Size, counted);
        }
    }

    /// <summary>The full file, fetched into a local view cache once and reused after that.</summary>
    public async Task<string> OpenOriginalAsync(GalleryItem item)
    {
        Directory.CreateDirectory(LocalStore.ViewCache);
        string ext = Path.GetExtension(item.Name);
        string path = Path.Combine(LocalStore.ViewCache, item.FileId + ext);
        if (File.Exists(path)) return path;
        await DownloadAsync(item, path);
        return path;
    }

    public async Task DeleteAsync(IReadOnlyList<GalleryItem> items)
    {
        foreach (var item in items)
        {
            await host.CallAsync("delete", new { file_id = item.FileId });
            Items.Remove(item);
            foreach (var f in Directory.Exists(LocalStore.ViewCache) ? Directory.GetFiles(LocalStore.ViewCache, item.FileId + "*") : [])
                TryDelete(f);
        }
    }

    private static void TryDelete(string path)
    {
        try { File.Delete(path); } catch { }
    }

    // -------------------------------------------------------------------------------------

    private void OnUi(Action a)
    {
        if (ui is null || ui.HasThreadAccess) a();
        else ui.TryEnqueue(() => a());
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    private void Changed([CallerMemberName] string? name = null) =>
        OnUi(() => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name)));

    private void Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value)) return;
        field = value;
        Changed(name);
    }
}

public sealed record DeviceRow(string Name, string Platform, string Device, bool IsThis)
{
    public string Detail => $"{Platform} · {Device[..Math.Min(10, Device.Length)]}{(IsThis ? " · this PC" : "")}";
}

public sealed record FriendRow(string Name, string Account, int Nodes)
{
    public string Title => string.IsNullOrEmpty(Name) ? Account[..Math.Min(10, Account.Length)] : Name;
    public string Detail => $"{Account[..Math.Min(10, Account.Length)]} · {Nodes} node{(Nodes == 1 ? "" : "s")}";
}

public sealed record PoolRow(string Id, string Label, string Owner, bool Reachable, string? Path, string Role, long Used, long? Quota, string? Error)
{
    public static PoolRow FromJson(JsonElement n)
    {
        JsonElement? info = n.TryGetProperty("info", out var i) && i.ValueKind == JsonValueKind.Object ? i : null;
        long? quota = info is { } x && x.TryGetProperty("quota_bytes", out var q) && q.ValueKind == JsonValueKind.Number ? q.GetInt64() : null;
        return new PoolRow(
            n.GetProperty("id").GetString() ?? "",
            n.TryGetProperty("label", out var l) ? l.GetString() ?? "" : "",
            n.GetProperty("owner").GetString() ?? "",
            n.GetProperty("reachable").GetBoolean(),
            n.TryGetProperty("path", out var p) ? p.GetString() : null,
            info?.GetProperty("role").GetString() ?? "none",
            info?.GetProperty("used_bytes").GetInt64() ?? 0,
            quota,
            n.TryGetProperty("error", out var e) ? e.GetString() : null);
    }

    public string Title => !string.IsNullOrEmpty(Label) ? Label : Owner == "home" ? $"Your node {Id[..Math.Min(8, Id.Length)]}" : $"{Owner}'s node";

    public string Detail
    {
        get
        {
            if (!Reachable) return $"Unreachable · {Id[..Math.Min(10, Id.Length)]}";
            string path = Path == "direct" ? "Direct" : Path == "relay" ? "Relay" : "Connecting";
            string role = Role == "owner" ? "you own it" : Role == "granted" ? "shared with you" : "no access yet";
            string quota = Quota is { } qb ? $" of {GalleryItem.FormatBytes(qb)}" : "";
            return $"{path} · {role} · {GalleryItem.FormatBytes(Used)}{quota} used";
        }
    }

    public string Glyph => Owner == "home" ? "" : "";
}
