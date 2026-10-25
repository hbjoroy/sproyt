using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Sproyt.WindowsProbe;

// One native timer per mounted timeline, restarted during geometry changes.
// Follow the tail only while the reader was already there. No render or Task
// is allocated for each resize event, and scrolling remains a native operation.
internal sealed class ChatViewportFollow : IDisposable
{
    private readonly ScrollViewer _view;
    private readonly Func<bool> _active;
    private readonly Action _revealEnd;
    private readonly DispatcherQueueTimer _timer;
    private readonly PointerEventHandler _pointerHandler;
    private readonly KeyEventHandler _keyHandler;
    private bool _atEnd = true, _disposed, _pending, _userInteracting;
    private double _width, _height, _extent;

    internal int Adjustments { get; private set; }
    internal bool Pending => _pending;

    internal ChatViewportFollow(ScrollViewer view, Func<bool> active, Action revealEnd)
    {
        _view = view; _active = active; _revealEnd = revealEnd;
        _timer = view.DispatcherQueue.CreateTimer();
        _timer.Interval = TimeSpan.FromMilliseconds(180);
        _timer.IsRepeating = false;
        _timer.Tick += Settled;
        _pointerHandler = PointerInput;
        _keyHandler = KeyInput;
        RememberGeometry();
        view.SizeChanged += SizeChanged;
        view.ViewChanged += ViewChanged;
        view.ViewChanging += ViewChanging;
        view.AddHandler(UIElement.PointerPressedEvent, _pointerHandler, true);
        view.AddHandler(UIElement.PointerWheelChangedEvent, _pointerHandler, true);
        view.AddHandler(UIElement.KeyDownEvent, _keyHandler, true);
    }

    internal void Transition()
    {
        if (_disposed || !_active() || !_atEnd || _view.ViewportHeight <= 0) return;
        _pending = true;
        _timer.Stop();
        _timer.Start();
    }

    internal void Synchronize()
    {
        CancelPending();
        _userInteracting = false;
        RememberGeometry();
        _atEnd = _view.ScrollableHeight - _view.VerticalOffset < 24;
    }

    private void SizeChanged(object sender, SizeChangedEventArgs args)
    {
        _userInteracting = false;
        RememberGeometry();
        Transition();
    }

    private void ViewChanged(object? sender, ScrollViewerViewChangedEventArgs args)
    {
        bool geometryChanged = Math.Abs(_width - _view.ViewportWidth) > .5 ||
            Math.Abs(_height - _view.ViewportHeight) > .5 || Math.Abs(_extent - _view.ExtentHeight) > .5;
        RememberGeometry();
        if (geometryChanged && !_userInteracting) { Transition(); return; }
        // Layout can change offsets while the viewport is shrinking. That
        // does not mean the reader intentionally left the end of the chat.
        if (_pending || !_active()) return;
        _atEnd = _view.ScrollableHeight - _view.VerticalOffset < 24;
        if (!args.IsIntermediate) _userInteracting = false;
    }

    private void ViewChanging(object? sender, ScrollViewerViewChangingEventArgs args)
    {
        if (!_active() || (_pending && !_userInteracting)) return;
        // Observe an explicit scroll before virtualization can revise its
        // extent. Resize-induced changes are already inside the debounce.
        _atEnd = _view.ScrollableHeight - args.FinalView.VerticalOffset < 24;
        if (!_atEnd) CancelPending();
    }

    private void RememberGeometry()
    {
        _width = _view.ViewportWidth; _height = _view.ViewportHeight; _extent = _view.ExtentHeight;
    }

    private void Settled(DispatcherQueueTimer timer, object args)
    {
        _pending = false;
        if (_disposed || !_active() || !_atEnd) return;
        Adjustments++;
        _revealEnd();
    }

    private void PointerInput(object sender, PointerRoutedEventArgs args) { CancelPending(); _userInteracting = true; }
    private void KeyInput(object sender, KeyRoutedEventArgs args) { CancelPending(); _userInteracting = true; }
    private void CancelPending() { _timer.Stop(); _pending = false; }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        CancelPending();
        _timer.Tick -= Settled;
        _view.SizeChanged -= SizeChanged;
        _view.ViewChanged -= ViewChanged;
        _view.ViewChanging -= ViewChanging;
        _view.RemoveHandler(UIElement.PointerPressedEvent, _pointerHandler);
        _view.RemoveHandler(UIElement.PointerWheelChangedEvent, _pointerHandler);
        _view.RemoveHandler(UIElement.KeyDownEvent, _keyHandler);
    }
}
