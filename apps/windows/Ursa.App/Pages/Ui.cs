using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.UI;

namespace Ursa.App.Pages;

/// <summary>Small builders for the Pool and Usage pages, which are drawn from data.</summary>
internal static class Ui
{
    public static Brush Res(string key) => (Brush)Application.Current.Resources[key];

    public static Color Accent => ((SolidColorBrush)Res("AccentFillColorDefaultBrush")).Color;

    /// <summary>The series colours: the accent at three strengths, then neutrals.</summary>
    public static Brush Series(int i) => i switch
    {
        0 => new SolidColorBrush(Accent),
        1 => new SolidColorBrush(Color.FromArgb(160, Accent.R, Accent.G, Accent.B)),
        2 => new SolidColorBrush(Color.FromArgb(80, Accent.R, Accent.G, Accent.B)),
        3 => Res("ControlStrongFillColorDisabledBrush"),
        _ => Res("ControlFillColorSecondaryBrush"),
    };

    public static Brush Track => Res("ControlFillColorSecondaryBrush");
    public static Brush Secondary => Res("TextFillColorSecondaryBrush");

    public static Border Card(UIElement child, double padding = 16) => new()
    {
        Background = Res("CardBackgroundFillColorDefaultBrush"),
        BorderBrush = Res("CardStrokeColorDefaultBrush"),
        BorderThickness = new Thickness(1),
        CornerRadius = new CornerRadius(8),
        Padding = new Thickness(padding + 4, padding, padding + 4, padding),
        Child = child,
    };

    public static TextBlock Section(string text) => new()
    {
        Text = text,
        Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"],
        Margin = new Thickness(0, 16, 0, 4),
    };

    public static TextBlock Text(string text, bool hint = false, double size = 0, bool strong = false)
    {
        var t = new TextBlock { Text = text, TextWrapping = TextWrapping.NoWrap, TextTrimming = TextTrimming.CharacterEllipsis };
        if (hint) { t.Foreground = Secondary; t.FontSize = 12; }
        if (size > 0) t.FontSize = size;
        if (strong) t.FontWeight = FontWeights.SemiBold;
        return t;
    }

    public static TextBlock Big(string text, double size = 40) => new()
    {
        Text = text,
        FontSize = size,
        FontWeight = FontWeights.SemiBold,
        FontFamily = new FontFamily("Segoe UI Variable Display"),
    };

    /// <summary>
    /// A horizontal bar of segments, each a share of <paramref name="total"/>; optionally a
    /// mark at <paramref name="tickAt"/> and a hatched-looking band over a range.
    /// </summary>
    public static Grid Bar(IEnumerable<(double Value, Brush Fill)> segments, double total, double height = 12, double? tickAt = null,
        (double From, double To)? band = null)
    {
        var bar = PlainBar(segments, total, height, tickAt);
        if (band is not { } b || total <= 0) return bar;
        double from = Math.Clamp(b.From, 0, total), to = Math.Clamp(b.To, from, total);
        var overlay = new Grid { Margin = new Thickness(0, -3, 0, -3) };
        overlay.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(from, 0.0001), GridUnitType.Star) });
        overlay.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(to - from, 0.0001), GridUnitType.Star) });
        overlay.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(total - to, 0.0001), GridUnitType.Star) });
        var r = new Rectangle
        {
            Fill = Res("SystemFillColorCautionBrush"),
            Opacity = 0.75,
            RadiusX = 2,
            RadiusY = 2,
            MinWidth = 3, // never narrower than a visible sliver
        };
        Grid.SetColumn(r, 1);
        overlay.Children.Add(r);
        var host = new Grid { Height = height };
        host.Children.Add(bar);
        host.Children.Add(overlay);
        return host;
    }

    private static Grid PlainBar(IEnumerable<(double Value, Brush Fill)> segments, double total, double height, double? tickAt)
    {
        var g = new Grid { Height = height, CornerRadius = new CornerRadius(3), Background = Track };
        var inner = new Grid();
        double used = 0;
        int col = 0;
        foreach (var (v, fill) in segments)
        {
            double w = Math.Max(0, Math.Min(v, total - used));
            if (w <= 0) continue;
            inner.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w, GridUnitType.Star) });
            var r = new Rectangle { Fill = fill };
            Grid.SetColumn(r, col++);
            inner.Children.Add(r);
            used += w;
        }
        if (total - used > 0) inner.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(total - used, GridUnitType.Star) });
        g.Children.Add(inner);
        if (tickAt is { } t && total > 0)
        {
            // A thin mark at t, drawn over the bar.
            var tick = new Grid { Margin = new Thickness(0, -3, 0, -3) };
            tick.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(t, 0.0001), GridUnitType.Star) });
            tick.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(total - t, 0.0001), GridUnitType.Star) });
            var mark = new Rectangle { Width = 2, Fill = Res("TextFillColorPrimaryBrush"), HorizontalAlignment = HorizontalAlignment.Right };
            tick.Children.Add(mark);
            var host = new Grid { Height = height };
            host.Children.Add(g);
            host.Children.Add(tick);
            g.CornerRadius = new CornerRadius(3);
            return host;
        }
        return g;
    }

    public static StackPanel Legend(IEnumerable<(string Label, Brush Fill)> items)
    {
        var p = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 16 };
        foreach (var (label, fill) in items)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
            row.Children.Add(new Rectangle { Width = 10, Height = 10, RadiusX = 2, RadiusY = 2, Fill = fill, VerticalAlignment = VerticalAlignment.Center });
            row.Children.Add(Text(label, hint: true));
            p.Children.Add(row);
        }
        return p;
    }

    /// <summary>Label on the left, value on the right.</summary>
    public static Grid Row(string label, string value, bool strong = false)
    {
        var g = new Grid { Padding = new Thickness(0, 3, 0, 3) };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var l = Text(label);
        l.Foreground = Secondary;
        var v = Text(value, strong: strong);
        Grid.SetColumn(v, 1);
        g.Children.Add(l);
        g.Children.Add(v);
        return g;
    }

    public static Border Badge(string text, bool accent)
    {
        var a = Accent;
        return new Border
        {
            CornerRadius = new CornerRadius(10),
            Padding = new Thickness(8, 1, 8, 2),
            HorizontalAlignment = HorizontalAlignment.Left,
            Background = accent ? new SolidColorBrush(Color.FromArgb(36, a.R, a.G, a.B)) : null,
            BorderBrush = accent ? null : Res("ControlStrokeColorDefaultBrush"),
            BorderThickness = new Thickness(accent ? 0 : 1),
            Child = new TextBlock { Text = text, FontSize = 12, Foreground = accent ? Res("AccentTextFillColorPrimaryBrush") : Secondary },
        };
    }

    /// <summary>A table: header row, then rows of cells. Columns given as GridLengths.</summary>
    public static Grid Table(GridLength[] columns, string[] header, IEnumerable<UIElement[]> rows)
    {
        var g = new Grid { ColumnSpacing = 16, RowSpacing = 10 };
        foreach (var c in columns) g.ColumnDefinitions.Add(new ColumnDefinition { Width = c });
        int r = 0;
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        for (int i = 0; i < header.Length; i++)
        {
            var h = Text(header[i], hint: true);
            Grid.SetColumn(h, i);
            g.Children.Add(h);
        }
        foreach (var cells in rows)
        {
            r++;
            g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            for (int i = 0; i < cells.Length; i++)
            {
                if (cells[i] is FrameworkElement fe) fe.VerticalAlignment = VerticalAlignment.Center;
                Grid.SetRow((FrameworkElement)cells[i], r);
                Grid.SetColumn((FrameworkElement)cells[i], i);
                g.Children.Add(cells[i]);
            }
        }
        return g;
    }

    /// <summary>A small bar and the limit beside it.</summary>
    public static UIElement LimitCell(long used, long? limit)
    {
        if (limit is not { } l) return Text("", hint: true);
        var g = new Grid { ColumnSpacing = 8 };
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 80 });
        g.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(72) });
        var bar = Bar([(used, Series(1))], Math.Max(l, 1), 6);
        bar.VerticalAlignment = VerticalAlignment.Center;
        var t = Text(Services.StorageReport.Size(l), hint: true);
        t.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(t, 1);
        g.Children.Add(bar);
        g.Children.Add(t);
        return g;
    }
}
