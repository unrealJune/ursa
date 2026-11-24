using System.Collections.Specialized;
using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Navigation;
using Ursa.App.Services;
using Windows.ApplicationModel.DataTransfer;
using Windows.Media.Core;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.System;

namespace Ursa.App.Pages;

public sealed partial class GalleryPage : Page
{
    public Library Library => App.Library;
    private GalleryItem? viewing;

    public GalleryPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        Library.Items.CollectionChanged += OnItemsChanged;
        Library.PropertyChanged += OnLibraryChanged;
        UpdateStates();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        Library.Items.CollectionChanged -= OnItemsChanged;
        Library.PropertyChanged -= OnLibraryChanged;
        CloseViewer();
    }

    /// <summary>Re-evaluates the empty and connection states (after onboarding).</summary>
    public void Refresh() => UpdateStates();

    public bool HasError(string? error) => !string.IsNullOrEmpty(error);

    private void OnErrorClosed(InfoBar sender, object args) => Library.Error = null;

    private void OnItemsChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        // The layout caches the aspect ratios it asked for by index; an insert shifts every
        // index after it, and stale ratios overlap tiles.
        Flow.InvalidateItemsInfo();
        UpdateStates();
    }

    private void OnLibraryChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Library.IsConnected) or nameof(Library.IsConfigured) or nameof(Library.IsReady) or nameof(Library.IsBusy))
            UpdateStates();
    }

    private void UpdateStates()
    {
        int n = Library.Items.Count;
        long bytes = Library.Items.Sum(i => i.Size);
        Summary.Text = n == 0 ? Library.DetailText : $"{n:N0} item{(n == 1 ? "" : "s")} · {GalleryItem.FormatBytes(bytes)}";
        bool empty = n == 0 && !Library.IsBusy;
        EmptyState.Visibility = empty ? Visibility.Visible : Visibility.Collapsed;
        if (!Library.IsConfigured)
        {
            EmptyTitle.Text = "Add a node you own";
            EmptyBody.Text = "Ursa keeps your library on nodes you and your friends run. Add yours in Settings.";
            EmptyAction.Content = "Open Settings";
        }
        else if (!Library.IsConnected)
        {
            EmptyTitle.Text = "Can't reach your nodes";
            EmptyBody.Text = string.IsNullOrEmpty(Library.DetailText) ? "It may be offline. Ursa will keep trying." : Library.DetailText;
            EmptyAction.Content = "Try again";
        }
        else
        {
            EmptyTitle.Text = "Your library is empty";
            EmptyBody.Text = "Drop photos, videos or any files here, or upload them. They're encrypted on this PC before they leave it.";
            EmptyAction.Content = "Upload files";
        }
    }

    private async void OnEmptyAction(object sender, RoutedEventArgs e)
    {
        if (!Library.IsConfigured) App.Window?.ShowSettings();
        else if (!Library.IsConnected) await Guard(async () => { await Library.RefreshStatusAsync(); if (Library.IsConnected) await Library.RefreshAsync(); });
        else await PickAndUploadAsync();
    }

    /// <summary>Runs a UI action and turns a failure into the page's error bar.</summary>
    private async Task Guard(Func<Task> action)
    {
        try { await action(); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    // ---------------------------------------------------------------------------------
    // Layout: each item asks for its own aspect ratio, so rows justify without measuring.

    private void OnItemsInfoRequested(LinedFlowLayout sender, LinedFlowLayoutItemsInfoRequestedEventArgs args)
    {
        int start = args.ItemsRangeStartIndex, count = args.ItemsRangeRequestedLength;
        var ratios = new double[count];
        for (int i = 0; i < count; i++)
            ratios[i] = start + i < Library.Items.Count ? Library.Items[start + i].AspectRatio : 1.5;
        args.SetDesiredAspectRatios(ratios);
    }

    private List<GalleryItem> Selected => Gallery.SelectedItems.OfType<GalleryItem>().ToList();

    private void OnSelectionChanged(ItemsView sender, ItemsViewSelectionChangedEventArgs args)
    {
        int n = Gallery.SelectedItems.Count;
        DownloadButton.IsEnabled = n > 0;
        DeleteButton.IsEnabled = n > 0;
        DownloadButton.Label = n > 1 ? $"Download {n}" : "Download";
        DeleteButton.Label = n > 1 ? $"Delete {n}" : "Delete";
    }

    // ---------------------------------------------------------------------------------
    // Upload

    private async void OnUpload(object sender, RoutedEventArgs e) => await PickAndUploadAsync();

    private async void OnUploadAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        if (Library.IsConnected) await PickAndUploadAsync();
    }

    private async Task PickAndUploadAsync()
    {
        var picker = new FileOpenPicker { ViewMode = PickerViewMode.Thumbnail, SuggestedStartLocation = PickerLocationId.PicturesLibrary };
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
        var files = await picker.PickMultipleFilesAsync();
        if (files is { Count: > 0 }) await Guard(() => Library.UploadAsync(files));
    }

    private void OnDragOver(object sender, DragEventArgs e)
    {
        if (!Library.IsConnected || !e.DataView.Contains(StandardDataFormats.StorageItems)) return;
        e.AcceptedOperation = DataPackageOperation.Copy;
        e.DragUIOverride.Caption = "Upload to Ursa";
        DropHint.Visibility = Visibility.Visible;
    }

    private async void OnDrop(object sender, DragEventArgs e)
    {
        DropHint.Visibility = Visibility.Collapsed;
        if (!Library.IsConnected || !e.DataView.Contains(StandardDataFormats.StorageItems)) return;
        var items = await e.DataView.GetStorageItemsAsync();
        var files = new List<StorageFile>();
        foreach (var item in items)
            await Collect(item, files);
        if (files.Count > 0) await Guard(() => Library.UploadAsync(files));
    }

    /// <summary>Files, and the files inside dropped folders.</summary>
    private static async Task Collect(IStorageItem item, List<StorageFile> into)
    {
        if (item is StorageFile f) into.Add(f);
        else if (item is StorageFolder folder)
            foreach (var child in await folder.GetItemsAsync())
                await Collect(child, into);
    }

    protected override void OnDragLeave(DragEventArgs e)
    {
        base.OnDragLeave(e);
        DropHint.Visibility = Visibility.Collapsed;
    }

    // ---------------------------------------------------------------------------------
    // Download, delete, refresh

    private async void OnDownload(object sender, RoutedEventArgs e) => await DownloadAsync(Selected);

    private async Task DownloadAsync(IReadOnlyList<GalleryItem> items)
    {
        if (items.Count == 0) return;
        await Guard(async () =>
        {
            if (items.Count == 1)
            {
                var item = items[0];
                var picker = new FileSavePicker { SuggestedFileName = Path.GetFileNameWithoutExtension(item.Name) };
                string ext = Path.GetExtension(item.Name);
                picker.FileTypeChoices.Add(string.IsNullOrEmpty(ext) ? "File" : ext.TrimStart('.').ToUpperInvariant(),
                    [string.IsNullOrEmpty(ext) ? "." : ext]);
                WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
                var target = await picker.PickSaveFileAsync();
                if (target is null) return;
                await Library.DownloadAsync(item, target.Path);
                return;
            }
            var folderPicker = new FolderPicker { SuggestedStartLocation = PickerLocationId.Downloads };
            folderPicker.FileTypeFilter.Add("*");
            WinRT.Interop.InitializeWithWindow.Initialize(folderPicker, App.Window!.Hwnd);
            var folder = await folderPicker.PickSingleFolderAsync();
            if (folder is null) return;
            foreach (var item in items)
                await Library.DownloadAsync(item, UniquePath(folder.Path, item.Name));
        });
    }

    private static string UniquePath(string dir, string name)
    {
        string path = Path.Combine(dir, name);
        for (int i = 2; File.Exists(path); i++)
            path = Path.Combine(dir, $"{Path.GetFileNameWithoutExtension(name)} ({i}){Path.GetExtension(name)}");
        return path;
    }

    private async void OnDelete(object sender, RoutedEventArgs e) => await DeleteAsync(Selected);

    private async void OnDeleteAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        await DeleteAsync(viewing is not null ? [viewing] : Selected);
    }

    private async Task DeleteAsync(IReadOnlyList<GalleryItem> items)
    {
        if (items.Count == 0) return;
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = items.Count == 1 ? $"Delete {items[0].Name}?" : $"Delete {items.Count} items?",
            Content = "They'll be removed from your library on every device.",
            PrimaryButtonText = "Delete",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        if (viewing is not null && items.Contains(viewing)) CloseViewer();
        await Guard(() => Library.DeleteAsync(items));
    }

    private async void OnRefresh(object sender, RoutedEventArgs e) => await Guard(Library.RefreshAsync);

    private async void OnRefreshAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        await Guard(async () => { await Library.RefreshStatusAsync(); if (Library.IsConnected) await Library.RefreshAsync(); });
    }

    // ---------------------------------------------------------------------------------
    // Viewer

    private async void OnItemInvoked(ItemsView sender, ItemsViewItemInvokedEventArgs args)
    {
        if (args.InvokedItem is GalleryItem item) await OpenViewerAsync(item);
    }

    private async Task OpenViewerAsync(GalleryItem item)
    {
        viewing = item;
        Viewer.Visibility = Visibility.Visible;
        ViewerTitle.Text = item.Name;
        ViewerDetails.Text = item.Details;
        int i = Library.Items.IndexOf(item);
        PrevButton.Visibility = i > 0 ? Visibility.Visible : Visibility.Collapsed;
        NextButton.Visibility = i >= 0 && i < Library.Items.Count - 1 ? Visibility.Visible : Visibility.Collapsed;
        StopVideo();
        ViewerFallback.Visibility = Visibility.Collapsed;
        ViewerVideo.Visibility = Visibility.Collapsed;
        // Show the thumbnail at once, then the original when it has arrived.
        ViewerImage.Source = item.Thumbnail;
        ViewerImage.Visibility = Visibility.Visible;
        Viewer.Focus(FocusState.Programmatic);

        if (!item.IsMedia)
        {
            ViewerImage.Visibility = Visibility.Collapsed;
            ViewerGlyph.Glyph = item.Glyph;
            ViewerFallbackText.Text = "No preview. Download it to open it.";
            ViewerFallback.Visibility = Visibility.Visible;
            return;
        }
        ViewerLoading.IsActive = true;
        try
        {
            string path = await Library.OpenOriginalAsync(item);
            if (viewing != item) return;
            if (item.IsVideo)
            {
                ViewerImage.Visibility = Visibility.Collapsed;
                ViewerVideo.Source = MediaSource.CreateFromUri(new Uri(path));
                ViewerVideo.Visibility = Visibility.Visible;
            }
            else
            {
                var bmp = new BitmapImage();
                bmp.ImageFailed += (_, _) =>
                {
                    if (viewing != item) return;
                    ViewerFallbackText.Text = "Windows can't display this format. Download it to open it.";
                    ViewerGlyph.Glyph = item.Glyph;
                    ViewerFallback.Visibility = item.Thumbnail is null ? Visibility.Visible : Visibility.Collapsed;
                };
                bmp.UriSource = new Uri(path);
                ViewerImage.Source = bmp;
            }
        }
        catch (Exception ex)
        {
            if (viewing == item) Library.Error = $"Couldn't open {item.Name}: {ex.Message}";
        }
        finally
        {
            if (viewing == item) ViewerLoading.IsActive = false;
        }
    }

    private void StopVideo()
    {
        if (ViewerVideo.MediaPlayer is { } player) player.Pause();
        ViewerVideo.Source = null;
    }

    private void CloseViewer()
    {
        if (viewing is null) return;
        var last = viewing;
        viewing = null;
        StopVideo();
        ViewerImage.Source = null;
        ViewerLoading.IsActive = false;
        Viewer.Visibility = Visibility.Collapsed;
        Gallery.StartBringItemIntoView(Library.Items.IndexOf(last), new BringIntoViewOptions());
    }

    private void OnCloseViewer(object sender, RoutedEventArgs e) => CloseViewer();

    private async void OnPrevious(object sender, RoutedEventArgs e) => await Step(-1);

    private async void OnNext(object sender, RoutedEventArgs e) => await Step(1);

    private async Task Step(int delta)
    {
        if (viewing is null) return;
        int i = Library.Items.IndexOf(viewing) + delta;
        if (i >= 0 && i < Library.Items.Count) await OpenViewerAsync(Library.Items[i]);
    }

    private async void OnViewerKeyDown(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Escape: e.Handled = true; CloseViewer(); break;
            case VirtualKey.Left: e.Handled = true; await Step(-1); break;
            case VirtualKey.Right: e.Handled = true; await Step(1); break;
        }
    }

    private async void OnViewerDownload(object sender, RoutedEventArgs e)
    {
        if (viewing is { } item) await DownloadAsync([item]);
    }

    private async void OnViewerDelete(object sender, RoutedEventArgs e)
    {
        if (viewing is { } item) await DeleteAsync([item]);
    }
}
