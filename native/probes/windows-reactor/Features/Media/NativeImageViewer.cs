using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Foundation;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;
using Windows.System;

namespace Sproyt.WindowsProbe;

internal sealed record NativeImageDecoded(BitmapImage Bitmap, uint OriginalWidth, uint OriginalHeight, int DecodedWidth, int DecodedHeight)
{
    internal bool ReducedResolution => DecodedWidth < OriginalWidth || DecodedHeight < OriginalHeight;
}

// The original dimensions own the geometry. Decode dimensions only bound the
// bitmap allocation; a downsampled bitmap must never redefine what 100% means.
internal sealed class NativeImageViewer : IDisposable
{
    internal const int MaximumDecodeEdge = 8192;
    internal const long MaximumDecodePixels = 16L * 1024 * 1024;
    private readonly Image _image;
    private readonly TextBlock _percentage, _status;
    private readonly Button _zoomIn, _zoomOut, _save;
    private readonly Func<bool> _current;
    private readonly Func<CancellationToken, Task<bool>> _saveOriginal;
    private readonly Action _close;
    private readonly CancellationTokenSource _lifetime;
    private readonly Dictionary<uint, Point> _pointers = [];
    private Point _startingPoint, _startingCenter;
    private double _startingScale, _startingPanX, _startingPanY, _startingDistance;
    private Task? _saving;
    private XamlRoot? _xamlRoot;
    internal static NativeImageViewer? LastForProbe { get; private set; }
    internal Grid Root { get; }
    internal Grid Surface { get; }
    internal NativeImageViewport Viewport { get; }
    internal uint OriginalWidth { get; }
    internal uint OriginalHeight { get; }
    internal int DecodedWidth { get; }
    internal int DecodedHeight { get; }
    internal bool ReducedResolution { get; }
    internal bool IsDisposed { get; private set; }
    internal string? OriginalPathForProbe { get; }

    internal static async Task<NativeImageDecoded> DecodeAsync(IRandomAccessStream stream, CancellationToken cancellation)
    {
        var decoder = await BitmapDecoder.CreateAsync(stream).AsTask(cancellation);
        uint width = decoder.OrientedPixelWidth, height = decoder.OrientedPixelHeight;
        // Keep pathological dimensions out of native layout as well as decode.
        if (width == 0 || height == 0 || width > 100_000 || height > 100_000)
            throw new InvalidDataException("Biletet har ikkje støtta mål.");
        double scale = Math.Min(1, Math.Min(MaximumDecodeEdge / (double)Math.Max(width, height),
            Math.Sqrt(MaximumDecodePixels / ((double)width * height))));
        int decodedWidth = Math.Max(1, (int)Math.Floor(width * scale));
        int decodedHeight = Math.Max(1, (int)Math.Floor(height * scale));
        var bitmap = new BitmapImage
        {
            DecodePixelType = DecodePixelType.Physical,
            DecodePixelWidth = decodedWidth,
            DecodePixelHeight = decodedHeight
        };
        stream.Seek(0);
        await bitmap.SetSourceAsync(stream).AsTask(cancellation);
        return new(bitmap, width, height, decodedWidth, decodedHeight);
    }

    internal NativeImageViewer(NativeImageDecoded decoded, string name, string originalPath, SproytPalette palette,
        Func<bool> current, Func<CancellationToken, Task<bool>> saveOriginal, Action close, CancellationToken cancellation)
    {
        OriginalWidth = decoded.OriginalWidth; OriginalHeight = decoded.OriginalHeight;
        DecodedWidth = decoded.DecodedWidth; DecodedHeight = decoded.DecodedHeight; ReducedResolution = decoded.ReducedResolution;
        _current = current; _saveOriginal = saveOriginal; _close = close;
        _lifetime = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        Viewport = new(OriginalWidth, OriginalHeight);
        if (Program.LiveUiProbeDirectory is not null) { LastForProbe = this; OriginalPathForProbe = originalPath; }
        Root = new Grid { RowSpacing = 8, MinWidth = 220, HorizontalAlignment = HorizontalAlignment.Stretch };
        Root.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        Root.RowDefinitions.Add(new() { Height = GridLength.Auto });
        Root.RowDefinitions.Add(new() { Height = GridLength.Auto });
        AutomationProperties.SetAutomationId(Root, "native-image-viewer");
        Surface = new Grid { Background = palette.Paper, IsTabStop = true, MinHeight = 80, HorizontalAlignment = HorizontalAlignment.Stretch };
        AutomationProperties.SetAutomationId(Surface, "native-image-viewer-content");
        AutomationProperties.SetName(Surface, name + ". Bruk pluss, minus, 0 og 1 for zoom, og piltastane for panorering.");
        var canvas = new Canvas { IsHitTestVisible = false };
        _image = new Image { Source = decoded.Bitmap, Stretch = Stretch.Fill };
        AutomationProperties.SetName(_image, name);
        canvas.Children.Add(_image); Surface.Children.Add(canvas); Root.Children.Add(Surface);
        var controls = new NativeImageViewerToolbar();
        _zoomOut = Control("−", "Zoom ut", "zoom-out", () => Change(() => Viewport.ZoomBy(1 / 1.4)));
        var fit = Control("Tilpass", "Tilpass heile biletet til visinga", "fit", () => Change(Viewport.Fit));
        var actual = Control("1:1", "Vis faktisk storleik (100 prosent)", "actual", () => Change(Viewport.ActualSize));
        ToolTipService.SetToolTip(actual, "100 % er éin biletepunkt per logisk skjermpunkt.");
        _percentage = new TextBlock { Text = "", Foreground = palette.Ink, VerticalAlignment = VerticalAlignment.Center, MinWidth = 56, TextAlignment = TextAlignment.Center };
        AutomationProperties.SetAutomationId(_percentage, "native-image-viewer-percent");
        _zoomIn = Control("+", "Zoom inn", "zoom-in", () => Change(() => Viewport.ZoomBy(1.4)));
        _save = Control("Lagre", "Lagre originalbiletet", "save", () => { if (_saving is null || _saving.IsCompleted) _saving = SaveAsync(); });
        var closeButton = Control("Lukk", "Lukk bilete", "close", () => { if (!IsDisposed) _close(); });
        foreach (var child in new FrameworkElement[] { _zoomOut, fit, actual, _percentage, _zoomIn, _save, closeButton }) controls.Children.Add(child);
        Grid.SetRow(controls, 1); Root.Children.Add(controls);
        var resolution = new TextBlock { Foreground = palette.Muted, TextWrapping = TextWrapping.Wrap, FontSize = 13,
            Text = ReducedResolution ? "Avgrensa detaljnivå i visinga. Lagre originalbiletet for alle detaljane." : "" };
        AutomationProperties.SetAutomationId(resolution, "native-image-viewer-resolution");
        _status = new TextBlock { Foreground = palette.Muted, TextWrapping = TextWrapping.Wrap, FontSize = 13 };
        AutomationProperties.SetAutomationId(_status, "native-image-viewer-save-status");
        var messages = new StackPanel { Spacing = 4 }; messages.Children.Add(resolution); messages.Children.Add(_status);
        Grid.SetRow(messages, 2); Root.Children.Add(messages);
        Surface.SizeChanged += SizeChanged;
        Surface.PointerPressed += PointerPressed; Surface.PointerMoved += PointerMoved;
        Surface.PointerReleased += PointerReleased; Surface.PointerCanceled += PointerReleased;
        Surface.PointerCaptureLost += PointerCaptureLost; Surface.PointerWheelChanged += Wheel;
        Surface.DoubleTapped += DoubleTapped;
        Root.KeyDown += KeyDown;
        Root.Loaded += Loaded;
    }

    private static Button Control(string text, string name, string id, Action action)
    {
        var button = new Button { Content = text, MinWidth = 44, MinHeight = 44, Padding = new Thickness(8, 4, 8, 4) };
        AutomationProperties.SetName(button, name); AutomationProperties.SetAutomationId(button, "native-image-viewer-" + id);
        ToolTipService.SetToolTip(button, name); button.Click += (_, _) => action(); return button;
    }
    private bool Current => !IsDisposed && !_lifetime.IsCancellationRequested && _current();
    private void Loaded(object sender, RoutedEventArgs args)
    {
        if (!Current || Root.XamlRoot is not { } xamlRoot) return;
        if (_xamlRoot is not null) _xamlRoot.Changed -= XamlRootChanged;
        _xamlRoot = xamlRoot; xamlRoot.Changed += XamlRootChanged; ResizeRoot();
        Surface.Focus(FocusState.Programmatic);
    }
    private void XamlRootChanged(XamlRoot sender, XamlRootChangedEventArgs args) { if (Current) ResizeRoot(); }
    private void ResizeRoot()
    {
        if (_xamlRoot is null) return;
        Root.Width = Math.Max(220, Math.Min(1000, _xamlRoot.Size.Width - 80));
        Root.Height = Math.Max(180, Math.Min(800, _xamlRoot.Size.Height - 150));
    }
    private void Change(Action action) { if (!Current) return; ResetPointers(); action(); Render(); }
    private void SizeChanged(object sender, SizeChangedEventArgs args)
    {
        if (!Current || args.NewSize.Width <= 0 || args.NewSize.Height <= 0) return;
        ResetPointers(); Viewport.Resize(args.NewSize.Width, args.NewSize.Height); Render();
    }
    private void Render()
    {
        if (!Current || !Viewport.Ready) return;
        double width = OriginalWidth * Viewport.Scale, height = OriginalHeight * Viewport.Scale;
        _image.Width = width; _image.Height = height;
        Canvas.SetLeft(_image, (Surface.ActualWidth - width) / 2 + Viewport.PanX);
        Canvas.SetTop(_image, (Surface.ActualHeight - height) / 2 + Viewport.PanY);
        Surface.Clip = new RectangleGeometry { Rect = new Rect(0, 0, Math.Max(0, Surface.ActualWidth), Math.Max(0, Surface.ActualHeight)) };
        _percentage.Text = (Viewport.Scale * 100).ToString(Viewport.Scale < .01 ? "0.##" : "0.#", CultureInfo.GetCultureInfo("nn-NO")) + " %";
        AutomationProperties.SetName(_percentage, "Zoom " + _percentage.Text);
        _zoomOut.IsEnabled = Viewport.Scale > Viewport.MinScale + 1e-9;
        _zoomIn.IsEnabled = Viewport.Scale < Viewport.MaxScale - 1e-9;
    }
    private void BeginGesture()
    {
        var points = _pointers.Values.ToArray();
        _startingScale = Viewport.Scale; _startingPanX = Viewport.PanX; _startingPanY = Viewport.PanY;
        if (points.Length == 1) _startingPoint = points[0];
        if (points.Length == 2) { _startingCenter = Center(points[0], points[1]); _startingDistance = Math.Max(1, Distance(points[0], points[1])); }
    }
    private void PointerPressed(object sender, PointerRoutedEventArgs args)
    {
        if (!Current || _pointers.Count >= 2) return;
        var point = args.GetCurrentPoint(Surface);
        if (point.Properties.IsRightButtonPressed || point.Properties.IsMiddleButtonPressed || !Surface.CapturePointer(args.Pointer)) return;
        Surface.Focus(FocusState.Pointer); _pointers[args.Pointer.PointerId] = point.Position; BeginGesture(); args.Handled = true;
    }
    private void PointerMoved(object sender, PointerRoutedEventArgs args)
    {
        if (!Current || !_pointers.ContainsKey(args.Pointer.PointerId)) return;
        _pointers[args.Pointer.PointerId] = args.GetCurrentPoint(Surface).Position;
        var points = _pointers.Values.ToArray();
        if (points.Length == 2 && _startingDistance > .001)
        {
            var center = Center(points[0], points[1]);
            Viewport.Pinch(_startingScale, _startingPanX, _startingPanY, _startingCenter.X, _startingCenter.Y,
                center.X, center.Y, Distance(points[0], points[1]) / _startingDistance);
        }
        else if (points.Length == 1)
            Viewport.PanBy(_startingPanX + points[0].X - _startingPoint.X - Viewport.PanX,
                _startingPanY + points[0].Y - _startingPoint.Y - Viewport.PanY);
        Render(); args.Handled = true;
    }
    private void PointerReleased(object sender, PointerRoutedEventArgs args)
    { if (_pointers.Remove(args.Pointer.PointerId)) { Surface.ReleasePointerCapture(args.Pointer); BeginGesture(); args.Handled = true; } }
    private void PointerCaptureLost(object sender, PointerRoutedEventArgs args)
    { if (_pointers.Remove(args.Pointer.PointerId)) BeginGesture(); }
    private void ResetPointers() { _pointers.Clear(); Surface.ReleasePointerCaptures(); }
    private void Wheel(object sender, PointerRoutedEventArgs args)
    {
        if (!Current) return;
        var properties = args.GetCurrentPoint(Surface).Properties;
        if (properties.IsHorizontalMouseWheel) return;
        Change(() => Viewport.ZoomBy(Math.Exp(properties.MouseWheelDelta * .002))); args.Handled = true;
    }
    private void DoubleTapped(object sender, DoubleTappedRoutedEventArgs args)
    { if (Current) { Change(Viewport.ToggleZoom); args.Handled = true; } }
    private void KeyDown(object sender, KeyRoutedEventArgs args)
    {
        if (!Current) return;
        Action? action = args.Key switch
        {
            VirtualKey.Add or (VirtualKey)187 => () => Viewport.ZoomBy(1.25),
            VirtualKey.Subtract or (VirtualKey)189 => () => Viewport.ZoomBy(.8),
            VirtualKey.Home => Viewport.Fit,
            VirtualKey.Number0 or VirtualKey.NumberPad0 or VirtualKey.Number1 or VirtualKey.NumberPad1 => Viewport.ActualSize,
            VirtualKey.Left => () => Viewport.PanBy(64, 0), VirtualKey.Right => () => Viewport.PanBy(-64, 0),
            VirtualKey.Up => () => Viewport.PanBy(0, 64), VirtualKey.Down => () => Viewport.PanBy(0, -64),
            VirtualKey.Escape => _close, _ => null
        };
        if (action is null) return;
        Change(action); args.Handled = true;
    }
    private async Task SaveAsync()
    {
        if (!Current) return;
        _save.IsEnabled = false;
        try { if (await _saveOriginal(_lifetime.Token) && Current) _status.Text = "Originalbiletet er lagra."; }
        catch (OperationCanceledException) { }
        catch { if (Current) _status.Text = "Biletet kunne ikkje lagrast. Prøv igjen."; }
        finally { if (Current) _save.IsEnabled = true; }
    }
    internal async Task WaitForSaveAsync() { try { if (_saving is not null) await _saving; } finally { _lifetime.Dispose(); } }
    private static Point Center(Point a, Point b) => new((a.X + b.X) / 2, (a.Y + b.Y) / 2);
    private static double Distance(Point a, Point b) => Math.Sqrt(Math.Pow(a.X - b.X, 2) + Math.Pow(a.Y - b.Y, 2));
    public void Dispose()
    {
        if (IsDisposed) return;
        IsDisposed = true; _lifetime.Cancel(); ResetPointers(); _image.Source = null;
        Surface.SizeChanged -= SizeChanged;
        Surface.PointerPressed -= PointerPressed; Surface.PointerMoved -= PointerMoved;
        Surface.PointerReleased -= PointerReleased; Surface.PointerCanceled -= PointerReleased;
        Surface.PointerCaptureLost -= PointerCaptureLost; Surface.PointerWheelChanged -= Wheel;
        Surface.DoubleTapped -= DoubleTapped; Root.KeyDown -= KeyDown; Root.Loaded -= Loaded;
        if (_xamlRoot is not null) { _xamlRoot.Changed -= XamlRootChanged; _xamlRoot = null; }
        if (ReferenceEquals(LastForProbe, this)) LastForProbe = null;
    }
}

// A small responsive native panel keeps every 44-DIP control reachable in a
// narrow dialog without a fixed horizontal toolbar overflowing its viewport.
internal sealed class NativeImageViewerToolbar : Panel
{
    protected override Size MeasureOverride(Size availableSize)
    {
        double width = double.IsFinite(availableSize.Width) ? availableSize.Width : 900;
        double x = 0, y = 0, rowHeight = 0, used = 0;
        foreach (var child in Children)
        {
            child.Measure(new Size(width, double.PositiveInfinity)); var size = child.DesiredSize;
            if (x > 0 && x + size.Width > width) { y += rowHeight + 4; x = 0; rowHeight = 0; }
            used = Math.Max(used, x + size.Width); x += size.Width + 4; rowHeight = Math.Max(rowHeight, size.Height);
        }
        return new Size(Math.Min(width, used), y + rowHeight);
    }
    protected override Size ArrangeOverride(Size finalSize)
    {
        double x = 0, y = 0, rowHeight = 0;
        foreach (var child in Children)
        {
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > finalSize.Width) { y += rowHeight + 4; x = 0; rowHeight = 0; }
            child.Arrange(new Rect(x, y, size.Width, size.Height)); x += size.Width + 4; rowHeight = Math.Max(rowHeight, size.Height);
        }
        return finalSize;
    }
}
