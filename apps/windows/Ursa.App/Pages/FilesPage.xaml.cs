using System.Collections.ObjectModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Ursa.App.Services;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.System;

namespace Ursa.App.Pages;

/// <summary>One step of the path the breadcrumb shows; <see cref="Id"/> null is the top.</summary>
public sealed record Crumb(string? Id, string Name)
{
    // What the breadcrumb and the Move list show: WinUI's plain bindings can't read a C# record.
    public override string ToString() => Name;
}

/// <summary>A folder or a file in the folder being shown.</summary>
public sealed class FileRow
{
    // Internal: the XAML compiler would otherwise try to build these types itself.
    internal FolderEntry? Folder { get; init; }
    internal GalleryItem? File { get; init; }
    public bool IsFolder => Folder is not null;
    public string Name => Folder?.Name ?? File!.Name;
    public string Glyph => IsFolder ? "" : File!.IsVideo ? "" : File.IsImage ? "" : "";
    public Brush GlyphBrush => (Brush)Application.Current.Resources[IsFolder ? "AccentTextFillColorPrimaryBrush" : "TextFillColorSecondaryBrush"];
    public string DateText => File is { } f
        ? DateTimeOffset.FromUnixTimeMilliseconds(f.ModifiedMs > 0 ? f.ModifiedMs : f.CreatedMs).ToLocalTime().ToString("d MMM yyyy, HH:mm")
        : "";
    public string SizeText => File?.SizeText ?? "";
}

/// <summary>
/// The library as folders: browse, make folders, upload files and folders into the one shown,
/// download, rename, move and delete. The same tree every device shows.
/// </summary>
public sealed partial class FilesPage : Page
{
    public Library Library => App.Library;
    /// <summary>What the list shows; replaced whole on each change (see <see cref="Show"/>).</summary>
    private List<FileRow> Rows = [];
    private readonly ObservableCollection<Crumb> crumbs = [];

    /// <summary>The folder shown; null is the top. Kept across visits to the page.</summary>
    private static string? current;

    public FilesPage()
    {
        InitializeComponent();
        Crumbs.ItemsSource = crumbs;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        Library.TreeChanged += Show;
        Show();
        if (Library.IsConnected && Library.Items.Count == 0 && Library.Folders.Count == 0)
            _ = Guard(Library.RefreshAsync);
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => Library.TreeChanged -= Show;

    public bool HasError(string? error) => !string.IsNullOrEmpty(error);

    private void OnErrorClosed(InfoBar sender, object args) => Library.Error = null;

    private async Task Guard(Func<Task> action)
    {
        try { await action(); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    // ---------------------------------------------------------------------------------
    // Showing a folder

    private void Open(string? folderId)
    {
        current = folderId;
        Show();
    }

    private void Show()
    {
        // The folder may be gone (deleted here or on another device): show the nearest one left.
        if (current is not null && !Library.Folders.Any(f => f.Id == current)) current = null;

        var keep = new HashSet<string>(List.SelectedItems.OfType<FileRow>().Select(Key));
        var rows = new List<FileRow>();
        foreach (var f in Library.Folders.Where(f => f.Parent == current).OrderBy(f => f.Name, StringComparer.CurrentCultureIgnoreCase))
            rows.Add(new FileRow { Folder = f });
        foreach (var i in Library.Items.Where(i => i.Dir == current).OrderBy(i => i.Name, StringComparer.CurrentCultureIgnoreCase))
            rows.Add(new FileRow { File = i });
        // A fresh list, swapped in with nothing selected: changing the shown collection under a
        // selection whose item goes away (a folder just deleted) fails in WinUI ("Unspecified error").
        List.SelectedItems.Clear();
        Rows = rows;
        List.ItemsSource = Rows;
        foreach (var r in Rows.Where(r => keep.Contains(Key(r))))
            List.SelectedItems.Add(r);

        crumbs.Clear();
        crumbs.Add(new Crumb(null, "Library"));
        var chain = new List<FolderEntry>();
        for (var at = Library.Folders.FirstOrDefault(f => f.Id == current); at is not null; at = Library.Folders.FirstOrDefault(f => f.Id == at.Parent))
            chain.Insert(0, at);
        foreach (var f in chain) crumbs.Add(new Crumb(f.Id, f.Name));

        int folders = Rows.Count(r => r.IsFolder), files = Rows.Count - folders;
        long bytes = Rows.Sum(r => r.File?.Size ?? 0);
        Summary.Text = !Library.IsConnected ? Library.DetailText
            : $"{folders:N0} folder{(folders == 1 ? "" : "s")} · {files:N0} file{(files == 1 ? "" : "s")} · {GalleryItem.FormatBytes(bytes)}";
        EmptyState.Visibility = Rows.Count == 0 && !Library.IsBusy ? Visibility.Visible : Visibility.Collapsed;
        EmptyTitle.Text = Library.IsConnected ? (current is null ? "Your library is empty" : "This folder is empty") : "Can't reach your nodes";
        UpdateCommands();
    }

    private static string Key(FileRow r) => r.IsFolder ? "d:" + r.Folder!.Id : "f:" + r.File!.FileId;

    private void OnCrumbClicked(BreadcrumbBar sender, BreadcrumbBarItemClickedEventArgs args)
    {
        if (args.Item is Crumb c) Open(c.Id);
    }

    private void Up()
    {
        if (current is null) return;
        Open(Library.Folders.FirstOrDefault(f => f.Id == current)?.Parent);
    }

    private void OnUpAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        // Backspace in a dialog's text box is not for us.
        if (FocusManager.GetFocusedElement(XamlRoot) is TextBox) return;
        args.Handled = true;
        Up();
    }

    private async void OnDoubleTapped(object sender, DoubleTappedRoutedEventArgs e)
    {
        if ((e.OriginalSource as FrameworkElement)?.DataContext is FileRow row) await Activate(row);
    }

    private async void OnListKeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (e.Key == VirtualKey.Enter && Selected is [var row])
        {
            e.Handled = true;
            await Activate(row);
        }
    }

    /// <summary>A folder opens in place; a file opens in its app, fetched first.</summary>
    private async Task Activate(FileRow row)
    {
        if (row.Folder is { } f) { Open(f.Id); return; }
        await Guard(async () =>
        {
            string path = await Library.OpenOriginalAsync(row.File!);
            await Launcher.LaunchFileAsync(await StorageFile.GetFileFromPathAsync(path));
        });
    }

    private async void OnOpen(object sender, RoutedEventArgs e)
    {
        if (Selected is [var row]) await Activate(row);
    }

    private List<FileRow> Selected => List.SelectedItems.OfType<FileRow>().ToList();

    private void OnSelectionChanged(object sender, SelectionChangedEventArgs e) => UpdateCommands();

    private void UpdateCommands()
    {
        int n = List.SelectedItems.Count;
        bool on = Library.IsConnected;
        DownloadButton.IsEnabled = on && n > 0;
        DeleteButton.IsEnabled = on && n > 0;
        MoveButton.IsEnabled = on && n > 0;
        RenameButton.IsEnabled = on && n == 1;
        OpenButton.IsEnabled = on && n == 1;
        DownloadButton.Label = n > 1 ? $"Download {n}" : "Download";
        DeleteButton.Label = n > 1 ? $"Delete {n}" : "Delete";
    }

    private void OnRowContextRequested(UIElement sender, ContextRequestedEventArgs args)
    {
        if ((sender as FrameworkElement)?.DataContext is not FileRow row) return;
        if (!List.SelectedItems.Contains(row))
        {
            List.SelectedItems.Clear();
            List.SelectedItems.Add(row);
        }
        var menu = new MenuFlyout();
        void Add(string text, string glyph, Func<Task> run, bool enabled = true)
        {
            var item = new MenuFlyoutItem { Text = text, Icon = new FontIcon { Glyph = glyph }, IsEnabled = enabled && Library.IsConnected };
            item.Click += async (_, _) => await run();
            menu.Items.Add(item);
        }
        if (Selected.Count == 1) Add("Open", "", () => Activate(row));
        Add("Download", "", () => DownloadAsync(Selected));
        Add("Rename", "", () => RenameAsync(row), Selected.Count == 1);
        Add("Move to…", "", () => MoveAsync(Selected));
        menu.Items.Add(new MenuFlyoutSeparator());
        Add("Delete", "", () => DeleteAsync(Selected));
        if (args.TryGetPosition(sender, out var at)) menu.ShowAt(sender, at);
        else menu.ShowAt((FrameworkElement)sender);
        args.Handled = true;
    }

    // ---------------------------------------------------------------------------------
    // New folder, rename, move, delete

    private async void OnNewFolder(object sender, RoutedEventArgs e) => await NewFolderAsync();

    private async void OnNewFolderAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        if (Library.IsConnected) await NewFolderAsync();
    }

    private async Task NewFolderAsync()
    {
        string? name = await AskName("New folder", Library.FreeName(current, "New folder", isFolder: true), "Create");
        if (name is null) return;
        await Guard(() => Library.CreateFolderAsync(current, name));
    }

    private async void OnRename(object sender, RoutedEventArgs e)
    {
        if (Selected is [var row]) await RenameAsync(row);
    }

    private async void OnRenameAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        if (Library.IsConnected && Selected is [var row]) await RenameAsync(row);
    }

    private async Task RenameAsync(FileRow row)
    {
        string? name = await AskName("Rename", row.Name, "Rename", selectStem: !row.IsFolder);
        if (name is null || name == row.Name) return;
        await Guard(() => Library.EditAsync([row.IsFolder
            ? new { op = "rename_folder", id = row.Folder!.Id, name }
            : new { op = "rename_file", file_id = row.File!.FileId, name }]));
    }

    /// <summary>Asks for a name; null if cancelled.</summary>
    private async Task<string?> AskName(string title, string initial, string action, bool selectStem = false)
    {
        var box = new TextBox { Text = initial };
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(box, title);
        box.Loaded += (_, _) =>
        {
            box.Focus(FocusState.Programmatic);
            int stem = selectStem ? Path.GetFileNameWithoutExtension(initial).Length : initial.Length;
            box.Select(0, stem > 0 ? stem : initial.Length);
        };
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = title,
            Content = box,
            PrimaryButtonText = action,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        box.TextChanged += (_, _) => dialog.IsPrimaryButtonEnabled = box.Text.Trim().Length > 0;
        return await dialog.ShowAsync() == ContentDialogResult.Primary ? box.Text.Trim() : null;
    }

    private async void OnMove(object sender, RoutedEventArgs e) => await MoveAsync(Selected);

    private async Task MoveAsync(IReadOnlyList<FileRow> rows)
    {
        if (rows.Count == 0) return;
        // A folder can't go inside itself: leave its own subtree out of the choices.
        var blocked = new HashSet<string>();
        foreach (var r in rows.Where(r => r.IsFolder)) blocked.UnionWith(Library.Subtree(r.Folder!.Id));
        var targets = new List<Crumb> { new(null, "Library") };
        targets.AddRange(Library.Folders.Where(f => !blocked.Contains(f.Id))
            .OrderBy(f => f.Path, StringComparer.CurrentCultureIgnoreCase)
            .Select(f => new Crumb(f.Id, "Library / " + f.Path.Replace("/", " / "))));
        var list = new ListView
        {
            ItemsSource = targets,
            SelectionMode = ListViewSelectionMode.Single,
            MaxHeight = 360,
            SelectedIndex = Math.Max(0, targets.FindIndex(t => t.Id == current)),
        };
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(list, "Folders");
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = rows.Count == 1 ? $"Move {rows[0].Name} to" : $"Move {rows.Count} items to",
            Content = list,
            PrimaryButtonText = "Move here",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary || list.SelectedItem is not Crumb to) return;
        var edits = rows.Select(r => r.IsFolder
            ? (object)new { op = "move_folder", id = r.Folder!.Id, parent = to.Id }
            : new { op = "move_file", file_id = r.File!.FileId, dir = to.Id }).ToList();
        await Guard(() => Library.EditAsync(edits));
    }

    private async void OnDelete(object sender, RoutedEventArgs e) => await DeleteAsync(Selected);

    private async void OnDeleteAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        if (FocusManager.GetFocusedElement(XamlRoot) is TextBox) return;
        args.Handled = true;
        if (Library.IsConnected) await DeleteAsync(Selected);
    }

    private async Task DeleteAsync(IReadOnlyList<FileRow> rows)
    {
        if (rows.Count == 0) return;
        int inside = rows.Where(r => r.IsFolder).Sum(r =>
        {
            var sub = Library.Subtree(r.Folder!.Id);
            return Library.Items.Count(i => i.Dir is { } d && sub.Contains(d));
        });
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = rows.Count == 1 ? $"Delete {rows[0].Name}?" : $"Delete {rows.Count} items?",
            Content = inside > 0
                ? $"This also deletes the {inside:N0} file{(inside == 1 ? "" : "s")} inside, from your library on every device."
                : "They'll be removed from your library on every device.",
            PrimaryButtonText = "Delete",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        var edits = rows.Select(r => r.IsFolder
            ? (object)new { op = "delete_folder", id = r.Folder!.Id }
            : new { op = "delete_file", file_id = r.File!.FileId }).ToList();
        await Guard(() => Library.EditAsync(edits));
    }

    // ---------------------------------------------------------------------------------
    // Upload and download

    private async void OnUploadFiles(object sender, RoutedEventArgs e) => await PickFilesAsync();

    private async void OnUploadAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        if (Library.IsConnected) await PickFilesAsync();
    }

    private async Task PickFilesAsync()
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
        var files = await picker.PickMultipleFilesAsync();
        string? into = current;
        if (files is { Count: > 0 }) await Guard(() => Library.UploadIntoAsync([.. files], into));
    }

    private async void OnUploadFolder(object sender, RoutedEventArgs e)
    {
        var picker = new FolderPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
        var folder = await picker.PickSingleFolderAsync();
        string? into = current;
        if (folder is not null) await Guard(() => Library.UploadIntoAsync([folder], into));
    }

    private void OnDragOver(object sender, DragEventArgs e)
    {
        if (!Library.IsConnected || !e.DataView.Contains(StandardDataFormats.StorageItems)) return;
        e.AcceptedOperation = DataPackageOperation.Copy;
        string where = crumbs.LastOrDefault()?.Name ?? "Library";
        e.DragUIOverride.Caption = $"Upload to {where}";
        DropText.Text = $"Drop to upload to {where}";
        DropHint.Visibility = Visibility.Visible;
    }

    protected override void OnDragLeave(DragEventArgs e)
    {
        base.OnDragLeave(e);
        DropHint.Visibility = Visibility.Collapsed;
    }

    private async void OnDrop(object sender, DragEventArgs e)
    {
        DropHint.Visibility = Visibility.Collapsed;
        if (!Library.IsConnected || !e.DataView.Contains(StandardDataFormats.StorageItems)) return;
        var items = await e.DataView.GetStorageItemsAsync();
        string? into = current;
        if (items.Count > 0) await Guard(() => Library.UploadIntoAsync([.. items], into));
    }

    private async void OnDownload(object sender, RoutedEventArgs e) => await DownloadAsync(Selected);

    private async Task DownloadAsync(IReadOnlyList<FileRow> rows)
    {
        if (rows.Count == 0) return;
        await Guard(async () =>
        {
            if (rows is [{ File: { } item }])
            {
                var picker = new FileSavePicker { SuggestedFileName = Path.GetFileNameWithoutExtension(item.Name) };
                string ext = Path.GetExtension(item.Name);
                picker.FileTypeChoices.Add(string.IsNullOrEmpty(ext) ? "File" : ext.TrimStart('.').ToUpperInvariant(),
                    [string.IsNullOrEmpty(ext) ? "." : ext]);
                WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
                var target = await picker.PickSaveFileAsync();
                if (target is not null) await Library.DownloadAsync(item, target.Path);
                return;
            }
            var folderPicker = new FolderPicker { SuggestedStartLocation = PickerLocationId.Downloads };
            folderPicker.FileTypeFilter.Add("*");
            WinRT.Interop.InitializeWithWindow.Initialize(folderPicker, App.Window!.Hwnd);
            var folder = await folderPicker.PickSingleFolderAsync();
            if (folder is null) return;
            foreach (var row in rows)
            {
                if (row.Folder is { } f) await Library.DownloadFolderAsync(f.Id, folder.Path);
                else await Library.DownloadAsync(row.File!, UniquePath(folder.Path, row.File!.Name));
            }
        });
    }

    private static string UniquePath(string dir, string name)
    {
        string path = Path.Combine(dir, name);
        for (int i = 2; File.Exists(path); i++)
            path = Path.Combine(dir, $"{Path.GetFileNameWithoutExtension(name)} ({i}){Path.GetExtension(name)}");
        return path;
    }

    /// <summary>Re-reads the account from its nodes first, for what other devices changed.</summary>
    private async void OnRefresh(object sender, RoutedEventArgs e) =>
        await Guard(async () => { await Library.RefreshStatusAsync(); if (Library.IsConnected) await Library.RefreshAsync(); });

    private async void OnRefreshAccelerator(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs args)
    {
        args.Handled = true;
        await Guard(async () => { await Library.RefreshStatusAsync(); if (Library.IsConnected) await Library.RefreshAsync(); });
    }
}
