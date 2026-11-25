using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Ursa.App.Pages;
using Ursa.App.Services;

namespace Ursa.App;

public sealed partial class MainWindow : Window
{
    public Library Library => App.Library;

    public MainWindow()
    {
        InitializeComponent();
        // Send what is buffered before the process goes.
        Closed += (_, _) => Services.Telemetry.Flush();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;
        string icon = Path.Combine(AppContext.BaseDirectory, "Assets", "ursa.ico");
        if (File.Exists(icon)) AppWindow.SetIcon(icon);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1280, 840));
        Nav.SelectedItem = PhotosItem;
        App.Library.AccountChanged += () => DispatcherQueue.TryEnqueue(ShowLibraryIfReady);
    }

    /// <summary>Onboarding until the account exists and owns a node; the library after.</summary>
    public void ShowLibraryIfReady()
    {
        bool onboarding = App.Library.IsReady && (!App.Library.HasAccount || App.Library.NeedsNode);
        if (onboarding)
        {
            Nav.Visibility = Visibility.Collapsed;
            OnboardingFrame.Visibility = Visibility.Visible;
            OnboardingFrame.Navigate(typeof(OnboardingPage), null, new EntranceNavigationTransitionInfo());
            AppTitleBar.IsPaneToggleButtonVisible = false;
        }
        else
        {
            OnboardingFrame.Visibility = Visibility.Collapsed;
            OnboardingFrame.Content = null;
            Nav.Visibility = Visibility.Visible;
            AppTitleBar.IsPaneToggleButtonVisible = true;
            if (ContentFrame.Content is GalleryPage g) g.Refresh();
        }
    }

    public IntPtr Hwnd => WinRT.Interop.WindowNative.GetWindowHandle(this);

    public Brush StatusBrush(bool connected) =>
        new SolidColorBrush(connected ? ColorHelper.FromArgb(255, 0x3F, 0xB9, 0x50) : ColorHelper.FromArgb(255, 0x9E, 0x9E, 0x9E));

    private void OnPaneToggleRequested(TitleBar sender, object args) => Nav.IsPaneOpen = !Nav.IsPaneOpen;

    private void OnNavigationChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        Type page = args.IsSettingsSelected ? typeof(SettingsPage) : (args.SelectedItem as NavigationViewItem)?.Tag switch
        {
            "pool" => typeof(PoolPage),
            "usage" => typeof(UsagePage),
            _ => typeof(GalleryPage),
        };
        if (ContentFrame.CurrentSourcePageType != page)
            ContentFrame.Navigate(page, null, new EntranceNavigationTransitionInfo());
    }

    public void ShowSettings() => Nav.SelectedItem = Nav.SettingsItem;
}
