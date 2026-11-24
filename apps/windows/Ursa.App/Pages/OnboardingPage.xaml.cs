using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using Ursa.App.Services;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage;
using Windows.Storage.Pickers;

namespace Ursa.App.Pages;

/// <summary>
/// First run: create an account (and save its recovery key), or link to / recover an existing
/// one; then add a node the account owns, which every account needs before its library opens.
/// </summary>
public sealed partial class OnboardingPage : Page
{
    public Library Library => App.Library;
    private string phrase = "";
    private string? keyFileJson;

    public OnboardingPage()
    {
        InitializeComponent();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (Library.NeedsNode) Show(StepNode);
        else Show(StepWelcome);
        if (!string.IsNullOrEmpty(Library.OpenError))
            Fail($"Couldn't reach your account's nodes: {Library.OpenError}. Recover or link again, or check your network.");
    }

    private void Show(StackPanel step)
    {
        foreach (var s in new[] { StepWelcome, StepCreate, StepRecovery, StepExisting, StepNode })
            s.Visibility = s == step ? Visibility.Visible : Visibility.Collapsed;
        Problem.IsOpen = false;
    }

    private void Fail(string message)
    {
        Problem.Message = message;
        Problem.IsOpen = true;
    }

    private void OnStartCreate(object sender, RoutedEventArgs e) => Show(StepCreate);

    private void OnStartExisting(object sender, RoutedEventArgs e) => Show(StepExisting);

    private void OnBack(object sender, RoutedEventArgs e) => Show(StepWelcome);

    // ---------------------------------------------------------------------------------
    // Create

    private async void OnCreate(object sender, RoutedEventArgs e)
    {
        string name = NameBox.Text.Trim();
        if (name.Length == 0) { Fail("Enter a name."); return; }
        CreateButton.IsEnabled = false;
        try
        {
            await Library.CreateAccountAsync(name);
            phrase = await Library.RecoveryPhraseAsync();
            var words = phrase.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            Words.ItemsSource = words.Select((w, i) => $"{i + 1,2}. {w}").ToList();
            Show(StepRecovery);
        }
        catch (Exception ex) { Fail(ex.Message); }
        finally { CreateButton.IsEnabled = true; }
    }

    private void OnCopyWords(object sender, RoutedEventArgs e)
    {
        var p = new DataPackage();
        p.SetText(phrase);
        Clipboard.SetContent(p);
    }

    private async void OnSaveKeyFile(object sender, RoutedEventArgs e) => await SaveKeyFileAsync(this, Library);

    /// <summary>Asks for a passphrase twice, then where to save the key file. Shared with Settings.</summary>
    public static async Task<bool> SaveKeyFileAsync(Page page, Library library)
    {
        var pass1 = new PasswordBox { Header = "Passphrase (8 characters or more)" };
        var pass2 = new PasswordBox { Header = "Again" };
        var dialog = new ContentDialog
        {
            XamlRoot = page.XamlRoot,
            Title = "Save a key file",
            Content = new StackPanel
            {
                Spacing = 12,
                Children =
                {
                    new TextBlock { Text = "The key file is your account key sealed with this passphrase. Keep the file and the passphrase apart.", TextWrapping = TextWrapping.Wrap },
                    pass1, pass2,
                },
            },
            PrimaryButtonText = "Save…",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Primary,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return false;
        if (pass1.Password.Length < 8 || pass1.Password != pass2.Password)
        {
            await new ContentDialog { XamlRoot = page.XamlRoot, Title = "Passphrases don't match or are too short", CloseButtonText = "OK" }.ShowAsync();
            return false;
        }
        string json = await library.KeyFileAsync(pass1.Password);
        var picker = new FileSavePicker { SuggestedFileName = "Ursa recovery key", SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
        picker.FileTypeChoices.Add("Ursa key file", [".ursa-key"]);
        WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
        var file = await picker.PickSaveFileAsync();
        if (file is null) return false;
        await FileIO.WriteTextAsync(file, json);
        return true;
    }

    private void OnSavedChanged(object sender, RoutedEventArgs e) => RecoveryContinue.IsEnabled = SavedCheck.IsChecked == true;

    private void OnRecoveryContinue(object sender, RoutedEventArgs e)
    {
        phrase = "";
        Words.ItemsSource = null;
        Show(StepNode);
    }

    // ---------------------------------------------------------------------------------
    // Existing account

    private async void OnJoin(object sender, RoutedEventArgs e)
    {
        string code = LinkCodeBox.Text.Trim();
        if (!code.StartsWith("ursa-link:")) { Fail("A link code starts with ursa-link:. Get one from Settings › Devices › Add a device on the other device."); return; }
        JoinButton.IsEnabled = false;
        try
        {
            await Library.JoinAsync(code, c =>
            {
                ConfirmCode.Text = $"{c[..3]} {c[3..]}";
                ConfirmPanel.Visibility = Visibility.Visible;
            });
            Done();
        }
        catch (Exception ex) { Fail($"Couldn't link: {ex.Message}"); }
        finally
        {
            JoinButton.IsEnabled = true;
            ConfirmPanel.Visibility = Visibility.Collapsed;
        }
    }

    private async void OnPickKeyFile(object sender, RoutedEventArgs e)
    {
        var picker = new FileOpenPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
        picker.FileTypeFilter.Add(".ursa-key");
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, App.Window!.Hwnd);
        var file = await picker.PickSingleFileAsync();
        if (file is null) return;
        keyFileJson = await FileIO.ReadTextAsync(file);
        KeyFileName.Text = file.Name;
    }

    private async void OnRecover(object sender, RoutedEventArgs e)
    {
        RecoverButton.IsEnabled = false;
        try
        {
            if (keyFileJson is not null) await Library.RecoverFromKeyFileAsync(keyFileJson, KeyFilePass.Password);
            else if (PhraseBox.Text.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries).Length == 24) await Library.RecoverFromPhraseAsync(PhraseBox.Text);
            else { Fail("Choose your key file and enter its passphrase, or type all 24 recovery words."); return; }
            Done();
        }
        catch (Exception ex) { Fail($"Couldn't recover: {ex.Message}"); }
        finally { RecoverButton.IsEnabled = true; }
    }

    // ---------------------------------------------------------------------------------
    // A node

    private async void OnClaim(object sender, RoutedEventArgs e)
    {
        string link = ClaimBox.Text.Trim();
        if (!link.StartsWith("ursa-claim:")) { Fail("A claim link starts with ursa-claim:. The node prints it when it first starts."); return; }
        ClaimButton.IsEnabled = false;
        ClaimProgress.IsActive = true;
        try
        {
            await Library.ClaimNodeAsync(link, NodeLabelBox.Text.Trim());
            Done();
        }
        catch (Exception ex) { Fail($"Couldn't add the node: {ex.Message}"); }
        finally
        {
            ClaimButton.IsEnabled = true;
            ClaimProgress.IsActive = false;
        }
    }

    /// <summary>The window decides what is next: the library, or the node step if still needed.</summary>
    private void Done()
    {
        if (Library.NeedsNode) Show(StepNode);
        else App.Window?.ShowLibraryIfReady();
    }
}
