using Microsoft.UI.Dispatching;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Media.Core;
using Windows.Media.Playback;
using Windows.Storage;
using Launcher = Windows.System.Launcher;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal static class NativeMessageView
{
    internal static Uri MessageLink(NativeMessage message)
    {
        if (!Guid.TryParseExact(message.ChannelId, "D", out _) || !Guid.TryParseExact(message.Id, "D", out _))
            return new("https://sproyt-canary.bjoroy.me/");
        return new($"https://sproyt-canary.bjoroy.me/?channel={message.ChannelId}&message={message.Id}&sequence={message.Sequence}");
    }
    internal static NativeMediaScope? ScopeFor(NativeMediaLibrary? library, NativeAuthSnapshot viewer)
    {
        var scope = library?.TryCapture();
        return scope is not null && scope.UserId == viewer.UserId &&
            viewer.SessionStamp?.StartsWith(scope.FamilyStamp + ":", StringComparison.Ordinal) == true ? scope : null;
    }
    internal static Element Render(ILiveApplication app, NativeMessage message, SproytPalette palette, NativeAuthSnapshot viewer)
    {
        var parts = NativeMessageContent.Parse(message.Body);
        var scope = ScopeFor(app.Media, viewer);
        return VStack(8, parts.Select<NativeContentPart, Element>((part, index) => part.Kind switch
        {
            "text" => TextBlock(part.Text).FontSize(16).TextWrapping().IsTextSelectionEnabled().Foreground(palette.Ink),
            "media" when part.ContentType!.StartsWith("image/", StringComparison.Ordinal) || part.ContentType.StartsWith("video/", StringComparison.Ordinal)
                => Component<NativeMediaComponent, NativeMediaProps>(new(app.Media, scope, part, MessageLink(message), palette))
                    .WithKey($"{message.Id}:{index}:{scope?.FamilyStamp}:{part.MediaId}:{part.ContentType}:{part.Text}"),
            _ => VStack(4, TextBlock(part.Text).FontSize(16).SemiBold().TextWrapping().Foreground(palette.Ink),
                Button(part.Kind == "media" ? "Opne vedlegget i Sprøyt" : "Opne i Sprøyt", async () => await Launcher.LaunchUriAsync(MessageLink(message)))
                    .AutomationName("Opne " + part.Text + " i Sprøyt")
                    .MinHeight(44).HAlign(HorizontalAlignment.Left)).Padding(12).Background(palette.Chrome).AutomationId("native-content-" + part.Kind),
        }).ToArray());
    }
}

internal sealed record NativeMediaProps(NativeMediaLibrary? Library, NativeMediaScope? Scope, NativeContentPart Part, Uri MessageLink, SproytPalette Palette);
internal sealed class NativeMediaComponent : Component<NativeMediaProps>
{
    private NativeMediaPresenter? _presenter;
    public NativeMediaComponent() { }
    public override Element Render() => Border(Empty())
        .OnMount(element =>
        {
            if (element is not Border host) return;
            _presenter = new(Props, host.DispatcherQueue);
            host.Child = _presenter.Root;
        })
        .OnUnmount(_ => { _presenter?.Dispose(); _presenter = null; })
        .WithKey(Props.Palette == SproytPalette.Dark ? "dark" : "light");
}

// Native Image/MediaPlayer controls. Content is data; no HTML, arbitrary media
// URLs, credentials or JavaScript are passed to the view.
internal sealed class NativeMediaPresenter : IDisposable
{
    private readonly NativeMediaProps _props;
    private readonly DispatcherQueue _dispatcher;
    private readonly CancellationTokenSource _life = new();
    private readonly Microsoft.UI.Xaml.Controls.Image _image = new() { MaxHeight = 360, MaxWidth = 640, Stretch = Stretch.Uniform };
    private readonly Microsoft.UI.Xaml.Controls.TextBlock _status;
    private readonly Microsoft.UI.Xaml.Controls.Button _open;
    private readonly Microsoft.UI.Xaml.Controls.Button _load;
    private readonly HyperlinkButton _web;
    private NativePreviewLease? _previewLease;
    private ContentDialog? _dialog;
    private MediaPlayer? _player;
    private bool _disposed, _opening, _loading;
    internal StackPanel Root { get; }
    internal NativeMediaPresenter(NativeMediaProps props, DispatcherQueue dispatcher)
    {
        _props = props; _dispatcher = dispatcher;
        AutomationProperties.SetAutomationId(_image, "native-image-" + props.Part.MediaId);
        Root = new StackPanel { Spacing = 8, MaxWidth = 640, HorizontalAlignment = HorizontalAlignment.Left };
        AutomationProperties.SetAutomationId(Root, "native-media-" + props.Part.MediaId);
        bool video = props.Part.ContentType!.StartsWith("video/", StringComparison.Ordinal);
        _status = new() { FontSize = 13, Foreground = props.Palette.Muted, TextWrapping = TextWrapping.Wrap };
        _open = new() { MinHeight = 44, MinWidth = 44, HorizontalAlignment = HorizontalAlignment.Left, Content = video ? "▷ Spel av" : new SymbolIcon(Symbol.FullScreen) };
        AutomationProperties.SetName(_open, video ? "Spel av " + props.Part.Text : "Vis " + props.Part.Text + " i større vising");
        ToolTipService.SetToolTip(_open, video ? "Spel av videoen" : "Vis biletet i større vising");
        _open.Click += async (_, _) => await OpenAsync();
        _load = new() { Content = "Last førehandsvising", MinHeight = 44, Visibility = Visibility.Collapsed, HorizontalAlignment = HorizontalAlignment.Left };
        _load.Click += async (_, _) => await PreviewAsync();
        if (!video)
        {
            var thumbnail = new Microsoft.UI.Xaml.Controls.Button { Content = _image, Padding = new Thickness(0), BorderThickness = new Thickness(0), Background = props.Palette.Paper, HorizontalAlignment = HorizontalAlignment.Left };
            AutomationProperties.SetName(thumbnail, "Vis " + props.Part.Text);
            thumbnail.Click += async (_, _) => await OpenAsync();
            Root.Children.Add(thumbnail);
        }
        var filename = new Microsoft.UI.Xaml.Controls.TextBlock { Text = props.Part.Text, FontSize = 13, Foreground = props.Palette.Muted, TextWrapping = TextWrapping.Wrap };
        AutomationProperties.SetAutomationId(filename, "native-media-name-" + props.Part.MediaId);
        filename.TextTrimming = TextTrimming.CharacterEllipsis; filename.MaxLines = 1;
        ToolTipService.SetToolTip(filename, props.Part.Text);
        var caption = new Microsoft.UI.Xaml.Controls.Grid { ColumnSpacing = 8 };
        caption.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        caption.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        filename.VerticalAlignment = VerticalAlignment.Center;
        Microsoft.UI.Xaml.Controls.Grid.SetColumn(_open, 1);
        caption.Children.Add(filename); caption.Children.Add(_open);
        Root.Children.Add(caption); Root.Children.Add(_status); Root.Children.Add(_load);
        _status.Visibility = Visibility.Collapsed;
        _web = new() { Content = "Opne meldinga i Sprøyt", NavigateUri = props.MessageLink, MinHeight = 44, Padding = new Thickness(0), HorizontalAlignment = HorizontalAlignment.Left, Visibility = Visibility.Collapsed };
        Root.Children.Add(_web);
        if (props.Library is not null) props.Library.Invalidated += Invalidated;
        if (!video) _ = PreviewAsync();
    }
    private bool Current => !_disposed && _props.Library is not null && _props.Scope is not null && _props.Library.Current(_props.Scope);
    private void Status(string text) { _status.Text = text; _status.Visibility = text.Length == 0 ? Visibility.Collapsed : Visibility.Visible; }
    private bool Post(Action action) => !_disposed && _dispatcher.TryEnqueue(() => { if (!_disposed) action(); });
    private void ReleasePreview() { _image.Source = null; Interlocked.Exchange(ref _previewLease, null)?.Dispose(); }
    private async Task PreviewAsync()
    {
        if (!Current) { Status("Logg inn for å hente vedlegget."); return; }
        if (_loading) return;
        _loading = true; _load.IsEnabled = false;
        Status("Lastar førehandsvising …");
        bool posted = false;
        try
        {
            var lease = await _props.Library!.PreviewAsync(_props.Part.MediaId!, _props.Scope!, _life.Token);
            if (!Current) { lease.Dispose(); return; }
            Interlocked.Exchange(ref _previewLease, lease)?.Dispose();
            posted = Post(async () =>
            {
                try
                {
                    if (!Current || !lease.BeginDecode()) { ReleasePreview(); return; }
                    using var data = new MemoryStream(lease.Bytes); using var stream = data.AsRandomAccessStream();
                    var bitmap = new BitmapImage { DecodePixelWidth = 720, DecodePixelHeight = 720 };
                    await bitmap.SetSourceAsync(stream).AsTask(_life.Token);
                    if (Current) { _image.Source = bitmap; Status(""); _load.Visibility = Visibility.Collapsed; }
                    else ReleasePreview();
                }
                catch { ReleasePreview(); if (!_disposed) { Status("Førehandsvisinga kunne ikkje visast. Bruk Opne."); _web.Visibility = Visibility.Visible; } }
                finally { lease.Decoded(); if (Current) { _loading = false; _load.IsEnabled = true; } }
            });
            if (!posted) Interlocked.Exchange(ref _previewLease, null)?.Dispose();
        }
        catch (OperationCanceledException) { }
        catch (Exception error) { Post(() => { Status(error is InvalidOperationException ? error.Message : "Fekk ikkje henta biletet. Bruk Opne for å prøve igjen."); _load.Visibility = Visibility.Visible; _web.Visibility = Visibility.Visible; }); }
        finally { if (!posted) Post(() => { _loading = false; _load.IsEnabled = Current; }); }
    }
    private async Task OpenAsync()
    {
        if (_opening || !Current) return;
        _opening = true; _open.IsEnabled = false; Status("Hentar vedlegget …");
        try
        {
            var progress = new Progress<long>(bytes => Post(() => Status($"Hentar vedlegget … {bytes / (1024 * 1024)} MiB")));
            using var file = await _props.Library!.OriginalAsync(_props.Part.MediaId!, _props.Scope!, progress, _life.Token);
            if (!Current) return;
            var stored = await StorageFile.GetFileFromPathAsync(file.Path);
            using var stream = await stored.OpenReadAsync();
            if (!Current) return;
            FrameworkElement content;
            MediaSource? source = null;
            try
            {
                if (file.ContentType.StartsWith("video/", StringComparison.Ordinal))
                {
                    source = MediaSource.CreateFromStream(stream, file.ContentType);
                    _player = new MediaPlayer { Source = source, AutoPlay = true };
                    var video = new MediaPlayerElement { AreTransportControlsEnabled = true, MaxWidth = 900, MaxHeight = 600, MinHeight = 220 };
                    video.SetMediaPlayer(_player); content = video;
                }
                else
                {
                    var bitmap = new BitmapImage { DecodePixelWidth = 1600, DecodePixelHeight = 1600 };
                    await bitmap.SetSourceAsync(stream);
                    content = new ScrollViewer { Content = new Microsoft.UI.Xaml.Controls.Image { Source = bitmap, Stretch = Stretch.Uniform, MaxWidth = 900, MaxHeight = 650 }, ZoomMode = ZoomMode.Enabled, MinZoomFactor = 1, MaxZoomFactor = 5, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
                }
                if (!Current) return;
                _dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = _props.Part.Text, Content = content, CloseButtonText = "Lukk", FullSizeDesired = true };
                Status("");
                // The native presenter owns the dialog, playback and file lease;
                // logout/unmount hides it and the finally block releases all three.
#pragma warning disable REACTOR_DIALOG_001
                await _dialog.ShowAsync();
#pragma warning restore REACTOR_DIALOG_001
            }
            finally { _player?.Pause(); if (_player is not null) _player.Source = null; _player?.Dispose(); _player = null; source?.Dispose(); _dialog = null; }
        }
        catch (OperationCanceledException) { }
        catch { if (!_disposed) { Status("Vedlegget kunne ikkje opnast. Du kan opne meldinga i Sprøyt."); _web.Visibility = Visibility.Visible; } }
        finally { if (!_disposed) { _opening = false; _open.IsEnabled = true; } }
    }
    private void Invalidated() => Post(() => { _life.Cancel(); ReleasePreview(); _player?.Pause(); _dialog?.Hide(); Status("Logg inn på nytt for å hente vedlegget."); _open.IsEnabled = false; _load.IsEnabled = false; });
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true; _life.Cancel(); ReleasePreview(); _player?.Pause(); _dialog?.Hide();
        if (_props.Library is not null) _props.Library.Invalidated -= Invalidated;
    }
}
