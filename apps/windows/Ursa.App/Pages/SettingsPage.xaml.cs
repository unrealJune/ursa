using System.Diagnostics;
using System.Reflection;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Ursa.App.Services;
using Windows.ApplicationModel.DataTransfer;

namespace Ursa.App.Pages;

public sealed partial class SettingsPage : Page
{
    public Library Library => App.Library;

    public SettingsPage()
    {
        InitializeComponent();
        var settings = LocalStore.LoadSettings();
        RelayUrls.Text = string.Join(Environment.NewLine, settings.Relays);
        RelayToken.PlaceholderText = settings.RelayTokenProtected is null ? "None" : "Saved (leave empty to keep it)";
        string version = Assembly.GetExecutingAssembly().GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion ?? "dev";
        About.Text = $"Ursa {version.Split('+')[0]} for Windows. Private, encrypted photo and file storage on your own nodes.";
    }

    public bool HasError(string? error) => !string.IsNullOrEmpty(error);

    public string Short(string id) => string.IsNullOrEmpty(id) ? "—" : id.Length > 20 ? $"{id[..10]}…{id[^10..]}" : id;

    private void OnErrorClosed(InfoBar sender, object args) => Library.Error = null;

    private async void OnConnect(object sender, RoutedEventArgs e)
    {
        string link = ClaimLink.Text.Trim();
        if (!link.StartsWith("ursa-claim:"))
        {
            Library.Error = "That isn't a claim link. It starts with ursa-claim: and the node prints it when it first starts.";
            return;
        }
        ConnectButton.IsEnabled = false;
        ConnectProgress.Visibility = Visibility.Visible;
        try
        {
            await Library.ConnectAsync(link);
            ClaimLink.Text = "";
            Library.Error = null;
        }
        catch (Exception ex)
        {
            Library.Error = $"Couldn't connect: {ex.Message}";
        }
        finally
        {
            ConnectButton.IsEnabled = true;
            ConnectProgress.Visibility = Visibility.Collapsed;
        }
    }

    private async void OnCheck(object sender, RoutedEventArgs e)
    {
        try { await Library.RefreshStatusAsync(); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    private async void OnForget(object sender, RoutedEventArgs e)
    {
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = "Forget this node?",
            Content = "This PC stops using it. Nothing on the node is deleted, and you can connect again with a claim link.",
            PrimaryButtonText = "Forget",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        try { await Library.ForgetAsync(); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    private async void OnSaveRelays(object sender, RoutedEventArgs e)
    {
        var settings = LocalStore.LoadSettings();
        settings.Relays = RelayUrls.Text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).ToList();
        foreach (var url in settings.Relays)
        {
            if (!Uri.TryCreate(url, UriKind.Absolute, out var u) || u.Scheme != "https")
            {
                Library.Error = $"{url} isn't an https URL.";
                return;
            }
        }
        if (!string.IsNullOrEmpty(RelayToken.Password)) settings.RelayToken = RelayToken.Password;
        LocalStore.SaveSettings(settings);
        RelayToken.Password = "";
        RelayToken.PlaceholderText = settings.RelayTokenProtected is null ? "None" : "Saved (leave empty to keep it)";
        await Library.RestartAsync();
    }

    private void OnOpenData(object sender, RoutedEventArgs e)
    {
        Directory.CreateDirectory(LocalStore.Root);
        Process.Start(new ProcessStartInfo("explorer.exe", $"\"{LocalStore.Root}\"") { UseShellExecute = true });
    }

    private async void OnShowKey(object sender, RoutedEventArgs e)
    {
        string key = LocalStore.AccountKeyHex();
        string grouped = string.Join(" ", Enumerable.Range(0, key.Length / 4).Select(i => key.Substring(i * 4, 4)));
        var text = new TextBlock { Text = grouped, FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas"), TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = "Recovery key",
            Content = new StackPanel
            {
                Spacing = 12,
                Children =
                {
                    new TextBlock { Text = "Anyone with this key can read your whole library. Store it offline, like a password manager or on paper.", TextWrapping = TextWrapping.Wrap },
                    text,
                },
            },
            PrimaryButtonText = "Copy",
            CloseButtonText = "Close",
        };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary)
        {
            var package = new DataPackage();
            package.SetText(key);
            Clipboard.SetContent(package);
        }
    }
}
