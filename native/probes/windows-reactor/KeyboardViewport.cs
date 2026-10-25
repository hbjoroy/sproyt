using Microsoft.UI.Reactor;
using System.Runtime.InteropServices;
using Windows.UI.ViewManagement;

namespace Sproyt.WindowsProbe;

// Desktop interop: GetForCurrentView requires a UWP CoreWindow. Native WinUI
// already brings the editor into view; notify the chat after that transition.
internal sealed class KeyboardViewport : IDisposable
{
    private InputPane? _pane;
    private readonly Microsoft.UI.Dispatching.DispatcherQueue _dispatcher;
    private bool _disposed;
    internal bool Available => _pane is not null;

    internal KeyboardViewport(ReactorWindow window)
    {
        _dispatcher = window.NativeWindow.DispatcherQueue;
        try
        {
            _pane = InputPaneInterop.GetForWindow(WinRT.Interop.WindowNative.GetWindowHandle(window.NativeWindow));
            _pane.Showing += Changed;
            _pane.Hiding += Changed;
        }
        catch (COMException exception)
        {
            // Some Windows Server/headless installations have no input pane.
            // Ordinary viewport resize handling remains available there.
            System.Diagnostics.Debug.WriteLine($"Input pane unavailable: {exception.HResult:X8}");
            _pane = null;
        }
    }

    private void Changed(InputPane sender, InputPaneVisibilityEventArgs args) => _dispatcher.TryEnqueue(() =>
    {
        if (!_disposed) PilotComponent.ViewportTransition?.Invoke();
    });

    public void Dispose()
    {
        _disposed = true;
        if (_pane is null) return;
        _pane.Showing -= Changed;
        _pane.Hiding -= Changed;
        _pane = null;
    }
}
