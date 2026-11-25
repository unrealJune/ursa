using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using QRCoder;
using Windows.Storage.Streams;

namespace Ursa.App.Services;

/// <summary>QR codes for the codes a phone scans (ursa-link:, ursa-invite:).</summary>
public static class Qr
{
    /// <summary>
    /// A black-on-white PNG with its quiet zone, rendered large: WinUI scales an Image smoothly, so
    /// a module must be many pixels wide to stay sharp at the size it is shown.
    /// </summary>
    public static async Task<ImageSource> ImageAsync(string text)
    {
        byte[] png = PngByteQRCodeHelper.GetQRCode(text, QRCodeGenerator.ECCLevel.M, 12);
        using var stream = new InMemoryRandomAccessStream();
        using (var writer = new DataWriter(stream.GetOutputStreamAt(0)))
        {
            writer.WriteBytes(png);
            await writer.StoreAsync();
            await writer.FlushAsync();
        }
        var image = new BitmapImage();
        await image.SetSourceAsync(stream);
        return image;
    }
}
