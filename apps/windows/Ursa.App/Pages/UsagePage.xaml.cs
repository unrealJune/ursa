using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Microsoft.UI.Xaml.Shapes;
using Ursa.App.Services;

namespace Ursa.App.Pages;

/// <summary>What you store, what it takes in the pool, what you give back, and how it grows.</summary>
public sealed partial class UsagePage : Page
{
    private Library Library => App.Library;

    // The column takes the window's width up to its maximum, whatever its content asks for: the
    // scroll viewer would otherwise centre it on the width its content wants.
    private void OnScrollerSized(object sender, SizeChangedEventArgs e) => Column.Width = Math.Min(880, e.NewSize.Width);
    private StorageReport? report;

    public UsagePage() => InitializeComponent();

    protected override async void OnNavigatedTo(NavigationEventArgs e)
    {
        Library.SwitchEnded += OnSwitchEnded;
        await LoadAsync();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => Library.SwitchEnded -= OnSwitchEnded;

    private async void OnSwitchEnded() => await LoadAsync();

    private async Task LoadAsync()
    {
        try
        {
            report = await Library.StorageAsync();
            ErrorBar.IsOpen = false;
            Render(report);
        }
        catch (Exception e)
        {
            ErrorBar.Message = e.Message;
            ErrorBar.IsOpen = true;
        }
        finally
        {
            LoadingRing.IsActive = false;
            LoadingRing.Visibility = Visibility.Collapsed;
        }
    }

    private void Render(StorageReport r)
    {
        Body.Children.Clear();
        string S(long b) => StorageReport.Size(b);

        // Your files against what the current protection lets you store.
        var hero = new StackPanel { Spacing = 10 };
        var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        line.Children.Add(Ui.Big(S(r.StoredBytes)));
        var of = Ui.Text($"of {S(r.LimitBytes)}", size: 18);
        of.Foreground = Ui.Secondary;
        of.VerticalAlignment = VerticalAlignment.Bottom;
        of.Margin = new Thickness(0, 0, 0, 8);
        line.Children.Add(of);
        hero.Children.Add(line);
        double total = Math.Max(r.LimitBytes, r.StoredBytes);
        hero.Children.Add(Ui.Bar([(r.Photos, Ui.Series(0)), (r.Videos, Ui.Series(1)), (r.OtherFiles, Ui.Series(2))], Math.Max(total, 1), 18));
        hero.Children.Add(Ui.Legend([
            ($"Photos {S(r.Photos)}", Ui.Series(0)),
            ($"Videos {S(r.Videos)}", Ui.Series(1)),
            ($"Files {S(r.OtherFiles)}", Ui.Series(2)),
            ($"Free {S(Math.Max(0, r.LimitBytes - r.StoredBytes))}", Ui.Track),
        ]));
        Body.Children.Add(Ui.Card(hero));

        // In the pool, and fair share.
        var two = new Grid { ColumnSpacing = 8 };
        two.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        two.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var pool = new StackPanel();
        pool.Children.Add(Ui.Text("In the pool", strong: true));
        pool.Children.Add(Ui.Row("Your node", S(r.HomeBytes)));
        pool.Children.Add(Ui.Row("Friends' nodes", S(r.FriendsBytes)));
        pool.Children.Add(new Rectangle { Height = 1, Fill = Ui.Res("DividerStrokeColorDefaultBrush"), Margin = new Thickness(0, 4, 0, 2) });
        pool.Children.Add(Ui.Row("Total", S(r.HomeBytes + r.FriendsBytes), strong: true));
        var fair = new StackPanel { Spacing = 4 };
        fair.Children.Add(Ui.Text("Fair share", strong: true));
        fair.Children.Add(Ui.Row("On friends", $"{S(r.FriendsBytes)} of {S(r.GiveBytes)}"));
        fair.Children.Add(Ui.Bar([(r.FriendsBytes, Ui.Series(0))], Math.Max(r.GiveBytes, Math.Max(r.FriendsBytes, 1)), 10));
        fair.Children.Add(Ui.Row("Friends on you", S(r.Hosting.Sum(h => h.UsedBytes))));
        var c1 = Ui.Card(pool);
        var c2 = Ui.Card(fair);
        Grid.SetColumn(c2, 1);
        two.Children.Add(c1);
        two.Children.Add(c2);
        Body.Children.Add(two);

        var cols = new[] { new GridLength(1, GridUnitType.Star), new GridLength(90), new GridLength(1.4, GridUnitType.Star) };

        Body.Children.Add(Ui.Section("Your pieces"));
        Body.Children.Add(Ui.Card(Ui.Table(cols, ["Node", "Stored", "Limit"], r.Members.Select(m => new UIElement[]
        {
            Ui.Text(m.Own ? "You" : m.Name, strong: true),
            Right(S(m.UsedBytes)),
            Ui.LimitCell(m.UsedBytes, m.GivesYou),
        }))));

        if (r.Hosting.Count > 0)
        {
            Body.Children.Add(Ui.Section("Hosting"));
            Body.Children.Add(Ui.Card(Ui.Table(cols, ["Friend", "Stored", "Limit"], r.Hosting.Select(h => new UIElement[]
            {
                Ui.Text(h.Name, strong: true),
                Right(S(h.UsedBytes)),
                Ui.LimitCell(h.UsedBytes, h.QuotaBytes),
            }))));
        }

        if (r.Disk is { } d)
        {
            long free = Math.Max(0, d.Free - d.Reserved);
            Body.Children.Add(Ui.Section($"Disk · {S(d.Total)}"));
            var disk = new StackPanel { Spacing = 10 };
            disk.Children.Add(Ui.Bar([(d.Yours, Ui.Series(0)), (d.Friends, Ui.Series(1)), (d.Reserved, Ui.Series(2)), (d.Total - d.Free - d.Yours - d.Friends, Ui.Series(3))], d.Total, 18));
            disk.Children.Add(Ui.Legend([
                ($"Yours {S(d.Yours)}", Ui.Series(0)),
                ($"Friends {S(d.Friends)}", Ui.Series(1)),
                ($"Reserved {S(d.Reserved)}", Ui.Series(2)),
                ($"Other {S(Math.Max(0, d.Total - d.Free - d.Yours - d.Friends))}", Ui.Series(3)),
                ($"Free {S(free)}", Ui.Track),
            ]));
            Body.Children.Add(Ui.Card(disk));
        }

        Body.Children.Add(Ui.Section("Growth"));
        var chart = new Canvas { Height = 180, HorizontalAlignment = HorizontalAlignment.Stretch };
        chart.SizeChanged += (_, e) => DrawGrowth(chart, r, e.NewSize.Width);
        Body.Children.Add(Ui.Card(chart));
    }

    private static TextBlock Right(string text)
    {
        var t = Ui.Text(text);
        t.HorizontalAlignment = HorizontalAlignment.Right;
        return t;
    }

    /// <summary>Stored bytes by month, a projection at the recent pace, and the limit.</summary>
    private static void DrawGrowth(Canvas c, StorageReport r, double width)
    {
        c.Children.Clear();
        if (width <= 0 || r.Growth.Count == 0) return;
        const double padL = 56, padR = 8, padT = 10, padB = 22, h = 180;
        var hist = r.Growth;
        int ahead = 6;
        // Pace: the last three months' average.
        int back = Math.Min(3, hist.Count - 1);
        double pace = back > 0 ? (hist[^1].Bytes - hist[^(back + 1)].Bytes) / (double)back : 0;
        int points = hist.Count + ahead;
        double yMax = Math.Max(Math.Max(r.LimitBytes, hist[^1].Bytes + pace * ahead), 1) * 1.05;
        double X(int i) => padL + i * (width - padL - padR) / (points - 1);
        double Y(double v) => padT + (1 - v / yMax) * (h - padT - padB);

        foreach (double t in new[] { 0, yMax / 2, yMax })
        {
            c.Children.Add(new Line { X1 = padL, X2 = width - padR, Y1 = Y(t), Y2 = Y(t), Stroke = Ui.Res("DividerStrokeColorDefaultBrush"), StrokeThickness = 1 });
            var label = Ui.Text(StorageReport.Size((long)t), hint: true);
            label.FontSize = 11;
            label.Width = padL - 8;
            label.TextAlignment = TextAlignment.Right;
            Canvas.SetLeft(label, 0);
            Canvas.SetTop(label, Y(t) - 8);
            c.Children.Add(label);
        }
        if (r.LimitBytes > 0 && r.LimitBytes < yMax)
        {
            c.Children.Add(new Line { X1 = padL, X2 = width - padR, Y1 = Y(r.LimitBytes), Y2 = Y(r.LimitBytes), Stroke = Ui.Res("TextFillColorTertiaryBrush"), StrokeThickness = 1, StrokeDashArray = [2, 3] });
            var lim = Ui.Text(StorageReport.Size(r.LimitBytes), hint: true);
            lim.FontSize = 11;
            Canvas.SetLeft(lim, width - padR - 60);
            Canvas.SetTop(lim, Y(r.LimitBytes) - 18);
            c.Children.Add(lim);
        }

        var area = new Polygon { Fill = Ui.Series(2) };
        area.Points.Add(new Windows.Foundation.Point(X(0), Y(0)));
        var line = new Polyline { Stroke = Ui.Series(0), StrokeThickness = 2 };
        for (int i = 0; i < hist.Count; i++)
        {
            var pt = new Windows.Foundation.Point(X(i), Y(hist[i].Bytes));
            area.Points.Add(pt);
            line.Points.Add(pt);
        }
        area.Points.Add(new Windows.Foundation.Point(X(hist.Count - 1), Y(0)));
        c.Children.Add(area);
        c.Children.Add(line);
        if (pace > 0)
        {
            var proj = new Polyline { Stroke = Ui.Series(0), StrokeThickness = 1.5, StrokeDashArray = [4, 4] };
            for (int i = 0; i <= ahead; i++)
                proj.Points.Add(new Windows.Foundation.Point(X(hist.Count - 1 + i), Y(hist[^1].Bytes + pace * i)));
            c.Children.Add(proj);
        }
        var dot = new Ellipse { Width = 8, Height = 8, Fill = Ui.Series(0) };
        Canvas.SetLeft(dot, X(hist.Count - 1) - 4);
        Canvas.SetTop(dot, Y(hist[^1].Bytes) - 4);
        c.Children.Add(dot);

        // Month labels: every other one when narrow.
        int step = width < 520 ? 2 : 1;
        var last = DateTime.TryParse(hist[^1].Month + "-01", out var d) ? d : DateTime.Today;
        for (int i = 0; i < points; i += step)
        {
            string m = i < hist.Count ? hist[i].Short : last.AddMonths(i - hist.Count + 1).ToString("MMM");
            var t = Ui.Text(m, hint: true);
            t.FontSize = 11;
            Canvas.SetLeft(t, X(i) - 10);
            Canvas.SetTop(t, h - padB + 4);
            c.Children.Add(t);
        }
    }
}
