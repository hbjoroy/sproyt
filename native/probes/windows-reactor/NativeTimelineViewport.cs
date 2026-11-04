using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;

namespace Sproyt.WindowsProbe;

internal sealed record NativeTimelineContext(string UserId, string SessionStamp, string ChannelId,
    long ViewGeneration, NativeNavigationVisit Visit, IReadOnlyList<NativeMessage> Messages, bool Loading);
internal sealed record NativeViewportObservation(NativeTimelineContext Context, NativeScrollAnchor Anchor,
    IReadOnlyList<string> VisibleMessageIds);

// A single native timer settles layout before restoring an ID-based anchor or
// reporting actual visible rows. It never guesses read progress from the tail.
internal sealed class NativeTimelineViewport : IDisposable
{
    private readonly ScrollViewer _view;
    private readonly Func<bool> _active;
    private readonly Action<NativeViewportObservation> _observe;
    private readonly DispatcherQueueTimer _timer;
    private readonly PointerEventHandler _pointer;
    private readonly KeyEventHandler _key;
    private NativeTimelineContext? _context;
    private NativeScrollAnchor _anchor = new();
    private NativeScrollAnchor? _restore;
    private int _attempts, _stable;
    private bool _disposed, _adjusting, _userInteracting;
    private long _lastInteraction;
    private double _extent, _width, _height;
    internal bool Restoring => _restore is not null;
    internal bool AnchorUnavailable { get; private set; }
    internal int Restorations { get; private set; }

    internal NativeTimelineViewport(ScrollViewer view, Func<bool> active, Action<NativeViewportObservation> observe)
    {
        _view = view; _active = active; _observe = observe;
        _timer = view.DispatcherQueue.CreateTimer();
        _timer.IsRepeating = false;
        _timer.Interval = TimeSpan.FromMilliseconds(140);
        _timer.Tick += Settled;
        _pointer = UserPointer; _key = UserKey;
        view.ViewChanged += ViewChanged;
        view.SizeChanged += SizeChanged;
        view.LayoutUpdated += LayoutUpdated;
        view.AddHandler(UIElement.PointerPressedEvent, _pointer, true);
        view.AddHandler(UIElement.PointerWheelChangedEvent, _pointer, true);
        view.AddHandler(UIElement.KeyDownEvent, _key, true);
        RememberGeometry();
    }

    internal void Update(NativeTimelineContext? context, NativeScrollAnchor? incoming = null)
    {
        if (_disposed) return;
        bool changedView = !SameView(_context, context);
        bool changedRows = _context is not null && context is not null &&
            (!_context.Messages.Select(item => (item.Id, item.Body, item.DeletedAt)).SequenceEqual(
                context.Messages.Select(item => (item.Id, item.Body, item.DeletedAt))));
        bool wasLoading = _context?.Loading == true;
        bool changedCredential = _context?.SessionStamp != context?.SessionStamp;
        _context = context;
        if (context is null)
        {
            _timer.Stop(); _restore = null; _anchor = new();
            _attempts = _stable = 0; _userInteracting = false;
            AnchorUnavailable = false;
            return;
        }
        if (changedView || incoming is not null)
            Restore(incoming ?? new NativeScrollAnchor());
        else if (changedRows && _restore is null)
            Restore(_anchor);
        else if ((wasLoading && !context.Loading) || changedCredential) Queue();
    }

    internal NativeScrollAnchor Capture()
    {
        // A new navigation must retain the intended location, not an
        // intermediate virtualization offset used to find the target row.
        if (_restore is not null) return _restore;
        return CaptureActual();
    }
    private NativeScrollAnchor CaptureActual()
    {
        if (_disposed || _context is null) return _anchor;
        var first = VisibleRows().FirstOrDefault(row => !row.Message.Id.StartsWith("pending:", StringComparison.Ordinal));
        return new(first?.Message.Id, first?.Message.Sequence, first?.Y ?? 0,
            _view.ScrollableHeight - _view.VerticalOffset < 24, Math.Max(0, _view.VerticalOffset));
    }

    internal void Restore(NativeScrollAnchor anchor)
    {
        if (_disposed || !anchor.IsValid) return;
        _restore = anchor; _anchor = anchor; _attempts = _stable = 0; AnchorUnavailable = false;
        Queue();
    }

    internal void RevealEnd() => Restore(new NativeScrollAnchor());
    internal void ObserveSoon() => Queue();

    private static string? Family(string stamp) => stamp.LastIndexOf(':') is var last && last > 0 ? stamp[..last] : null;
    private static bool SameView(NativeTimelineContext? left, NativeTimelineContext? right) =>
        left is not null && right is not null && left.UserId == right.UserId &&
        Family(left.SessionStamp) == Family(right.SessionStamp) && left.ChannelId == right.ChannelId &&
        left.ViewGeneration == right.ViewGeneration && left.Visit.Generation == right.Visit.Generation;
    internal void UserIntentForProbe()
    {
        if (Program.LiveUiProbeDirectory is null) throw new InvalidOperationException("Viewport intent injection is only available in the UI fixture.");
        UserIntent();
    }

    private sealed record Row(NativeMessage Message, FrameworkElement Element, double Y);
    private IReadOnlyList<Row> MaterializedRows()
    {
        if (_context is null) return [];
        var known = _context.Messages.ToDictionary(item => "live-message-" + item.Id);
        var rows = new List<Row>();
        void Walk(DependencyObject parent)
        {
            if (parent is UIElement { Visibility: not Visibility.Visible }) return;
            if (parent is FrameworkElement element && element.Visibility == Visibility.Visible && element.ActualHeight > 0 &&
                known.TryGetValue(AutomationProperties.GetAutomationId(element), out var message))
            {
                try { rows.Add(new(message, element, element.TransformToVisual(_view).TransformPoint(new Point()).Y)); }
                catch (ArgumentException) { /* A recycled row can leave the tree during layout. */ }
                return;
            }
            for (int index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++) Walk(VisualTreeHelper.GetChild(parent, index));
        }
        Walk(_view);
        return rows.OrderBy(row => row.Y).ToArray();
    }
    private IReadOnlyList<Row> VisibleRows() => MaterializedRows()
        .Where(row => row.Y < _view.ViewportHeight - 1 && row.Y + row.Element.ActualHeight > 1).ToArray();

    private void Settled(DispatcherQueueTimer timer, object args)
    {
        if (_disposed || !_active() || _context is not { Loading: false } context || _view.ViewportHeight <= 0) return;
        if (_userInteracting)
        {
            if (System.Diagnostics.Stopwatch.GetElapsedTime(_lastInteraction) < TimeSpan.FromMilliseconds(220)) { Queue(); return; }
            _userInteracting = false; _anchor = CaptureActual();
        }
        if (_restore is { } saved)
        {
            double target;
            bool hasTarget = saved.MessageId is not null || saved.Sequence is not null;
            bool Matches(NativeMessage message) => saved.MessageId is not null
                ? message.Id == saved.MessageId : saved.Sequence is { } sequence && message.Sequence == sequence;
            var row = hasTarget ? MaterializedRows().FirstOrDefault(item => Matches(item.Message)) : null;
            if (saved.AtEnd) target = _view.ScrollableHeight;
            else if (row is not null) target = _view.VerticalOffset + row.Y - saved.PixelOffset;
            else
            {
                int index = hasTarget ? context.Messages.ToList().FindIndex(Matches) : -1;
                if (index >= 0 && _attempts < 24 && Realize(index, context.Messages.Count))
                {
                    _attempts++; Queue(); return;
                }
                target = index >= 0 && _attempts < 10 && context.Messages.Count > 1
                    ? index / (double)context.Messages.Count * _view.ExtentHeight : saved.AbsoluteOffset;
            }
            target = Math.Clamp(target, 0, Math.Max(0, _view.ScrollableHeight));
            bool settled = Math.Abs(target - _view.VerticalOffset) < .75 && (saved.AtEnd || !hasTarget || row is not null);
            _stable = settled ? _stable + 1 : 0;
            _attempts++;
            if (_stable < 2 && _attempts < 24)
            {
                _adjusting = true;
                try { _view.ChangeView(null, target, null, true); }
                finally { _adjusting = false; }
                Queue(); return;
            }
            if (!saved.AtEnd && hasTarget && row is null && _attempts >= 24)
            {
                // Do not acknowledge a guessed location when virtualization
                // could not realize the requested row. User scrolling recovers.
                AnchorUnavailable = true; _restore = null;
                _observe(new(context, saved, [])); return;
            }
            _restore = null; Restorations++;
        }
        _anchor = CaptureActual();
        var visible = VisibleRows().Select(row => row.Message.Id).ToArray();
        _observe(new(context, _anchor, visible));
    }
    private bool Realize(int index, int count)
    {
        ItemsRepeater? Find(DependencyObject parent)
        {
            if (parent is UIElement { Visibility: not Visibility.Visible }) return null;
            if (parent is ItemsRepeater repeater && repeater.ItemsSourceView?.Count == count) return repeater;
            for (int child = 0; child < VisualTreeHelper.GetChildrenCount(parent); child++)
                if (Find(VisualTreeHelper.GetChild(parent, child)) is { } found) return found;
            return null;
        }
        var host = Find(_view);
        if (host is null) return false;
        _adjusting = true;
        try
        {
            // The documented ItemsRepeater restoration path realizes the
            // exact index before applying its saved relative pixel offset.
            var element = host.GetOrCreateElement(index);
            element.UpdateLayout();
            element.StartBringIntoView(new BringIntoViewOptions { AnimationDesired = false });
            return true;
        }
        catch (ArgumentException) { return false; }
        finally { _adjusting = false; }
    }

    private void Queue()
    {
        if (_disposed) return;
        _timer.Stop(); _timer.Start();
    }
    private void UserPointer(object sender, PointerRoutedEventArgs args) => UserIntent();
    private void UserKey(object sender, KeyRoutedEventArgs args) => UserIntent();
    private void UserIntent()
    {
        _restore = null; _stable = _attempts = 0;
        AnchorUnavailable = false;
        _userInteracting = true; _lastInteraction = System.Diagnostics.Stopwatch.GetTimestamp();
        _anchor = CaptureActual(); Queue();
    }
    private void ViewChanged(object? sender, ScrollViewerViewChangedEventArgs args)
    {
        if (_disposed || _adjusting) return;
        bool geometry = GeometryChanged();
        RememberGeometry();
        if (_userInteracting)
        {
            _anchor = CaptureActual();
            _lastInteraction = System.Diagnostics.Stopwatch.GetTimestamp();
            if (!args.IsIntermediate) _userInteracting = false;
            Queue(); return;
        }
        if (_restore is null)
        {
            if (geometry) _restore = _anchor;
            else _anchor = CaptureActual();
        }
        if (!args.IsIntermediate || geometry) Queue();
    }
    private void SizeChanged(object sender, SizeChangedEventArgs args)
    {
        if (_userInteracting) _anchor = CaptureActual();
        else if (_restore is null) _restore = _anchor;
        RememberGeometry(); Queue();
    }
    private void LayoutUpdated(object? sender, object args)
    {
        if (_disposed || _adjusting || !GeometryChanged()) return;
        if (_userInteracting) _anchor = CaptureActual();
        else if (_restore is null) _restore = _anchor;
        RememberGeometry(); Queue();
    }
    private bool GeometryChanged() => Math.Abs(_extent - _view.ExtentHeight) > .5 || Math.Abs(_width - _view.ViewportWidth) > .5 || Math.Abs(_height - _view.ViewportHeight) > .5;
    private void RememberGeometry() { _extent = _view.ExtentHeight; _width = _view.ViewportWidth; _height = _view.ViewportHeight; }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true; _timer.Stop(); _timer.Tick -= Settled;
        _view.ViewChanged -= ViewChanged; _view.SizeChanged -= SizeChanged; _view.LayoutUpdated -= LayoutUpdated;
        _view.RemoveHandler(UIElement.PointerPressedEvent, _pointer);
        _view.RemoveHandler(UIElement.PointerWheelChangedEvent, _pointer);
        _view.RemoveHandler(UIElement.KeyDownEvent, _key);
    }
}
