using System.Diagnostics;
using System.Reflection;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
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
        About.Text = $"Ursa {version.Split('+')[0]} for Windows. Private, encrypted photo and file storage on your own and your friends' nodes.";
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        try { await Library.RefreshStatusAsync(); } catch (Exception ex) { Library.Error = ex.Message; }
    }

    protected override async void OnNavigatedFrom(NavigationEventArgs e) => await Library.CancelLinkAsync();

    public bool HasError(string? error) => !string.IsNullOrEmpty(error);

    public string Short(string id) => string.IsNullOrEmpty(id) ? "—" : id.Length > 20 ? $"{id[..10]}…{id[^10..]}" : id;

    private void OnErrorClosed(InfoBar sender, object args) => Library.Error = null;

    private static void Copy(string text)
    {
        var p = new DataPackage();
        p.SetText(text);
        Clipboard.SetContent(p);
    }

    // ---------------------------------------------------------------------------------
    // Devices

    private async void OnAddDevice(object sender, RoutedEventArgs e)
    {
        AddDeviceButton.IsEnabled = false;
        CancelLinkButton.Visibility = Visibility.Visible;
        LinkStatus.Text = "Waiting for the other device…";
        try
        {
            string? linked = await Library.OfferLinkAsync(
                code =>
                {
                    LinkCode.Text = code;
                    LinkPanel.Visibility = Visibility.Visible;
                },
                async (name, platform, code) =>
                {
                    var dialog = new ContentDialog
                    {
                        XamlRoot = XamlRoot,
                        Title = $"Add {name}?",
                        Content = new StackPanel
                        {
                            Spacing = 12,
                            Children =
                            {
                                new TextBlock { Text = $"A {platform} device called {name} wants to join your account. Only let it in if it shows this code:", TextWrapping = TextWrapping.Wrap },
                                new TextBlock { Text = $"{code[..3]} {code[3..]}", FontSize = 32, FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas"), HorizontalAlignment = HorizontalAlignment.Center },
                            },
                        },
                        PrimaryButtonText = "It matches: add it",
                        CloseButtonText = "Don't add",
                        DefaultButton = ContentDialogButton.Close,
                    };
                    bool ok = await dialog.ShowAsync() == ContentDialogResult.Primary;
                    LinkStatus.Text = ok ? "Sending your account key…" : "Refused. Waiting for another attempt…";
                    return ok;
                });
            if (linked is not null) Library.Error = null;
        }
        catch (Exception ex) { Library.Error = $"Couldn't add the device: {ex.Message}"; }
        finally
        {
            LinkPanel.Visibility = Visibility.Collapsed;
            CancelLinkButton.Visibility = Visibility.Collapsed;
            AddDeviceButton.IsEnabled = true;
            LinkCode.Text = "";
        }
    }

    private async void OnCancelLink(object sender, RoutedEventArgs e) => await Library.CancelLinkAsync();

    private void OnCopyLink(object sender, RoutedEventArgs e) => Copy(LinkCode.Text);

    // ---------------------------------------------------------------------------------
    // Recovery

    private async void OnShowWords(object sender, RoutedEventArgs e)
    {
        string phrase;
        try { phrase = await Library.RecoveryPhraseAsync(); }
        catch (Exception ex) { Library.Error = ex.Message; return; }
        var words = phrase.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        var grid = new Grid { ColumnSpacing = 16, RowSpacing = 6 };
        for (int c = 0; c < 3; c++) grid.ColumnDefinitions.Add(new ColumnDefinition());
        for (int r = 0; r < 8; r++) grid.RowDefinitions.Add(new RowDefinition());
        for (int i = 0; i < words.Length; i++)
        {
            var t = new TextBlock { Text = $"{i + 1,2}. {words[i]}", FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Consolas"), IsTextSelectionEnabled = true };
            Grid.SetColumn(t, i / 8);
            Grid.SetRow(t, i % 8);
            grid.Children.Add(t);
        }
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = "Recovery words",
            Content = new StackPanel
            {
                Spacing = 12,
                Children =
                {
                    new TextBlock { Text = "Anyone with these words can read your whole library. Write them down and keep them offline.", TextWrapping = TextWrapping.Wrap },
                    grid,
                },
            },
            PrimaryButtonText = "Copy",
            CloseButtonText = "Close",
        };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary) Copy(phrase);
    }

    private async void OnSaveKeyFile(object sender, RoutedEventArgs e)
    {
        try { await OnboardingPage.SaveKeyFileAsync(this, Library); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    // ---------------------------------------------------------------------------------
    // Nodes

    private async void OnCheck(object sender, RoutedEventArgs e)
    {
        try { await Library.RefreshStatusAsync(); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    private async void OnRepair(object sender, RoutedEventArgs e)
    {
        RepairButton.IsEnabled = false;
        string label = (string)RepairButton.Content;
        try
        {
            var report = await Library.RepairAsync(new Progress<(long Done, long Total)>(p => RepairButton.Content = $"Repairing {p.Done}/{p.Total}…"));
            int rebuilt = report.GetProperty("objects_rebuilt").GetInt32();
            int lost = report.GetProperty("unrecoverable").GetArrayLength();
            await new ContentDialog
            {
                XamlRoot = XamlRoot,
                Title = lost == 0 ? "Library healthy" : "Some data could not be recovered",
                Content = $"{report.GetProperty("files").GetInt32()} files checked, {rebuilt} objects rebuilt onto live nodes{(lost > 0 ? $", {lost} lost" : "")}.",
                CloseButtonText = "OK",
            }.ShowAsync();
            await Library.RefreshPoolAsync();
        }
        catch (Exception ex) { Library.Error = $"Repair failed: {ex.Message}"; }
        finally
        {
            RepairButton.Content = label;
            RepairButton.IsEnabled = true;
        }
    }

    private async void OnClaim(object sender, RoutedEventArgs e)
    {
        string link = ClaimLink.Text.Trim();
        if (!link.StartsWith("ursa-claim:"))
        {
            Library.Error = "That isn't a claim link. It starts with ursa-claim: and the node prints it when it first starts.";
            return;
        }
        ClaimButton.IsEnabled = false;
        try
        {
            await Library.ClaimNodeAsync(link, NodeLabel.Text.Trim());
            ClaimLink.Text = "";
            NodeLabel.Text = "";
            Library.Error = null;
        }
        catch (Exception ex) { Library.Error = $"Couldn't add the node: {ex.Message}"; }
        finally { ClaimButton.IsEnabled = true; }
    }

    // ---------------------------------------------------------------------------------
    // Friends

    private async void OnCopyInvite(object sender, RoutedEventArgs e)
    {
        try { Copy(await Library.InviteAsync()); }
        catch (Exception ex) { Library.Error = ex.Message; }
    }

    private async void OnBefriend(object sender, RoutedEventArgs e)
    {
        string invite = InviteBox.Text.Trim();
        if (!invite.StartsWith("ursa-invite:")) { Library.Error = "An invite starts with ursa-invite:. Ask your friend to copy theirs from Settings › Friends."; return; }
        try
        {
            await Library.BefriendAsync(invite, double.IsNaN(QuotaBox.Value) ? 50 : (long)QuotaBox.Value);
            InviteBox.Text = "";
            Library.Error = null;
        }
        catch (Exception ex) { Library.Error = $"Couldn't add the friend: {ex.Message}"; }
    }

    // ---------------------------------------------------------------------------------
    // Relays and this PC

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
}
