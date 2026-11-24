using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.FileProperties;
using Windows.Storage.Streams;

namespace Ursa.App.Services;

/// <summary>What the gallery needs to know about a file before it uploads: type, dates, size, thumbnail.</summary>
public sealed record MediaInfo(string Mime, long CreatedMs, long ModifiedMs, uint? Width, uint? Height, string? ThumbPath);

public static class Thumbnailer
{
    private const uint ThumbLongSide = 640;

    private static readonly Dictionary<string, string> MimeByExtension = new(StringComparer.OrdinalIgnoreCase)
    {
        [".jpg"] = "image/jpeg", [".jpeg"] = "image/jpeg", [".png"] = "image/png", [".gif"] = "image/gif",
        [".webp"] = "image/webp", [".heic"] = "image/heic", [".heif"] = "image/heif", [".bmp"] = "image/bmp",
        [".tif"] = "image/tiff", [".tiff"] = "image/tiff", [".dng"] = "image/x-adobe-dng", [".avif"] = "image/avif",
        [".mp4"] = "video/mp4", [".mov"] = "video/quicktime", [".m4v"] = "video/x-m4v", [".mkv"] = "video/x-matroska",
        [".webm"] = "video/webm", [".avi"] = "video/x-msvideo",
    };

    public static string MimeOf(StorageFile file) =>
        !string.IsNullOrEmpty(file.ContentType) && file.ContentType != "application/octet-stream"
            ? file.ContentType
            : MimeByExtension.GetValueOrDefault(file.FileType, "application/octet-stream");

    public static async Task<MediaInfo> InspectAsync(StorageFile file)
    {
        string mime = MimeOf(file);
        var basic = await file.GetBasicPropertiesAsync();
        long modified = basic.DateModified.ToUnixTimeMilliseconds();
        long created = file.DateCreated.ToUnixTimeMilliseconds();
        uint? width = null, height = null;
        string? thumb = null;
        Directory.CreateDirectory(LocalStore.ThumbScratch);
        string thumbPath = Path.Combine(LocalStore.ThumbScratch, Guid.NewGuid().ToString("N") + ".jpg");

        try
        {
            if (mime.StartsWith("image/"))
            {
                var props = await file.Properties.GetImagePropertiesAsync();
                if (props.DateTaken.Year > 1900) created = props.DateTaken.ToUnixTimeMilliseconds();
                using IRandomAccessStream stream = await file.OpenReadAsync();
                var decoder = await BitmapDecoder.CreateAsync(stream);
                width = decoder.OrientedPixelWidth;
                height = decoder.OrientedPixelHeight;
                await WriteThumbAsync(decoder, thumbPath);
                thumb = thumbPath;
            }
            else if (mime.StartsWith("video/"))
            {
                var props = await file.Properties.GetVideoPropertiesAsync();
                if (props.Width > 0) { width = props.Width; height = props.Height; }
                if (props.Orientation is VideoOrientation.Rotate90 or VideoOrientation.Rotate270)
                    (width, height) = (height, width);
                using var t = await file.GetThumbnailAsync(ThumbnailMode.SingleItem, ThumbLongSide, ThumbnailOptions.ResizeThumbnail);
                if (t is not null && t.Type == ThumbnailType.Image)
                {
                    var decoder = await BitmapDecoder.CreateAsync(t);
                    await WriteThumbAsync(decoder, thumbPath);
                    thumb = thumbPath;
                }
            }
        }
        catch
        {
            // No codec (HEIC without the extension, an odd RAW): upload anyway, without a thumbnail.
            thumb = null;
        }
        return new MediaInfo(mime, created, modified, width, height, thumb);
    }

    private static async Task WriteThumbAsync(BitmapDecoder decoder, string path)
    {
        double scale = Math.Min(1.0, (double)ThumbLongSide / Math.Max(decoder.PixelWidth, decoder.PixelHeight));
        uint w = Math.Max(1, (uint)Math.Round(decoder.PixelWidth * scale));
        uint h = Math.Max(1, (uint)Math.Round(decoder.PixelHeight * scale));
        var transform = new BitmapTransform { ScaledWidth = w, ScaledHeight = h, InterpolationMode = BitmapInterpolationMode.Fant };
        var pixels = await decoder.GetPixelDataAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, transform,
            ExifOrientationMode.RespectExifOrientation, ColorManagementMode.ColorManageToSRgb);
        // The scale applies before orientation: a rotated photo comes out with the sides swapped.
        bool swapped = decoder.OrientedPixelWidth != decoder.PixelWidth;
        (uint ow, uint oh) = swapped ? (h, w) : (w, h);
        var folder = await StorageFolder.GetFolderFromPathAsync(Path.GetDirectoryName(path)!);
        var file = await folder.CreateFileAsync(Path.GetFileName(path), CreationCollisionOption.ReplaceExisting);
        using var output = await file.OpenAsync(FileAccessMode.ReadWrite);
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.JpegEncoderId, output,
            [new KeyValuePair<string, BitmapTypedValue>("ImageQuality", new BitmapTypedValue(0.82, Windows.Foundation.PropertyType.Single))]);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, ow, oh, 96, 96, pixels.DetachPixelData());
        await encoder.FlushAsync();
    }
}
