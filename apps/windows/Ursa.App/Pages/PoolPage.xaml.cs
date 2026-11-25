using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Ursa.App.Services;

namespace Ursa.App.Pages;

/// <summary>
/// Who is in the pool, how the library is protected (any k of n pieces rebuild a file), what
/// each choice lets you store, how much you give friends, and switching between choices.
/// </summary>
public sealed partial class PoolPage : Page
{
    private Library Library => App.Library;

    // The column takes the window's width up to its maximum, whatever its content asks for: the
    // scroll viewer would otherwise centre it on the width its content wants.
    private void OnScrollerSized(object sender, SizeChangedEventArgs e) => Column.Width = Math.Min(880, e.NewSize.Width);
    private StorageReport? report;
    private (int K, int N) selected;
    private (int K, int N)? custom;
    private bool settingSlider;
    private bool switching => Library.IsSwitching;
    private readonly DispatcherTimer giveDebounce = new() { Interval = TimeSpan.FromMilliseconds(700) };

    public PoolPage()
    {
        InitializeComponent();
        giveDebounce.Tick += async (_, _) => { giveDebounce.Stop(); await ApplyGiveAsync(); };
    }

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        Library.PropertyChanged += OnLibraryChanged;
        Library.SwitchEnded += OnSwitchEnded;
        ShowSwitchProgress();
        await LoadAsync(keepSelection: false);
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        // The switch itself carries on; this page just stops watching it.
        Library.PropertyChanged -= OnLibraryChanged;
        Library.SwitchEnded -= OnSwitchEnded;
        giveDebounce.Stop();
    }

    private void OnLibraryChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Library.IsSwitching) or nameof(Library.SwitchPercent)) ShowSwitchProgress();
    }

    private void ShowSwitchProgress()
    {
        SwitchProgress.Visibility = Library.IsSwitching ? Visibility.Visible : Visibility.Collapsed;
        SwitchProgress.IsIndeterminate = Library.SwitchPercent <= 0;
        SwitchProgress.Value = Library.SwitchPercent;
        UpdateSwitch();
    }

    private async void OnSwitchEnded()
    {
        if (Library.SwitchError is { } err) ShowError(err);
        await LoadAsync(keepSelection: true);
    }

    private async Task LoadAsync(bool keepSelection)
    {
        try
        {
            report = await Library.StorageAsync(custom);
            if (!keepSelection) selected = (report.K, report.N);
            ErrorBar.IsOpen = false;
            Render();
        }
        catch (Exception e)
        {
            ShowError(e.Message);
        }
        finally
        {
            LoadingRing.IsActive = false;
            LoadingRing.Visibility = Visibility.Collapsed;
        }
    }

    private void ShowError(string message)
    {
        ErrorBar.Message = message;
        ErrorBar.IsOpen = true;
    }

    private void Render()
    {
        if (report is null) return;
        Body.Visibility = Visibility.Visible;
        MembersHost.Content = Ui.Card(Members(report));
        RenderChoices();
        RenderGive();
    }

    /// <summary>Everything that depends on the selected choice.</summary>
    private void RenderChoices()
    {
        if (report is null) return;
        ChoicesHost.Children.Clear();
        foreach (var group in report.Choices.GroupBy(c => c.Survives).OrderBy(g => g.Key))
        {
            ChoicesHost.Children.Add(new TextBlock
            {
                Text = $"Survives {group.Key} down",
                FontSize = 13,
                FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
                Margin = new Thickness(0, 10, 0, 4),
            });
            var grid = new ItemsRepeater
            {
                Layout = new UniformGridLayout { MinItemWidth = 196, MinColumnSpacing = 8, MinRowSpacing = 8, ItemsStretch = UniformGridLayoutItemsStretch.Fill },
                ItemsSource = group.OrderBy(c => (double)c.N / c.K).Select(ChoiceCard).ToList(),
            };
            ChoicesHost.Children.Add(grid);
        }
        CapacityHost.Content = Ui.Card(Capacity(report));
        UpdateSwitch();
    }

    private static UIElement Members(StorageReport r)
    {
        var grid = new ItemsRepeater
        {
            Layout = new UniformGridLayout { MinItemWidth = 130, MinColumnSpacing = 8, MinRowSpacing = 8, ItemsStretch = UniformGridLayoutItemsStretch.Fill },
            ItemsSource = r.Members.Select(m =>
            {
                var p = new StackPanel { Spacing = 2 };
                p.Children.Add(Ui.Text(m.Own ? "You" : m.Name, strong: true));
                p.Children.Add(Ui.Text(m.GivesYou is { } g ? $"{m.UpText} · {StorageReport.Size(g)}" : m.UpText, hint: true));
                return (UIElement)new Border
                {
                    Padding = new Thickness(12, 10, 12, 10),
                    CornerRadius = new CornerRadius(6),
                    Background = Ui.Res("SubtleFillColorSecondaryBrush"),
                    BorderBrush = Ui.Res("CardStrokeColorDefaultBrush"),
                    BorderThickness = new Thickness(1),
                    Child = p,
                };
            }).ToList(),
        };
        return grid;
    }

    private UIElement ChoiceCard(Choice c)
    {
        bool isSelected = c.Is(selected.K, selected.N);
        var p = new StackPanel { Spacing = 6 };
        var top = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        var name = Ui.Big(c.Name, 22);
        if (!c.Available) name.Foreground = Ui.Res("TextFillColorDisabledBrush");
        top.Children.Add(name);
        if (c.Shape != "")
        {
            var shape = Ui.Text(c.Shape, hint: true);
            shape.VerticalAlignment = VerticalAlignment.Bottom;
            shape.Margin = new Thickness(0, 0, 0, 3);
            top.Children.Add(shape);
        }
        p.Children.Add(top);
        var facts = new Grid { ColumnSpacing = 8, RowSpacing = 2 };
        facts.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        facts.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        facts.RowDefinitions.Add(new RowDefinition());
        facts.RowDefinitions.Add(new RowDefinition());
        AddFact(facts, 0, "Store", c.Available ? StorageReport.Size(c.LimitBytes) : "—", strong: true);
        AddFact(facts, 1, "Offline", c.OfflineText);
        p.Children.Add(facts);
        var badges = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, MinHeight = 22 };
        if (!c.Available) badges.Children.Add(Ui.Badge($"Needs {c.N} nodes", accent: false));
        else if (c.Recommended) badges.Children.Add(Ui.Badge("Recommended", accent: true));
        p.Children.Add(badges);

        var b = new ToggleButton
        {
            Content = p,
            IsChecked = isSelected,
            IsEnabled = c.Available && !switching,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Left,
            Padding = new Thickness(14, 12, 14, 12),
            Style = (Style)Application.Current.Resources["DefaultToggleButtonStyle"],
            Background = Ui.Res("CardBackgroundFillColorDefaultBrush"),
            BorderBrush = isSelected ? Ui.Res("AccentFillColorDefaultBrush") : Ui.Res("CardStrokeColorDefaultBrush"),
            BorderThickness = new Thickness(isSelected ? 2 : 1),
            CornerRadius = new CornerRadius(8),
        };
        AutomationProperties.SetName(b, $"{c.Name}{(c.Shape == "" ? "" : " " + c.Shape)}, store {StorageReport.Size(c.LimitBytes)}");
        // A selected card keeps the card look, not the toggle's filled accent.
        b.Resources["ToggleButtonBackgroundChecked"] = Ui.Res("CardBackgroundFillColorDefaultBrush");
        b.Resources["ToggleButtonBackgroundCheckedPointerOver"] = Ui.Res("CardBackgroundFillColorSecondaryBrush");
        b.Resources["ToggleButtonBackgroundCheckedPressed"] = Ui.Res("CardBackgroundFillColorSecondaryBrush");
        b.Resources["ToggleButtonForegroundChecked"] = Ui.Res("TextFillColorPrimaryBrush");
        b.Resources["ToggleButtonForegroundCheckedPointerOver"] = Ui.Res("TextFillColorPrimaryBrush");
        b.Resources["ToggleButtonForegroundCheckedPressed"] = Ui.Res("TextFillColorPrimaryBrush");
        b.Resources["ToggleButtonBorderBrushChecked"] = Ui.Res("AccentFillColorDefaultBrush");
        b.Resources["ToggleButtonBorderBrushCheckedPointerOver"] = Ui.Res("AccentFillColorDefaultBrush");
        b.Resources["ToggleButtonBorderBrushCheckedPressed"] = Ui.Res("AccentFillColorDefaultBrush");
        b.Click += (_, _) =>
        {
            selected = (c.K, c.N);
            RenderChoices();
        };
        return b;
    }

    private static void AddFact(Grid g, int row, string label, string value, bool strong = false)
    {
        var l = Ui.Text(label, hint: true);
        var v = Ui.Text(value, strong: strong);
        v.FontSize = 13;
        Grid.SetRow(l, row);
        Grid.SetRow(v, row);
        Grid.SetColumn(v, 1);
        g.Children.Add(l);
        g.Children.Add(v);
    }

    private UIElement Capacity(StorageReport r)
    {
        var sel = r.Choices.FirstOrDefault(c => c.Is(selected.K, selected.N)) ?? r.Current;
        var p = new StackPanel { Spacing = 14 };

        var head = new Grid { ColumnSpacing = 24 };
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var big = Ui.Big(StorageReport.Size(sel.LimitBytes));
        if (r.StoredBytes > sel.LimitBytes) big.Foreground = Ui.Res("SystemFillColorCriticalBrush");
        head.Children.Add(big);
        double home = 1.0 / sel.K, friends = (sel.N - 1.0) / sel.K;
        var split = new StackPanel { Spacing = 6, VerticalAlignment = VerticalAlignment.Center, MaxWidth = 420, HorizontalAlignment = HorizontalAlignment.Left };
        split.Children.Add(Ui.Bar([(home, Ui.Series(1)), (friends, Ui.Series(0))], home + friends, 10));
        split.Children.Add(Ui.Legend([($"Yours {home:0.##}×", Ui.Series(1)), ($"Friends {friends:0.##}×", Ui.Series(0))]));
        Grid.SetColumn(split, 1);
        head.Children.Add(split);
        p.Children.Add(head);

        // A switch holds one batch of files twice (old pieces and new) until it commits it.
        bool moving = !sel.Is(r.K, r.N) || r.PendingFiles > 0;
        long peakFiles = sel.N > 0 ? sel.SwitchExtraBytes * sel.K / sel.N : 0;
        if (moving && sel.Available)
        {
            var extra = Ui.Row("While switching", $"+{StorageReport.Size(sel.SwitchExtraBytes)}", strong: true);
            if (!sel.SwitchFits) ((TextBlock)extra.Children[1]).Foreground = Ui.Res("SystemFillColorCriticalBrush");
            extra.MaxWidth = 420;
            extra.HorizontalAlignment = HorizontalAlignment.Left;
            extra.MinWidth = 260;
            p.Children.Add(extra);
        }

        var options = r.Choices.Where(c => c.Available).ToList();
        double max = Math.Max(options.Select(c => (double)c.LimitBytes).DefaultIfEmpty(0).Max(), r.StoredBytes) * 1.08;
        if (max <= 0) max = 1;
        var bars = new Grid { ColumnSpacing = 12, RowSpacing = 8 };
        bars.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(80) });
        bars.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        bars.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(72) });
        int row = 0;
        foreach (var c in options)
        {
            bool isSel = c.Is(sel.K, sel.N);
            bars.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var label = Ui.Text(c.Name, hint: !isSel, strong: isSel);
            label.FontSize = 13;
            // The selected option's bar also shows the room a switch to it takes, past what is
            // stored now.
            (double, double)? band = isSel && moving && peakFiles > 0 ? (r.StoredBytes, r.StoredBytes + peakFiles) : null;
            var bar = Ui.Bar([(c.LimitBytes, isSel ? Ui.Series(0) : Ui.Series(2))], max, 14, tickAt: r.StoredBytes, band: band);
            var value = Ui.Text(StorageReport.Size(c.LimitBytes));
            value.FontSize = 13;
            value.HorizontalAlignment = HorizontalAlignment.Right;
            foreach (var (el, col) in new (FrameworkElement, int)[] { (label, 0), (bar, 1), (value, 2) })
            {
                el.VerticalAlignment = VerticalAlignment.Center;
                Grid.SetRow(el, row);
                Grid.SetColumn(el, col);
                bars.Children.Add(el);
            }
            ToolTipService.SetToolTip(bar, $"Stored now: {StorageReport.Size(r.StoredBytes)}");
            row++;
        }
        p.Children.Add(bars);
        return p;
    }

    private void RenderGive()
    {
        if (report is null) return;
        double giveGb = report.GiveBytes / 1e9;
        double maxGb = report.Disk is { } d ? Math.Max((d.Total - d.Yours) / 1e9, giveGb) : Math.Max(giveGb * 4, 100);
        settingSlider = true;
        GiveSlider.Maximum = Math.Ceiling(maxGb);
        GiveSlider.Value = giveGb;
        GiveSlider.IsEnabled = report.Members.Any(m => !m.Own);
        settingSlider = false;
        GiveValue.Text = StorageReport.Size(report.GiveBytes);
    }

    private void OnGiveChanged(object sender, RangeBaseValueChangedEventArgs e)
    {
        GiveValue.Text = StorageReport.Size((long)(e.NewValue * 1e9));
        if (settingSlider) return;
        giveDebounce.Stop();
        giveDebounce.Start();
    }

    private async Task ApplyGiveAsync()
    {
        try
        {
            await Library.SetGiveAsync((long)(GiveSlider.Value * 1e9));
            report = await Library.StorageAsync(custom);
            RenderChoices();
            GiveValue.Text = StorageReport.Size(report.GiveBytes);
        }
        catch (Exception e)
        {
            ShowError(e.Message);
        }
    }

    private async void OnCustom(object sender, RoutedEventArgs e)
    {
        int k = (int)CustomK.Value, n = (int)CustomN.Value;
        if (double.IsNaN(CustomK.Value) || double.IsNaN(CustomN.Value) || k < 1 || n <= k)
        {
            ShowError("Use fewer pieces needed than pieces in total, e.g. 3 of 5.");
            return;
        }
        custom = (k, n);
        selected = (k, n);
        await LoadAsync(keepSelection: true);
    }

    private void UpdateSwitch()
    {
        if (report is null) return;
        var sel = report.Choices.FirstOrDefault(c => c.Is(selected.K, selected.N)) ?? report.Current;
        bool isCurrent = sel.Is(report.K, report.N);
        bool needed = !isCurrent || report.PendingFiles > 0;
        bool fits = sel.SwitchFits;
        SwitchButton.Content = switching ? "Switching" : !needed ? "In use" : !fits ? "Not enough space"
            : $"Switch to {sel.Name}{(sel.Shape == "" ? "" : $" ({sel.Shape})")}";
        SwitchButton.IsEnabled = !switching && needed && sel.Available && fits;
    }

    private async void OnSwitch(object sender, RoutedEventArgs e)
    {
        try
        {
            ErrorBar.IsOpen = false;
            await Library.StartSwitchAsync(selected.K, selected.N);
            report = await Library.StorageAsync(custom);
            RenderChoices();
        }
        catch (Exception ex)
        {
            ShowError(ex.Message);
        }
    }
}
