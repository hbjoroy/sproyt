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
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using Launcher = Windows.System.Launcher;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

// Keep the virtualized row's message, viewer and presentation metadata in one
// immutable snapshot. Stable message IDs preserve native controls and anchors.
internal sealed class NativeMessagePresentation(NativeMessage message, bool raw, NativeAuthSnapshot viewer,
    SproytPalette palette, NativeMessageActionScope? actionScope, bool rejected, bool interactive,
    IReadOnlyList<NativeReactionChip> reactions, bool reactionsLoaded, string threadLabel, string actionState) : IEquatable<NativeMessagePresentation>
{
    internal NativeMessage Message { get; } = message;
    internal bool Raw { get; } = raw;
    internal NativeAuthSnapshot Viewer { get; } = viewer;
    internal SproytPalette Palette { get; } = palette;
    internal NativeMessageActionScope? ActionScope { get; } = actionScope;
    internal bool Rejected { get; } = rejected;
    internal bool Interactive { get; } = interactive;
    internal IReadOnlyList<NativeReactionChip> Reactions { get; } = reactions;
    internal bool ReactionsLoaded { get; } = reactionsLoaded;
    internal string ThreadLabel { get; } = threadLabel;
    private string ActionState { get; } = actionState;
    public bool Equals(NativeMessagePresentation? other) => other is not null && Message == other.Message && Raw == other.Raw &&
        Viewer == other.Viewer && Palette == other.Palette && ActionScope == other.ActionScope && Rejected == other.Rejected &&
        Interactive == other.Interactive && ReactionsLoaded == other.ReactionsLoaded && ThreadLabel == other.ThreadLabel && ActionState == other.ActionState &&
        Reactions.Count == other.Reactions.Count && Reactions.Zip(other.Reactions).All(pair => pair.First.Emoji == pair.Second.Emoji &&
            pair.First.Count == pair.Second.Count && pair.First.Mine == pair.Second.Mine && pair.First.Names.SequenceEqual(pair.Second.Names));
    public override bool Equals(object? value) => value is NativeMessagePresentation other && Equals(other);
    public override int GetHashCode() => HashCode.Combine(Message, Raw, Viewer, Palette, ActionScope, Rejected, Interactive, ActionState);
}

internal static class NativeMessageView
{
    internal static async Task<(BitmapImage Bitmap, double AspectRatio)> DecodeBitmapAsync(IRandomAccessStream stream, int maximumEdge, CancellationToken cancellation)
    {
        var decoder = await BitmapDecoder.CreateAsync(stream).AsTask(cancellation);
        uint width = decoder.OrientedPixelWidth, height = decoder.OrientedPixelHeight;
        if (width == 0 || height == 0) throw new InvalidDataException("Biletet manglar gyldige mål.");
        double scale = Math.Min(1, maximumEdge / (double)Math.Max(width, height));
        // Both decode dimensions must follow the source ratio. A square decode
        // size stretches the pixels before Image.Stretch can preserve anything.
        var bitmap = new BitmapImage
        {
            DecodePixelWidth = Math.Max(1, (int)Math.Round(width * scale)),
            DecodePixelHeight = Math.Max(1, (int)Math.Round(height * scale))
        };
        stream.Seek(0);
        await bitmap.SetSourceAsync(stream).AsTask(cancellation);
        return (bitmap, width / (double)height);
    }
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
    internal static Element Render(ILiveApplication app, NativeMessage message, SproytPalette palette, NativeAuthSnapshot viewer, NativeRawMessageState rawState)
    {
        // Match the direct virtualized-row pattern used for message actions.
        // A nested component's retained render can hide an external source
        // choice even when the row factory itself has been invalidated.
        bool raw = rawState.IsRaw(message.Id);
        return VStack(4,
            raw ? TextBlock(message.Body).FontFamily("Cascadia Mono").FontSize(13).TextWrapping().IsTextSelectionEnabled()
                .Foreground(palette.Ink).AutomationId("native-message-raw-" + message.Id)
                : RenderParts(app, message, palette, viewer),
            Button(raw ? "Vis formatert" : "Vis råtekst", () =>
            {
                if (app.Authentication.UserId == viewer.UserId && app.Authentication.SessionStamp == viewer.SessionStamp) rawState.Toggle(message.Id);
            }).MinHeight(32).Padding(4, 2).AutomationName(raw ? "Vis formatert melding" : "Vis den opphavlege meldingsteksten")
                .AutomationId("native-message-raw-toggle-" + message.Id).HAlign(HorizontalAlignment.Left)
        ).WithKey("body:" + message.Id);
    }

    internal static Element RenderParts(ILiveApplication app, NativeMessage message, SproytPalette palette, NativeAuthSnapshot viewer)
    {
        var parts = NativeMessageContent.ParseRichText(message.Body, NativeRichText.LiteralRanges(message.Body));
        var scope = ScopeFor(app.Media, viewer);
        return VStack(8, parts.Select<NativeContentPart, Element>((part, index) => part.Kind switch
        {
            "text" => NativeRichTextView.Render(part.Text, palette, async uri =>
                { if (app.Authentication.UserId == viewer.UserId && app.Authentication.SessionStamp == viewer.SessionStamp) await Launcher.LaunchUriAsync(uri); }),
            "media" when part.ContentType!.StartsWith("image/", StringComparison.Ordinal) || part.ContentType.StartsWith("video/", StringComparison.Ordinal)
                => Component<NativeMediaComponent, NativeMediaProps>(new(app.Media, scope, part, MessageLink(message), palette))
                    .WithKey($"{message.Id}:{index}:{scope?.FamilyStamp}:{part.MediaId}:{part.ContentType}:{part.Text}"),
            "media" => Component<NativeAttachmentComponent, NativeAttachmentProps>(new(app.Media, scope, part, palette))
                .WithKey($"attachment:{message.Id}:{index}:{scope?.FamilyStamp}:{part.MediaId}"),
            _ => VStack(4, TextBlock(part.Text).FontSize(16).SemiBold().TextWrapping().Foreground(palette.Ink),
                Button(part.Kind == "media" ? "Opne vedlegget i Sprøyt" : "Opne i Sprøyt", async () => await Launcher.LaunchUriAsync(MessageLink(message)))
                    .AutomationName("Opne " + part.Text + " i Sprøyt")
                    .MinHeight(44).HAlign(HorizontalAlignment.Left)).Padding(12).Background(palette.Chrome).AutomationId("native-content-" + part.Kind),
        }).ToArray());
    }
}

// Repeater recycling must not undo the user's source choice while a row's
// height changes. This bounded UI state belongs to the conversation surface.
internal sealed class NativeRawMessageState
{
    private string? _user, _family;
    private readonly HashSet<string> _raw = [];
    internal event Action? Changed;
    internal void Bind(string? user, string? family)
    { if (_user == user && _family == family) return; _user = user; _family = family; _raw.Clear(); }
    internal bool IsRaw(string id) => _raw.Contains(id);
    internal void Toggle(string id)
    {
        if (!_raw.Remove(id)) { if (_raw.Count == 128) _raw.Remove(_raw.First()); _raw.Add(id); }
        Changed?.Invoke();
    }
}

internal sealed record NativeAttachmentProps(NativeMediaLibrary? Library, NativeMediaScope? Scope, NativeContentPart Part, SproytPalette Palette);
// Shared by documents and original image/video saves. The picker is explicit,
// and account/family admission is rechecked after every asynchronous boundary.
internal static class NativeMediaSave
{
    internal static Func<string, string, CancellationToken, Task<string?>>? PickDestinationForProbe { get; set; }
    internal static Task<bool>? LastSaveForProbe { get; private set; }
    internal static string Extension(string contentType) => contentType switch
    {
        "image/jpeg" => ".jpg", "image/png" => ".png", "image/webp" => ".webp", "image/gif" => ".gif",
        "image/heic" => ".heic", "image/heif" => ".heif", "image/avif" => ".avif",
        "video/mp4" => ".mp4", "video/quicktime" => ".mov", "video/webm" => ".webm", "video/x-matroska" => ".mkv",
        "application/pdf" => ".pdf", "application/zip" => ".zip", "text/plain" => ".txt", "text/csv" => ".csv",
        _ => throw new IOException("Denne vedleggstypen kan ikkje lagrast her.")
    };
    internal static Task<bool> SaveAsync(NativeMediaFile file, string name, NativeMediaLibrary library, NativeMediaScope scope, Func<bool> current, CancellationToken cancellation)
    {
        var task = SaveCoreAsync(file, name, library, scope, current, cancellation);
        if (Program.LiveUiProbeDirectory is not null) LastSaveForProbe = task;
        return task;
    }
    private static async Task<bool> SaveCoreAsync(NativeMediaFile file, string name, NativeMediaLibrary library, NativeMediaScope scope, Func<bool> current, CancellationToken cancellation)
    {
        if (!current() || Program.MainWindowId is not { } window) throw new OperationCanceledException();
        string extension = Extension(file.ContentType);
        string suggested = Path.GetFileNameWithoutExtension(NativeUploadDiskStore.SafeFileName(name));
        if (string.IsNullOrWhiteSpace(suggested)) suggested = "Vedlegg";
        string? chosenPath;
        if (Program.LiveUiProbeDirectory is not null && PickDestinationForProbe is { } injected)
            chosenPath = await injected(suggested + extension, file.ContentType, cancellation).WaitAsync(cancellation);
        else
        {
            var picker = new Microsoft.Windows.Storage.Pickers.FileSavePicker(window)
            { SuggestedFileName = suggested, DefaultFileExtension = extension };
            picker.FileTypeChoices.Add("Vedlegg", new List<string> { extension });
            chosenPath = (await picker.PickSaveFileAsync().AsTask(cancellation))?.Path;
        }
        cancellation.ThrowIfCancellationRequested();
        if (!current()) throw new OperationCanceledException();
        if (chosenPath is null) return false;
        if (!string.Equals(Path.GetExtension(chosenPath), extension, StringComparison.OrdinalIgnoreCase))
            throw new IOException("Vel filtypen " + extension + " for dette vedlegget.");
        string destination = Path.GetFullPath(chosenPath);
        string? staged = Path.Combine(Path.GetDirectoryName(destination)!, ".sproyt-download-" + Guid.NewGuid().ToString("N") + ".tmp");
        try
        {
            await using (var input = new FileStream(file.Path, FileMode.Open, FileAccess.Read, FileShare.Read | FileShare.Delete, 65536, FileOptions.Asynchronous))
            await using (var output = new FileStream(staged, FileMode.CreateNew, FileAccess.Write, FileShare.None, 65536, FileOptions.Asynchronous | FileOptions.WriteThrough))
            { await input.CopyToAsync(output, 65536, cancellation); await output.FlushAsync(cancellation); output.Flush(true); }
            cancellation.ThrowIfCancellationRequested();
            if (!current()) throw new OperationCanceledException();
            library.PublishSavedFile(scope, staged, destination); staged = null; return true;
        }
        finally
        {
            if (staged is not null) { try { File.Delete(staged); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
        }
    }
}
internal sealed class NativeAttachmentComponent : Component<NativeAttachmentProps>
{
    private readonly CancellationTokenSource _life = new();
    private CancellationTokenSource? _download;
    public NativeAttachmentComponent() { }
    public override Element Render()
    {
        var (status, setStatus) = UseState("", threadSafe: true);
        var (busy, setBusy) = UseState(false, threadSafe: true);
        UseEffect(() =>
        {
            void Invalidated() { if (Props.Library is not null && Props.Scope is not null && Props.Library.Current(Props.Scope)) return; try { _download?.Cancel(); } catch (ObjectDisposedException) { } }
            if (Props.Library is not null) Props.Library.Invalidated += Invalidated;
            return () => { if (Props.Library is not null) Props.Library.Invalidated -= Invalidated; _life.Cancel(); _download?.Cancel(); };
        }, Props.Scope?.FamilyStamp + ":" + Props.Part.MediaId);
        var captured = Props;
        bool Current() => !_life.IsCancellationRequested && captured.Library is not null && captured.Scope is not null &&
            ReferenceEquals(Props.Library, captured.Library) && Props.Scope == captured.Scope && captured.Library.Current(captured.Scope);
        async void Download()
        {
            if (busy || !Current()) return;
            setBusy(true); setStatus("Hentar vedlegget …");
            using var cancellation = _download = CancellationTokenSource.CreateLinkedTokenSource(_life.Token);
            try
            {
                using var file = await captured.Library!.OriginalAsync(captured.Part.MediaId!, captured.Scope!, null, cancellation.Token);
                if (file.ContentType != captured.Part.ContentType || !Current()) throw new OperationCanceledException();
                bool saved = await NativeMediaSave.SaveAsync(file, captured.Part.Text, captured.Library, captured.Scope!, Current, cancellation.Token);
                if (Current()) setStatus(saved ? "Vedlegget er lagra." : "");
            }
            catch (OperationCanceledException) { setStatus(""); }
            catch { if (Current()) setStatus("Vedlegget kunne ikkje lagrast. Prøv igjen."); }
            finally
            {
                if (ReferenceEquals(_download, cancellation)) _download = null;
                setBusy(false);
            }
        }
        return VStack(4, TextBlock(Props.Part.Text).SemiBold().TextWrapping().Foreground(Props.Palette.Ink),
            Button(busy ? "Hentar …" : "Last ned vedlegget", Download).MinHeight(44).IsEnabled(!busy && Current())
                .AutomationName("Last ned " + Props.Part.Text).AutomationId("native-attachment-download-" + Props.Part.MediaId),
            TextBlock(status).TextWrapping().Foreground(Props.Palette.Muted).IsVisible(status.Length != 0)).Padding(8).Background(Props.Palette.Chrome);
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
            _presenter = new(Props, host);
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
    private readonly Border _host;
    private readonly DispatcherQueue _dispatcher;
    private readonly CancellationTokenSource _life = new();
    // Reserve the learned aspect ratio even after releasing an offscreen bitmap.
    private double _aspectRatio = 1;
    private readonly Microsoft.UI.Xaml.Controls.Image _image = new() { Width = 320, Height = 320, Stretch = Stretch.Uniform };
    private readonly Microsoft.UI.Xaml.Controls.TextBlock _status;
    private readonly Microsoft.UI.Xaml.Controls.Button _open;
    private readonly Microsoft.UI.Xaml.Controls.Button _load;
    private readonly Microsoft.UI.Xaml.Controls.Button _save;
    private readonly HyperlinkButton _web;
    private NativePreviewLease? _previewLease;
    private CancellationTokenSource? _previewCancellation;
    private ContentDialog? _dialog;
    private MediaPlayer? _player;
    private volatile bool _disposed;
    private bool _opening, _saving, _loading, _visible, _waitingForCapacity;
    internal static ContentDialog? ActiveDialogForProbe { get; private set; }
    internal StackPanel Root { get; }
    internal NativeMediaPresenter(NativeMediaProps props, Border host)
    {
        _props = props; _host = host; _dispatcher = host.DispatcherQueue;
        AutomationProperties.SetAutomationId(_image, "native-image-" + props.Part.MediaId);
        Root = new StackPanel { Spacing = 8, MaxWidth = 320, HorizontalAlignment = HorizontalAlignment.Left };
        AutomationProperties.SetAutomationId(Root, "native-media-" + props.Part.MediaId);
        bool video = props.Part.ContentType!.StartsWith("video/", StringComparison.Ordinal);
        _status = new() { FontSize = 13, Foreground = props.Palette.Muted, TextWrapping = TextWrapping.Wrap };
        _open = new() { MinHeight = 44, MinWidth = 44, HorizontalAlignment = HorizontalAlignment.Left, Content = video ? "▷ Spel av" : new SymbolIcon(Symbol.FullScreen) };
        AutomationProperties.SetName(_open, video ? "Spel av " + props.Part.Text : "Vis " + props.Part.Text + " i større vising");
        AutomationProperties.SetAutomationId(_open, "native-media-open-" + props.Part.MediaId);
        ToolTipService.SetToolTip(_open, video ? "Spel av videoen" : "Vis biletet i større vising");
        _open.Click += async (_, _) => await OpenAsync();
        _save = new() { Content = "Lagre original", MinHeight = 44, MinWidth = 44, HorizontalAlignment = HorizontalAlignment.Left };
        AutomationProperties.SetName(_save, "Lagre originalen til " + props.Part.Text);
        AutomationProperties.SetAutomationId(_save, "native-media-save-" + props.Part.MediaId);
        _save.Click += async (_, _) => await SaveAsync();
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
        Root.Children.Add(caption); Root.Children.Add(_save); Root.Children.Add(_status); Root.Children.Add(_load);
        _status.Visibility = Visibility.Collapsed;
        _web = new() { Content = "Opne meldinga i Sprøyt", NavigateUri = props.MessageLink, MinHeight = 44, Padding = new Thickness(0), HorizontalAlignment = HorizontalAlignment.Left, Visibility = Visibility.Collapsed };
        Root.Children.Add(_web);
        if (props.Library is not null)
        {
            props.Library.Invalidated += Invalidated;
            if (!video) props.Library.PreviewSlotAvailable += PreviewSlotAvailable;
        }
        if (!video)
        {
            host.SizeChanged += HostSizeChanged;
            SizePreview();
            Root.EffectiveViewportChanged += ViewportChanged;
            Root.Unloaded += Unloaded;
        }
    }
    private bool Current => !_disposed && _props.Library is not null && _props.Scope is not null && _props.Library.Current(_props.Scope);
    private void Status(string text) { _status.Text = text; _status.Visibility = text.Length == 0 ? Visibility.Collapsed : Visibility.Visible; }
    private bool Post(Action action) => !_disposed && _dispatcher.TryEnqueue(() => { if (!_disposed) action(); });
    private void ReleasePreview() { _image.Source = null; Interlocked.Exchange(ref _previewLease, null)?.Dispose(); }
    private void HostSizeChanged(object sender, SizeChangedEventArgs args) => SizePreview();
    private void SizePreview()
    {
        if (_host.ActualWidth <= 0) return;
        _image.Width = Math.Min(320, _host.ActualWidth);
        _image.Height = _image.Width / _aspectRatio;
    }
    private void ViewportChanged(FrameworkElement sender, EffectiveViewportChangedEventArgs args)
    {
        var viewport = args.EffectiveViewport;
        bool visible = viewport.Width > 0 && viewport.Height > 0 && viewport.X < Root.ActualWidth &&
            viewport.Y < Root.ActualHeight && viewport.Right > 0 && viewport.Bottom > 0;
        if (_visible == visible) return;
        _visible = visible;
        if (visible) _ = PreviewAsync();
        else StopPreview();
    }
    private void Unloaded(object sender, RoutedEventArgs args) { _visible = false; StopPreview(); }
    private void StopPreview()
    {
        var attempt = _previewCancellation;
        _previewCancellation = null;
        attempt?.Cancel();
        _loading = false; _waitingForCapacity = false;
        ReleasePreview();
    }
    private bool PreviewCurrent(CancellationTokenSource attempt) => Current && _visible &&
        ReferenceEquals(_previewCancellation, attempt) && !attempt.IsCancellationRequested;
    private void PreviewSlotAvailable() => Post(() =>
    {
        if (_visible && _waitingForCapacity && !_loading) _ = PreviewAsync();
    });
    private void FinishPreview(CancellationTokenSource attempt)
    {
        if (ReferenceEquals(_previewCancellation, attempt))
        {
            _previewCancellation = null; _loading = false; _load.IsEnabled = Current;
            // A release can race the capacity error before its UI callback runs.
            if (_visible && _waitingForCapacity && _props.Library!.HasPreviewCapacity) _ = PreviewAsync();
        }
        attempt.Dispose();
    }
    private async Task PreviewAsync()
    {
        if (!Current || !_visible || _loading || _image.Source is not null) return;
        var attempt = CancellationTokenSource.CreateLinkedTokenSource(_life.Token);
        _previewCancellation = attempt;
        _waitingForCapacity = false;
        _loading = true; _load.IsEnabled = false;
        Status("Lastar førehandsvising …");
        bool posted = false;
        NativePreviewLease? downloaded = null;
        try
        {
            var lease = await _props.Library!.PreviewAsync(_props.Part.MediaId!, _props.Scope!, attempt.Token);
            downloaded = lease;
            // Resource-owning callbacks must run even if unmounted before dispatch.
            posted = _dispatcher.TryEnqueue(async () =>
            {
                bool retained = false;
                try
                {
                    if (!PreviewCurrent(attempt) || !lease.BeginDecode()) return;
                    using var data = new MemoryStream(lease.Bytes); using var stream = data.AsRandomAccessStream();
                    var decoded = await NativeMessageView.DecodeBitmapAsync(stream, 720, attempt.Token);
                    if (PreviewCurrent(attempt))
                    {
                        _previewLease = lease; retained = true;
                        _aspectRatio = decoded.AspectRatio; SizePreview(); _image.Source = decoded.Bitmap;
                        Status(""); _load.Visibility = Visibility.Collapsed; _web.Visibility = Visibility.Collapsed;
                    }
                }
                catch (OperationCanceledException) { }
                catch { if (PreviewCurrent(attempt)) { Status("Førehandsvisinga kunne ikkje visast. Bruk Opne."); _load.Visibility = Visibility.Visible; _web.Visibility = Visibility.Visible; } }
                finally { lease.Decoded(); if (!retained) lease.Dispose(); FinishPreview(attempt); }
            });
            if (posted) downloaded = null;
        }
        catch (OperationCanceledException) { }
        catch (NativePreviewCapacityException)
        {
            Post(() => { if (PreviewCurrent(attempt)) { _waitingForCapacity = true; Status(""); } });
        }
        catch (Exception error) { Post(() => { if (PreviewCurrent(attempt)) { Status(error is InvalidOperationException ? error.Message : "Fekk ikkje henta biletet. Bruk Opne for å prøve igjen."); _load.Visibility = Visibility.Visible; _web.Visibility = Visibility.Visible; } }); }
        finally
        {
            downloaded?.Dispose();
            if (!posted && !_dispatcher.TryEnqueue(() => FinishPreview(attempt))) attempt.Dispose();
        }
    }
    private async Task OpenAsync()
    {
        if (_opening || _saving || !Current) return;
        _opening = true; _open.IsEnabled = false; _save.IsEnabled = false; Status("Hentar vedlegget …");
        try
        {
            var progress = new Progress<long>(bytes => Post(() => { if (Current) Status($"Hentar vedlegget … {bytes / (1024 * 1024)} MiB"); }));
            using var file = await _props.Library!.OriginalAsync(_props.Part.MediaId!, _props.Scope!, progress, _life.Token);
            if (!Current) return;
            var stored = await StorageFile.GetFileFromPathAsync(file.Path);
            using var stream = await stored.OpenReadAsync();
            if (!Current) return;
            FrameworkElement content;
            MediaSource? source = null;
            NativeImageViewer? viewer = null;
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
                    var decoded = await NativeImageViewer.DecodeAsync(stream, _life.Token);
                    if (!Current) return;
                    viewer = new(decoded, _props.Part.Text, file.Path, _props.Palette, () => Current,
                        token => NativeMediaSave.SaveAsync(file, _props.Part.Text, _props.Library!, _props.Scope!, () => Current && viewer is { IsDisposed: false }, token),
                        () => _dialog?.Hide(), _life.Token);
                    content = viewer.Root;
                }
                if (!Current) return;
                _dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = _props.Part.Text, Content = content,
                    CloseButtonText = viewer is null ? "Lukk" : "", FullSizeDesired = true,
                    RequestedTheme = _props.Palette == SproytPalette.Dark ? ElementTheme.Dark : ElementTheme.Light };
                if (viewer is not null)
                { _dialog.Resources["ContentDialogMaxWidth"] = 1100d; _dialog.Resources["ContentDialogMaxHeight"] = 950d; }
                if (Program.LiveUiProbeDirectory is not null) ActiveDialogForProbe = _dialog;
                Status("");
                // Playback, dialog and private original lease have the same owner.
#pragma warning disable REACTOR_DIALOG_001
                await _dialog.ShowAsync();
#pragma warning restore REACTOR_DIALOG_001
            }
            finally
            {
                viewer?.Dispose();
                if (ReferenceEquals(ActiveDialogForProbe, _dialog)) ActiveDialogForProbe = null;
                _player?.Pause(); if (_player is not null) _player.Source = null; _player?.Dispose(); _player = null; source?.Dispose(); _dialog = null;
                if (viewer is not null) await viewer.WaitForSaveAsync();
            }
        }
        catch (OperationCanceledException) { }
        catch { if (Current) { Status("Vedlegget kunne ikkje opnast. Du kan opne meldinga i Sprøyt."); _web.Visibility = Visibility.Visible; } }
        finally { _opening = false; if (Current) { _open.IsEnabled = true; _save.IsEnabled = true; } }
    }
    private async Task SaveAsync()
    {
        if (_saving || _opening || !Current) return;
        _saving = true; _save.IsEnabled = false; _open.IsEnabled = false; Status("Hentar originalen …");
        try
        {
            using var file = await _props.Library!.OriginalAsync(_props.Part.MediaId!, _props.Scope!, null, _life.Token);
            if (!Current) return;
            bool saved = await NativeMediaSave.SaveAsync(file, _props.Part.Text, _props.Library!, _props.Scope!, () => Current, _life.Token);
            if (Current) Status(saved ? "Originalen er lagra." : "");
        }
        catch (OperationCanceledException) { }
        catch { if (Current) Status("Originalen kunne ikkje lagrast. Prøv igjen."); }
        finally { _saving = false; if (Current) { _save.IsEnabled = true; _open.IsEnabled = true; } }
    }
    private void Invalidated() => Post(() =>
    {
        if (Current) return;
        _life.Cancel(); StopPreview(); _player?.Pause(); _dialog?.Hide();
        Status("Logg inn på nytt for å hente vedlegget."); _open.IsEnabled = false; _save.IsEnabled = false; _load.IsEnabled = false;
    });
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true; _life.Cancel(); StopPreview(); _player?.Pause(); _dialog?.Hide();
        Root.EffectiveViewportChanged -= ViewportChanged; Root.Unloaded -= Unloaded;
        _host.SizeChanged -= HostSizeChanged;
        if (_props.Library is not null) { _props.Library.Invalidated -= Invalidated; _props.Library.PreviewSlotAvailable -= PreviewSlotAvailable; }
    }
}
