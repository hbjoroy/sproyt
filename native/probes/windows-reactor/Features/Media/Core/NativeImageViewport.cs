namespace Sproyt.WindowsProbe;

// The image origin and pan are centered in the viewport. At 100%, each
// oriented source pixel occupies one DIP, regardless of decode resolution.
internal sealed class NativeImageViewport
{
    private enum Sizing { OpeningWidth, Fit, Manual }

    private readonly double _originalWidth, _originalHeight;
    private double _width, _height;
    private Sizing _sizing = Sizing.OpeningWidth;

    internal NativeImageViewport(double originalWidth, double originalHeight)
    {
        // BitmapDecoder exposes the oriented pixel extents as unsigned 32-bit values.
        if (!(originalWidth >= 1 && originalWidth <= uint.MaxValue))
            throw new ArgumentOutOfRangeException(nameof(originalWidth));
        if (!(originalHeight >= 1 && originalHeight <= uint.MaxValue))
            throw new ArgumentOutOfRangeException(nameof(originalHeight));
        _originalWidth = originalWidth;
        _originalHeight = originalHeight;
    }

    internal bool Ready => _width > 0;
    internal double Scale { get; private set; } = 1;
    internal double PanX { get; private set; }
    internal double PanY { get; private set; }
    internal double FitScale { get; private set; } = 1;
    internal double MinScale => Math.Min(.05, FitScale);
    internal double MaxScale => 5;

    internal void Resize(double width, double height)
    {
        if (!double.IsFinite(width) || !double.IsFinite(height) || width <= 0 || height <= 0) return;
        double widthScale = Math.Min(1, width / _originalWidth);
        double fitted = Math.Min(widthScale, height / _originalHeight);
        if (fitted <= 0) return;
        _width = width;
        _height = height;
        FitScale = fitted;
        double scale = _sizing switch
        {
            Sizing.OpeningWidth => widthScale,
            Sizing.Fit => FitScale,
            _ => Scale
        };
        Update(scale, PanX, PanY);
    }

    internal void Fit()
    {
        if (!Ready) return;
        _sizing = Sizing.Fit;
        Update(FitScale, 0, 0);
    }

    internal void ActualSize()
    {
        if (!Ready) return;
        _sizing = Sizing.Manual;
        Update(1, 0, 0);
    }

    internal void ToggleZoom()
    {
        if (!Ready) return;
        if (_sizing == Sizing.Manual && Scale > FitScale * 1.01)
        {
            Fit();
            return;
        }
        _sizing = Sizing.Manual;
        Update(FitScale < 1 ? 1 : 2.5, PanX, PanY);
    }

    internal void ZoomBy(double factor)
    {
        if (!Ready || !double.IsFinite(factor) || factor <= 0) return;
        _sizing = Sizing.Manual;
        Update(Scale * factor, PanX, PanY);
    }

    internal void PanBy(double dx, double dy)
    {
        if (!Ready || !double.IsFinite(dx) || !double.IsFinite(dy)) return;
        Update(Scale, PanX + dx, PanY + dy);
    }

    // Midpoints are DIPs from the top left. Each move uses the same gesture
    // baseline. Clamp the new scale before placing the original focal pixel.
    internal void Pinch(double startingScale, double startingPanX, double startingPanY,
        double startCenterX, double startCenterY, double centerX, double centerY, double factor)
    {
        if (!Ready || !(startingScale >= MinScale && startingScale <= MaxScale) ||
            !double.IsFinite(startingPanX) || !double.IsFinite(startingPanY) ||
            !double.IsFinite(startCenterX) || !double.IsFinite(startCenterY) ||
            !double.IsFinite(centerX) || !double.IsFinite(centerY) ||
            !double.IsFinite(factor) || factor < 0) return;

        double scale = Math.Clamp(startingScale * factor, MinScale, MaxScale);
        double focalX = (startCenterX - _width / 2 - startingPanX) / startingScale;
        double focalY = (startCenterY - _height / 2 - startingPanY) / startingScale;
        double panX = centerX - _width / 2 - focalX * scale;
        double panY = centerY - _height / 2 - focalY * scale;
        if (double.IsNaN(panX) || double.IsNaN(panY)) return;
        _sizing = Sizing.Manual;
        Update(scale, panX, panY);
    }

    private void Update(double scale, double panX, double panY)
    {
        Scale = Math.Clamp(scale, MinScale, MaxScale);
        double maxX = Math.Max(0, (_originalWidth * Scale - _width) / 2);
        double maxY = Math.Max(0, (_originalHeight * Scale - _height) / 2);
        PanX = Math.Clamp(panX, -maxX, maxX);
        PanY = Math.Clamp(panY, -maxY, maxY);
    }
}
