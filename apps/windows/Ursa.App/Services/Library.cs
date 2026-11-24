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

    private bool ready, connected, configured, busy;
    private string statusText = "Starting", detailText = "", deviceId = "", nodeId = "", path = "", backend = "";
    private string? error;

    public bool IsReady { get => ready; private set => Set(ref ready, value); }
    public bool IsConnected { get => connected; private set => Set(ref connected, value); }
    public bool IsConfigured { get => configured; private set => Set(ref configured, value); }
    public bool IsBusy { get => busy; private set => Set(ref busy, value); }
    public string StatusText { get => statusText; private set => Set(ref statusText, value); }
    public string DetailText { get => detailText; private set => Set(ref detailText, value); }
    public string DeviceId { get => deviceId; private set => Set(ref deviceId, value); }
    public string NodeId { get => nodeId; private set => Set(ref nodeId, value); }
    public string PathKind { get => path; private set => Set(ref path, value); }
    public string Backend { get => backend; private set => Set(ref backend, value); }
    public string? Error { get => error; set => Set(ref error, value); }

    // Transfers: one bar for everything in flight.
    private int transfersActive, transfersDone, transfersTotal;
    private long bytesDone, bytesTotal;
    private string transferText = "";
    public bool IsTransferring => transfersActive > 0;
    public double TransferPercent => bytesTotal > 0 ? 100.0 * bytesDone / bytesTotal : 0;
    public string TransferText { get => transferText; private set => Set(ref transferText, value); }

    // -------------------------------------------------------------------------------------

    public async Task StartAsync()
    {
        ui = DispatcherQueue.GetForCurrentThread();
        host.Exited += reason => OnUi(() => { IsReady = false; IsConnected = false; StatusText = "Stopped"; Error = reason; });
        try
        {
            host.Start();
            var settings = LocalStore.LoadSettings();
            var init = await host.CallAsync("init", new
            {
                data_dir = LocalStore.HostDataDir,
                account_key = LocalStore.AccountKeyHex(),
                relays = settings.Relays,
                relay_token = settings.RelayToken,
            });
            DeviceId = init.GetProperty("device_id").GetString() ?? "";
            IsReady = true;
            await RefreshStatusAsync();
            if (IsConnected) await RefreshAsync();
        }
        catch (Exception e)
        {
            StatusText = "Not running";
            Error = e.Message;
        }
    }

    /// <summary>Restarts the host, e.g. after the relays changed.</summary>
    public async Task RestartAsync()
    {
        IsReady = false;
        await host.StopAsync();
        await StartAsync();
    }

    public async Task RefreshStatusAsync()
    {
        var s = await host.CallAsync("status");
        IsConfigured = s.GetProperty("configured").GetBoolean();
        IsConnected = s.GetProperty("connected").GetBoolean();
        NodeId = s.TryGetProperty("node_id", out var n) ? n.GetString() ?? "" : "";
        PathKind = s.TryGetProperty("path", out var p) ? p.GetString() ?? "" : "";
        Backend = s.TryGetProperty("backend", out var b) ? b.GetString() ?? "" : "";
        if (!IsConfigured) { StatusText = "No node"; DetailText = "Connect your home node in Settings"; }
        else if (IsConnected) { StatusText = "Connected"; DetailText = $"{(PathKind == "direct" ? "Direct" : PathKind == "relay" ? "Through a relay" : "Connecting")} · {Backend} storage"; }
        else
        {
            StatusText = "Offline";
            DetailText = s.TryGetProperty("error", out var e) ? e.GetString() ?? "" : "";
        }
    }

    public async Task ConnectAsync(string link)
    {
        await host.CallAsync("connect", new { link = link.Trim() });
        await RefreshStatusAsync();
        await RefreshAsync();
    }

    public async Task ForgetAsync()
    {
        await host.CallAsync("forget");
        Items.Clear();
        await RefreshStatusAsync();
    }

    public async Task RefreshAsync()
    {
        IsBusy = true;
        try
        {
            var r = await host.CallAsync("list");
            var fresh = r.GetProperty("items").EnumerateArray().Select(GalleryItem.FromJson).ToList();
            var known = Items.ToDictionary(i => i.FileId);
            Items.Clear();
            foreach (var item in fresh)
            {
                // Keep a thumbnail already loaded.
                if (known.TryGetValue(item.FileId, out var old) && old.Thumbnail is not null) item.Thumbnail = old.Thumbnail;
                Items.Add(item);
            }
            _ = LoadThumbnailsAsync(fresh.Where(i => i.HasThumb && i.Thumbnail is null).ToList());
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
