using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json;
using Microsoft.UI.Xaml.Media;

namespace Ursa.App.Services;

/// <summary>One file in the library, as the gallery shows it.</summary>
public sealed class GalleryItem : INotifyPropertyChanged
{
    public required string FileId { get; init; }
    public required string Name { get; init; }
    public long Size { get; init; }
    public string Mime { get; init; } = "application/octet-stream";
    public long CreatedMs { get; init; }
    public uint? Width { get; init; }
    public uint? Height { get; init; }
    public bool HasThumb { get; init; }

    public bool IsImage => Mime.StartsWith("image/");
    public bool IsVideo => Mime.StartsWith("video/");
    public bool IsMedia => IsImage || IsVideo;

    /// <summary>Width over height, for the justified rows. Files without a size are 3:2.</summary>
    public double AspectRatio => Width is > 0 && Height is > 0 ? Math.Clamp((double)Width.Value / Height.Value, 0.4, 3.0) : 1.5;

    public DateTimeOffset Created => DateTimeOffset.FromUnixTimeMilliseconds(CreatedMs).ToLocalTime();
    public string DateText => Created.ToString("d MMM yyyy");
    public string SizeText => FormatBytes(Size);
    public string Details => IsMedia && Width is > 0 ? $"{Width} × {Height} · {SizeText} · {DateText}" : $"{SizeText} · {DateText}";

    /// <summary>Segoe Fluent glyph for files without a thumbnail.</summary>
    public string Glyph => IsVideo ? "" : IsImage ? "" : "";

    private ImageSource? thumbnail;
    public ImageSource? Thumbnail
    {
        get => thumbnail;
        set { thumbnail = value; Changed(); Changed(nameof(ShowGlyph)); }
    }

    public bool ShowGlyph => thumbnail is null;

    public static GalleryItem FromJson(JsonElement e) => new()
    {
        FileId = e.GetProperty("file_id").GetString()!,
        Name = e.GetProperty("name").GetString() ?? "untitled",
        Size = e.GetProperty("size").GetInt64(),
        Mime = e.GetProperty("mime").GetString() ?? "application/octet-stream",
        CreatedMs = e.GetProperty("created_ms").GetInt64(),
        Width = e.TryGetProperty("width", out var w) && w.ValueKind == JsonValueKind.Number ? w.GetUInt32() : null,
        Height = e.TryGetProperty("height", out var h) && h.ValueKind == JsonValueKind.Number ? h.GetUInt32() : null,
        HasThumb = e.TryGetProperty("has_thumb", out var t) && t.GetBoolean(),
    };

    public static string FormatBytes(long b) => b switch
    {
        < 1024 => $"{b} B",
        < 1024 * 1024 => $"{b / 1024.0:0.#} KB",
        < 1024L * 1024 * 1024 => $"{b / 1048576.0:0.#} MB",
        _ => $"{b / 1073741824.0:0.##} GB",
    };

    public event PropertyChangedEventHandler? PropertyChanged;
    private void Changed([CallerMemberName] string? name = null) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
}
