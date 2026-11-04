using Microsoft.Windows.Storage.Pickers;
using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;

namespace Sproyt.WindowsProbe;

internal static class NativeUploadShell
{
    private static readonly SemaphoreSlim Dialog = new(1, 1);
    internal static Func<Task<IReadOnlyList<string>>>? PickFilesForProbe;
    internal static readonly string[] Extensions = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".heic", ".heif", ".avif", ".mp4", ".mov", ".webm", ".mkv", ".pdf", ".txt", ".csv", ".zip"];

    internal static async Task PickAsync(ILiveApplication app, NativeMessageActionScope scope)
    {
        var target = app.CaptureUploadTarget(scope);
        if (target is null) return;
        if (!await Dialog.WaitAsync(0)) return;
        try
        {
            if (Program.LiveUiProbeDirectory is not null && PickFilesForProbe is { } injected)
            { await app.AddUploadFilesAsync(target, await injected()); return; }
            if (Program.MainWindowId is not { } window || Program.CloseAdmissionClosed) return;
            var picker = new FileOpenPicker(window) { CommitButtonText = "Legg ved", SuggestedStartLocation = PickerLocationId.DocumentsLibrary };
            foreach (string extension in Extensions) picker.FileTypeFilter.Add(extension);
            var selected = await picker.PickMultipleFilesAsync();
            if (selected.Count == 0) return;
            await app.AddUploadFilesAsync(target, selected.Select(item => item.Path).ToArray());
        }
        finally { Dialog.Release(); }
    }

    internal static async Task PasteAsync(ILiveApplication app, NativeMessageActionScope scope)
    {
        var target = app.CaptureUploadTarget(scope);
        if (target is null) return;
        if (Program.CloseAdmissionClosed) return;
        var clipboard = Clipboard.GetContent();
        if (clipboard.Contains(StandardDataFormats.StorageItems))
        {
            var items = await clipboard.GetStorageItemsAsync();
            var paths = items.OfType<StorageFile>().Where(file => file.Path.Length != 0).Select(file => file.Path).ToArray();
            if (paths.Length != 0) { await app.AddUploadFilesAsync(target, paths); return; }
        }
        if (!clipboard.Contains(StandardDataFormats.Bitmap)) throw new IOException("Utklippstavla har ikkje eit bilete eller filer.");
        var reference = await clipboard.GetBitmapAsync();
        using var input = await reference.OpenReadAsync();
        if (input.Size > 64UL * 1024 * 1024) throw new IOException("Biletet på utklippstavla er for stort. Bruk Legg ved.");
        var decoder = await BitmapDecoder.CreateAsync(input);
        // The transfer path remains chunked. Only clipboard raster decoding has
        // this explicit 16-megapixel bound before any RGBA buffer is allocated.
        if (decoder.PixelWidth == 0 || decoder.PixelHeight == 0 || (ulong)decoder.PixelWidth * decoder.PixelHeight > 16 * 1024 * 1024)
            throw new IOException("Biletet på utklippstavla er for stort. Lagre det og bruk Legg ved.");
        using var pixels = await decoder.GetSoftwareBitmapAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied);
        using var encoded = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, encoded);
        encoder.SetSoftwareBitmap(pixels); await encoder.FlushAsync(); encoded.Seek(0);
        using var stream = encoded.AsStreamForRead();
        await app.AddUploadStreamAsync(target, stream, "Utklipp.png", "image/png");
    }

    internal static string DocumentExtension(string contentType) => contentType switch
    {
        "application/pdf" => ".pdf", "application/zip" => ".zip", "text/plain" => ".txt", "text/csv" => ".csv",
        _ => throw new IOException("Denne dokumenttypen kan ikkje lastast ned her.")
    };
}
