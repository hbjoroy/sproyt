#if WINDOWS
using Microsoft.UI.Xaml;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage;
#endif

namespace Sproyt.WindowsProbe;

internal sealed record NativeFileDropItem(string Path, bool IsFile = true);

// The captured target belongs to this drop, not to the view reached while the
// shell resolves its files. Authentication and membership remain authoritative
// in AddUploadFilesCountedAsync; this owner only fences delayed shell work and errors.
internal sealed class NativeFileDropIntake<TTarget>(Func<TTarget?> capture,
    Func<TTarget, bool> admitted, Func<TTarget, IReadOnlyList<string>, Task<int>> add,
    Action<TTarget, string?> report, int maximumFiles = 8) : IDisposable where TTarget : class
{
    private readonly CancellationTokenSource _lifetime = new();
    private bool _busy;
    private volatile bool _disposed;

    internal bool CanOffer(bool storageItems, bool copyAllowed)
    {
        if (_disposed || _busy || !storageItems || !copyAllowed) return false;
        try { return capture() is { } target && admitted(target); }
        catch { return false; }
    }

    internal async Task<bool> DropAsync(bool storageItems, bool copyAllowed,
        Func<CancellationToken, Task<IReadOnlyList<NativeFileDropItem>>> resolve)
    {
        if (_disposed || _busy) return false;
        // This runs synchronously, before invoking a resolver or awaiting it.
        var target = capture();
        if (target is null || !admitted(target)) return false;
        if (!storageItems || !copyAllowed)
        {
            Report(target, "Dra inn filer som kan kopierast. Tekst, lenkjer og flytting blir ikkje tekne imot her.");
            return false;
        }
        _busy = true;
        Report(target, null);
        try
        {
            var items = await resolve(_lifetime.Token).WaitAsync(_lifetime.Token);
            if (!Current(target)) return false;
            if (items.Count == 0 || items.Count > maximumFiles)
                return Reject(target, $"Dra inn mellom éi og {maximumFiles} filer om gongen.");
            if (items.Any(item => item is null || !item.IsFile))
                return Reject(target, "Mapper kan ikkje leggjast ved. Vel filene i mappa i staden.");
            if (items.Any(item => string.IsNullOrWhiteSpace(item.Path) || item.Path.Any(char.IsControl) || !Path.IsPathFullyQualified(item.Path)))
                return Reject(target, "Fila har ikkje ein lesbar lokal filsti. Bruk Legg ved og vel henne på nytt.");
            // Validate the whole set before submitting any file. The existing
            // private spool owns MIME/size/readability checks and durable IO.
            var paths = items.Select(item => item.Path).ToArray();
            if (!Current(target)) return false;
            int accepted = await add(target, paths);
            if (!Current(target)) return false;
            if (accepted < 0 || accepted > paths.Length)
                return Reject(target, "Opplastingskøen gav eit ukjent resultat. Kontroller køen før du prøver fleire filer.");
            if (accepted == 0)
                return Reject(target, "Ingen filer vart stadfesta i opplastingskøen. Kontroller køen og feilmeldinga før du vel filer på nytt.");
            if (accepted < paths.Length)
                Report(target, $"{accepted} av {paths.Length} filer vart stadfesta i opplastingskøen. Kontroller køen og feilmeldinga før du prøver igjen. Vel berre filer som ikkje alt finst i køen.");
            return true;
        }
        catch (OperationCanceledException) { return false; }
        catch (UnauthorizedAccessException)
        { return Reject(target, "Vi har ikkje lesetilgang til fila. Vel ei anna fil."); }
        catch
        { return Reject(target, "Filene kunne ikkje hentast. Prøv Legg ved og vel dei på nytt."); }
        finally { _busy = false; }
    }

    private bool Current(TTarget target) => !_disposed && admitted(target);
    private bool Reject(TTarget target, string message) { Report(target, message); return false; }
    private void Report(TTarget target, string? message)
    {
        if (!Current(target)) return;
        try { report(target, message); } catch { /* Closing presentation owns no transfer. */ }
    }
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true; _lifetime.Cancel();
        // An outstanding resolver may still observe its token after disposal.
        // Cancellation retires its result without disposing that token source.
    }
}

#if WINDOWS
internal sealed record NativeFileDropContext(ILiveApplication App, NativeMessageActionScope Scope,
    Action<NativeUploadTarget, string?> Report);

// Attach once to a retained composer host. The callback must read the host's
// latest fields; it must not capture one Render's channel, thread or account.
internal sealed class NativeFileDrop : IDisposable
{
    private readonly FrameworkElement _host;
    private readonly Func<NativeFileDropContext?> _current;
    private readonly bool _previousAllowDrop;
    private readonly DragEventHandler _over, _drop;
    private bool _disposed;
    private NativeFileDropIntake<NativeUploadTarget>? _active;

    private NativeFileDrop(FrameworkElement host, Func<NativeFileDropContext?> current)
    {
        _host = host; _current = current; _previousAllowDrop = host.AllowDrop;
        _over = OnDragOver; _drop = OnDrop;
        host.AllowDrop = true;
        host.AddHandler(UIElement.DragOverEvent, _over, true);
        host.AddHandler(UIElement.DropEvent, _drop, true);
        host.Unloaded += OnUnloaded;
    }
    internal static NativeFileDrop Attach(FrameworkElement host, Func<NativeFileDropContext?> current) => new(host, current);

    private NativeFileDropContext? Context()
    {
        if (_disposed || Program.CloseAdmissionClosed) return null;
        try { return _current(); } catch { return null; }
    }
    private static bool Admitted(NativeFileDropContext context, NativeUploadTarget target)
    {
        var auth = context.App.Authentication;
        return !Program.CloseAdmissionClosed && auth.Phase == NativeAuthPhase.Active && auth.UserId == target.UserId &&
            LiveApplication.SameSendFamily(target.FamilyStamp, auth.SessionStamp) && context.App.Chat.UserId == target.UserId &&
            context.App.Chat.Channels.Any(channel => channel.Id == target.ChannelId);
    }
    private void OnDragOver(object sender, DragEventArgs args)
    {
        // Text editing retains its native text/URL drop behavior. Only the
        // StorageItems family is owned by the attachment intake.
        if (!args.DataView.Contains(StandardDataFormats.StorageItems)) return;
        args.Handled = true;
        args.AcceptedOperation = DataPackageOperation.None;
        if (_active is not null || Context() is not { } context ||
            !args.DataView.Contains(StandardDataFormats.StorageItems) || (args.AllowedOperations & DataPackageOperation.Copy) == 0) return;
        var target = context.App.CaptureUploadTarget(context.Scope);
        if (target is null || !Admitted(context, target)) return;
        args.AcceptedOperation = DataPackageOperation.Copy;
        args.DragUIOverride.Caption = "Legg filene i dette utkastet";
        args.DragUIOverride.IsCaptionVisible = true;
    }
    private async void OnDrop(object sender, DragEventArgs args)
    {
        if (!args.DataView.Contains(StandardDataFormats.StorageItems)) return;
        var deferral = args.GetDeferral();
        try
        {
            args.Handled = true; args.AcceptedOperation = DataPackageOperation.None;
            bool accepted = await DropAsync(args.DataView.Contains(StandardDataFormats.StorageItems),
                (args.AllowedOperations & DataPackageOperation.Copy) != 0,
                cancellation => ResolveAsync(args.DataView, cancellation));
            args.AcceptedOperation = accepted ? DataPackageOperation.Copy : DataPackageOperation.None;
        }
        catch { /* Rejected shell work must not escape the native routed event. */ }
        finally { deferral.Complete(); }
    }
    private async Task<bool> DropAsync(bool storageItems, bool copyAllowed,
        Func<CancellationToken, Task<IReadOnlyList<NativeFileDropItem>>> resolve)
    {
        if (_active is not null || Context() is not { } context) return false;
        NativeUploadTarget? captured = null;
        using var intake = new NativeFileDropIntake<NativeUploadTarget>(
            () => captured = _disposed ? null : context.App.CaptureUploadTarget(context.Scope),
            target => !_disposed && Admitted(context, target), context.App.AddUploadFilesCountedAsync,
            context.Report, NativeUploadQueue.MaximumJobs);
        void Changed()
        {
            // Navigation and ordinary token rotation preserve the captured
            // composer. Logout, account/family change or lost membership stop
            // shell resolution immediately, before it can open more files.
            if (captured is { } target && !Admitted(context, target)) intake.Dispose();
        }
        context.App.Changed += Changed;
        _active = intake;
        try { return await intake.DropAsync(storageItems, copyAllowed, resolve); }
        finally
        {
            context.App.Changed -= Changed;
            if (ReferenceEquals(_active, intake)) _active = null;
        }
    }
    private static async Task<IReadOnlyList<NativeFileDropItem>> ResolveAsync(DataPackageView data, CancellationToken cancellation)
    {
        var items = await data.GetStorageItemsAsync().AsTask(cancellation);
        cancellation.ThrowIfCancellationRequested();
        if (items.Count == 0) return [];
        if (items.Count > NativeUploadQueue.MaximumJobs)
            // Count alone rejects this drop. Do not enumerate paths from an
            // unbounded external list merely to report the capacity error.
            return Enumerable.Repeat(new NativeFileDropItem(""), NativeUploadQueue.MaximumJobs + 1).ToArray();
        // Reject a mixed file/folder drop as a whole before any read or upload.
        if (items.Any(item => item is not StorageFile))
            return items.Select(item => new NativeFileDropItem(item.Path, item is StorageFile)).ToArray();
        if (items.Any(item => string.IsNullOrWhiteSpace(item.Path) || !Path.IsPathFullyQualified(item.Path)))
            return items.Select(item => new NativeFileDropItem(item.Path)).ToArray();
        foreach (var file in items.Cast<StorageFile>())
        {
            cancellation.ThrowIfCancellationRequested();
            // Open only a handle, without reading file bytes or allocating a
            // file-sized buffer. The spool rechecks the real file on staging.
            using var readable = await file.OpenReadAsync().AsTask(cancellation);
            cancellation.ThrowIfCancellationRequested();
        }
        return items.Select(item => new NativeFileDropItem(item.Path)).ToArray();
    }
    internal Task<bool> DropForProbeAsync(Func<Task<IReadOnlyList<NativeFileDropItem>>> resolver)
    {
        if (Program.LiveUiProbeDirectory is null) throw new InvalidOperationException("File-drop injection requires the synthetic UI fixture.");
        return DropAsync(true, true, _ => resolver());
    }
    private void OnUnloaded(object sender, RoutedEventArgs args) => Dispose();
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true; _active?.Dispose();
        _host.RemoveHandler(UIElement.DragOverEvent, _over);
        _host.RemoveHandler(UIElement.DropEvent, _drop);
        _host.Unloaded -= OnUnloaded;
        _host.AllowDrop = _previousAllowDrop;
    }
}
#endif
