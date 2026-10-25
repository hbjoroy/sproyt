using Microsoft.UI.Dispatching;
using Microsoft.UI.Reactor;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using System.Runtime.InteropServices;
using Windows.Foundation;
using Windows.UI.ViewManagement;

namespace Sproyt.WindowsProbe;

// A desktop keyboard can overlay the client without resizing its HWND. Keep
// the outer root fixed and reserve only the actual bottom overlap inside it.
internal sealed class KeyboardViewport : IDisposable
{
    private InputPane? _pane;
    private Grid? _root;
    private Thickness _originalPadding;
    private readonly Microsoft.UI.Xaml.Window _window;
    private readonly AppWindow _appWindow;
    private readonly DispatcherQueue _dispatcher;
    private readonly DispatcherQueueTimer _timer;
    private Rect? _probeOcclusion;
    private bool _disposed, _active = true;
    internal bool Available => _pane is not null;
    internal double Inset { get; private set; }

    internal KeyboardViewport(ReactorWindow window)
    {
        _window = window.NativeWindow;
        _appWindow = _window.AppWindow;
        _dispatcher = _window.DispatcherQueue;
        _timer = _dispatcher.CreateTimer();
        _timer.Interval = TimeSpan.FromMilliseconds(200);
        _timer.IsRepeating = true;
        _timer.Tick += Poll;
        _window.Activated += Activated;
        _appWindow.Changed += WindowChanged;
        try
        {
            _pane = InputPaneInterop.GetForWindow(WinRT.Interop.WindowNative.GetWindowHandle(_window));
            _pane.Showing += Showing;
            _pane.Hiding += Hiding;
        }
        catch (COMException exception)
        {
            System.Diagnostics.Debug.WriteLine($"Input pane unavailable: {exception.HResult:X8}");
            _pane = null;
        }
    }

    internal void Attach(Grid? root)
    {
        if (ReferenceEquals(_root, root) || _disposed) return;
        _timer.Stop();
        if (_root is not null)
        {
            _root.SizeChanged -= SizeChanged;
            _root.GotFocus -= FocusChanged;
            _root.LostFocus -= FocusChanged;
            _root.Padding = _originalPadding;
        }
        Inset = 0;
        _root = root;
        if (root is null) return;
        _originalPadding = root.Padding;
        root.SizeChanged += SizeChanged;
        root.GotFocus += FocusChanged;
        root.LostFocus += FocusChanged;
        Refresh();
    }

    private void Showing(InputPane sender, InputPaneVisibilityEventArgs args)
    {
        if (!_dispatcher.HasThreadAccess) { _dispatcher.TryEnqueue(Refresh); return; }
        if (_probeOcclusion is not null || _disposed) return;
        Apply(args.OccludedRect);
        // Set this before returning, not in a queued callback. We now own
        // avoidance of this overlap; prevent Windows from also panning it.
        if (Inset > 0) args.EnsuredFocusedElementInView = true;
        UpdatePolling();
    }

    private void Hiding(InputPane sender, InputPaneVisibilityEventArgs args)
    {
        if (!_dispatcher.HasThreadAccess) { _dispatcher.TryEnqueue(Refresh); return; }
        if (_probeOcclusion is not null || _disposed) return;
        Apply(default);
        UpdatePolling();
    }

    private void Refresh()
    {
        if (_disposed || _root is null) return;
        if (Program.CloseAdmissionClosed) { _timer.Stop(); return; }
        if (_probeOcclusion is Rect probe) Apply(probe);
        else if (_active && _pane is not null)
        {
            try { Apply(_pane.OccludedRect); }
            catch (COMException) { Apply(default); }
        }
        UpdatePolling();
    }

    private void Apply(Rect occlusion)
    {
        if (_root?.XamlRoot is null || Program.CloseAdmissionClosed) return;
        var origin = _root.TransformToVisual(null).TransformPoint(new Point());
        var bounds = new Rect(origin.X, origin.Y, _root.ActualWidth,
            Math.Max(0, _root.ActualHeight - _originalPadding.Bottom));
        double inset = BottomOverlap(bounds, occlusion);
        if (Math.Abs(inset - Inset) < .5) return;
        PilotComponent.ViewportTransition?.Invoke();
        Inset = inset;
        _root.Padding = new Thickness(_originalPadding.Left, _originalPadding.Top,
            _originalPadding.Right, _originalPadding.Bottom + inset);
    }

    // InputPane coordinates are client DIPs, not physical screen pixels. A
    // floating keyboard or an already resized client must not add an inset.
    internal static double BottomOverlap(Rect client, Rect keyboard)
    {
        if (client.Width <= 0 || client.Height <= 0 || keyboard.Top <= 0 ||
            keyboard.Width <= 0 || keyboard.Height <= 0 || keyboard.Bottom < client.Bottom - .5 ||
            keyboard.Right <= client.Left || keyboard.Left >= client.Right) return 0;
        return Math.Max(0, client.Bottom - Math.Max(client.Top, keyboard.Top));
    }

    private void UpdatePolling()
    {
        // Some desktop Windows versions miss Showing on automatic activation.
        // Sample only during text entry/known overlap in this active window.
        bool editing = _root?.XamlRoot is not null && PilotComponent.EditorForClose is not null &&
            ReferenceEquals(FocusManager.GetFocusedElement(_root.XamlRoot), PilotComponent.EditorForClose);
        if (!_disposed && _active && _pane is not null && _probeOcclusion is null &&
            !Program.CloseAdmissionClosed && (editing || Inset > 0))
        {
            if (!_timer.IsRunning) _timer.Start();
        }
        else _timer.Stop();
    }

    private void Poll(DispatcherQueueTimer timer, object args) => Refresh();
    private void SizeChanged(object sender, SizeChangedEventArgs args) => Refresh();
    private void FocusChanged(object sender, RoutedEventArgs args) => _dispatcher.TryEnqueue(Refresh);
    private void WindowChanged(AppWindow sender, AppWindowChangedEventArgs args) => Refresh();
    private void Activated(object sender, WindowActivatedEventArgs args)
    {
        _active = args.WindowActivationState != WindowActivationState.Deactivated;
        Refresh();
    }

    internal void SetOcclusionForProbe(Rect? occlusion)
    {
        if (Program.UiProbeDirectory is null) throw new InvalidOperationException("UI probe only.");
        _probeOcclusion = occlusion;
        Refresh();
    }

    public void Dispose()
    {
        if (_disposed) return;
        Attach(null);
        _disposed = true;
        _timer.Stop();
        _timer.Tick -= Poll;
        _window.Activated -= Activated;
        _appWindow.Changed -= WindowChanged;
        if (_pane is null) return;
        _pane.Showing -= Showing;
        _pane.Hiding -= Hiding;
        _pane = null;
    }
}
