using Microsoft.UI.Xaml.Media;
using System.Globalization;
using Windows.UI;

namespace Sproyt.WindowsProbe;

// Native equivalents of the Sprøyt semantic light/dark tokens. Constructed on
// the UI thread and reused across renders; the selected palette follows the
// same explicit theme state as the native controls' RequestedTheme.
internal sealed record SproytPalette(Brush Ink, Brush Muted, Brush Paper, Brush Chrome,
    Brush Surface, Brush Rule, Brush Selection, Brush Error, Brush Accent, Brush AccentInk)
{
    internal static readonly SproytPalette Light = new(
        Paint("#191b18"), Paint("#62695f"), Paint("#faf9f5"), Paint("#f0f1e8"),
        Paint("#ffffff"), Paint("#d9ddd0"), Paint("#e3e8d8"), Paint("#963d2d"), Paint("#d9ed70"), Paint("#191b18"));
    internal static readonly SproytPalette Dark = new(
        Paint("#ebece1"), Paint("#adb4a6"), Paint("#191b18"), Paint("#141612"),
        Paint("#22261f"), Paint("#3b4435"), Paint("#303829"), Paint("#f0b0a0"), Paint("#d9ed70"), Paint("#191b18"));

    private static Brush Paint(string hex)
    {
        uint rgb = uint.Parse(hex.AsSpan(1), NumberStyles.HexNumber, CultureInfo.InvariantCulture);
        return new SolidColorBrush(Color.FromArgb(255, (byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb));
    }
}
