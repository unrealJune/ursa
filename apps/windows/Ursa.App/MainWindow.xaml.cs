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
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;
        string icon = Path.Combine(AppContext.BaseDirectory, "Assets", "ursa.ico");
        if (File.Exists(icon)) AppWindow.SetIcon(icon);
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1280, 840));
        Nav.SelectedItem = PhotosItem;
    }

    public IntPtr Hwnd => WinRT.Interop.WindowNative.GetWindowHandle(this);

    public Brush StatusBrush(bool connected) =>
        new SolidColorBrush(connected ? ColorHelper.FromArgb(255, 0x3F, 0xB9, 0x50) : ColorHelper.FromArgb(255, 0x9E, 0x9E, 0x9E));

    private void OnPaneToggleRequested(TitleBar sender, object args) => Nav.IsPaneOpen = !Nav.IsPaneOpen;

    private void OnNavigationChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        Type page = args.IsSettingsSelected ? typeof(SettingsPage) : typeof(GalleryPage);
        if (ContentFrame.CurrentSourcePageType != page)
            ContentFrame.Navigate(page, null, new EntranceNavigationTransitionInfo());
    }

    public void ShowSettings() => Nav.SelectedItem = Nav.SettingsItem;
}
