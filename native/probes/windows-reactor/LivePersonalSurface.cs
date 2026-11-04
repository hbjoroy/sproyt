using Microsoft.UI.Dispatching;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed partial class LiveChatComponent
{
    internal static Func<NativePersonalSurfaceKind, Task>? OpenPersonalForProbe;
    internal static Action? ClosePersonalForProbe;
    internal static NativePersonalScope? PersonalScopeForProbe;
    private readonly NativePersonalSurfaceController _personalSurface = new();
    private ILiveApplication? _personalApp;
    private NativePeopleController? _people;
    private NativeProfileController? _profile;
    private NativeInboxController? _inbox;
    private NativePerson? _canonicalSelf;
    private Action? _personalInvalidate;
    private WeakReference<TextBox>? _conversationFocus;
    private long? _focusVisit;
    private NativePersonalScope? _loadedPersonalScope;
    private long _personalNavigationSerial;
    private sealed record PersonalReturn(NativePersonalSurfaceKind Kind, string UserId, string Stamp,
        NativeNavigationTarget Origin, long OriginGeneration, NativeNavigationTarget? Destination = null, long? DestinationGeneration = null);
    private PersonalReturn? _personalReturn;
    private bool PersonalOpen => _personalSurface.Kind is not null;

    private void PersonalChanged()
    {
        CacheCanonicalSelf();
        _personalInvalidate?.Invoke();
    }
    private void CacheCanonicalSelf()
    {
        if (_personalApp is not { } app || _personalSurface.Scope is not { } scope || !PersonalCurrent(app, scope)) return;
        var profile = _profile?.Capture();
        var people = _people?.Capture();
        var own = profile?.Scope == scope ? profile.Profile : people?.Scope == scope ? people.People.FirstOrDefault(person => person.Id == scope.UserId) : null;
        if (own?.Id == scope.UserId) _canonicalSelf = own;
    }
    private void RetirePersonalControllers()
    {
        if (_people is not null) { _people.Changed -= PersonalChanged; _people.Bind(null); }
        if (_profile is not null) { _profile.Changed -= PersonalChanged; _profile.Bind(null); }
        if (_inbox is not null) { _inbox.Changed -= PersonalChanged; _inbox.Bind(null); }
        _people = null; _profile = null; _inbox = null; _canonicalSelf = null;
    }
    private void SynchronizePersonal(ILiveApplication app, LiveSnapshot state, Action invalidate)
    {
        _personalInvalidate = invalidate;
        var before = _personalSurface.Scope;
        bool valid = state.ShowConversation && state.Authentication.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline;
        bool identityChanged = _personalSurface.Bind(valid ? state.Authentication.UserId : null, valid ? state.Authentication.SessionStamp : null);
        if (!ReferenceEquals(_personalApp, app) || identityChanged)
        {
            if (before is not null) _personalApp?.UncoverConversation(before);
            if (!ReferenceEquals(_personalApp, app)) _personalSurface.Close();
            RetirePersonalControllers(); _personalReturn = null; _conversationFocus = null;
            _personalApp = app;
            _personalNavigationSerial++;
        }
        if (_people is null)
        {
            _people = new(app.PersonalCommands); _profile = new(app.PersonalCommands); _inbox = new();
            _people.Changed += PersonalChanged; _profile.Changed += PersonalChanged; _inbox.Changed += PersonalChanged;
        }
        ApplyPersonalBindings(app, state, before);
        if (_personalReturn is { } frame && (state.Authentication.UserId != frame.UserId || state.Authentication.SessionStamp != frame.Stamp))
            _personalReturn = null;
        if (_personalSurface.Scope is { } scope && _loadedPersonalScope != scope)
        { _loadedPersonalScope = scope; _ = LoadPersonalAsync(scope); }
        if (Program.LiveUiProbeDirectory is not null)
        {
            OpenPersonalForProbe = kind => OpenPersonalAsync(app, kind);
            var captured = _personalSurface.Scope;
            ClosePersonalForProbe = () => { if (captured is not null) ClosePersonal(app, captured); };
            PersonalScopeForProbe = captured;
        }
    }
    private void ApplyPersonalBindings(ILiveApplication app, LiveSnapshot state, NativePersonalScope? previous)
    {
        var scope = _personalSurface.Scope;
        _rootActions.SetCovered(scope is not null); _threadActions.SetCovered(scope is not null);
        if (scope is not null) app.CoverConversation(scope);
        else if (previous is not null) app.UncoverConversation(previous);
        _people?.Bind(_personalSurface.Kind == NativePersonalSurfaceKind.People ? scope : null);
        _profile?.Bind(_personalSurface.Kind == NativePersonalSurfaceKind.Profile ? scope : null);
        bool inbox = _personalSurface.Kind is NativePersonalSurfaceKind.Unread or NativePersonalSurfaceKind.Mentions or NativePersonalSurfaceKind.Tasks;
        var known = state.Chat.Messages.Concat(state.Threads?.Thread is { } thread
            ? (thread.Root is { } root ? new[] { root } : Array.Empty<NativeMessage>()).Concat(thread.Replies) : []);
        _inbox?.Bind(inbox ? scope : null, state.Chat.Channels, known.ToArray(), !state.Chat.Loading);
        if (inbox && scope is not null) _inbox?.SelectTab(scope, InboxTab(_personalSurface.Kind!.Value));
    }
    private static NativeInboxTab InboxTab(NativePersonalSurfaceKind kind) => kind switch
    { NativePersonalSurfaceKind.Mentions => NativeInboxTab.Mentions, NativePersonalSurfaceKind.Tasks => NativeInboxTab.Tasks, _ => NativeInboxTab.Unread };
    private bool PersonalCurrent(ILiveApplication app, NativePersonalScope scope) => ReferenceEquals(app, _personalApp) &&
        _personalSurface.Current(scope) && !Program.CloseAdmissionClosed && app.Authentication.UserId == scope.UserId && app.Authentication.SessionStamp == scope.SessionStamp;
    private async Task LoadPersonalAsync(NativePersonalScope scope)
    {
        if (_personalApp is not { } app || !PersonalCurrent(app, scope)) return;
        try
        {
            if (_personalSurface.Kind == NativePersonalSurfaceKind.People) await _people!.LoadAsync(scope);
            else if (_personalSurface.Kind == NativePersonalSurfaceKind.Profile) await _profile!.LoadAsync(scope);
            else if (_personalSurface.Kind is { } kind) await _inbox!.LoadAsync(scope, InboxTab(kind), app.PersonalCommands);
        }
        catch (OperationCanceledException) { }
    }
    private void SavePersonalOrigin(ILiveApplication app)
    {
        var state = app.Capture();
        if (state.Navigation?.Current is not { } visit) return;
        if (state.Threads?.Visit is { IsDirectory: false } thread && _threadTimeline is not null)
            app.RememberThreadViewport(thread, _threadTimeline.Capture());
        else if (_timeline is not null) app.RememberViewport(visit, _timeline.Capture());
        _focusVisit = visit.Generation;
    }
    private async Task OpenPersonalAsync(ILiveApplication app, NativePersonalSurfaceKind kind)
    {
        var state = app.Capture();
        if (!ReferenceEquals(app, _personalApp) || Program.CloseAdmissionClosed || !state.ShowConversation ||
            state.Authentication.Phase != NativeAuthPhase.Active || !state.Chat.Connected) return;
        _personalNavigationSerial++;
        if (!PersonalOpen) SavePersonalOrigin(app);
        var before = _personalSurface.Scope;
        var scope = _personalSurface.Open(kind);
        if (scope is null) return;
        _personalReturn = null;
        ApplyPersonalBindings(app, state, before); _loadedPersonalScope = scope;
        // The mutable predicates close admission immediately, before Reactoring
        // the covered tree. Existing drafts and edit controls remain mounted.
        _surfaceVisible = _threadSurfaceVisible = false;
        _rootDropContext = _threadDropContext = null;
        _personalInvalidate?.Invoke();
        await LoadPersonalAsync(scope);
    }
    private void ClosePersonal(ILiveApplication app, NativePersonalScope scope, bool restoreFocus = true)
    {
        if (!PersonalCurrent(app, scope)) return;
        _personalNavigationSerial++;
        CacheCanonicalSelf(); _personalSurface.Close(); _loadedPersonalScope = null;
        ApplyPersonalBindings(app, app.Capture(), scope); _personalInvalidate?.Invoke();
        if (restoreFocus && _conversationFocus?.TryGetTarget(out var input) == true)
        {
            long? expectedVisit = _focusVisit;
            input.DispatcherQueue.TryEnqueue(DispatcherQueuePriority.Low, () =>
            {
                var latest = app.Capture();
                if (!PersonalOpen && latest.Authentication.UserId == scope.UserId && latest.Authentication.SessionStamp == scope.SessionStamp &&
                    latest.Navigation?.Current?.Generation == expectedVisit && input.IsLoaded && input.IsEnabled)
                    input.Focus(FocusState.Programmatic);
            });
        }
    }
    private async Task OpenPersonalSourceAsync(ILiveApplication app, NativePersonalScope scope, NativeNavigationTarget target)
    {
        if (!PersonalCurrent(app, scope) || app.Capture().Navigation?.Current is not { } origin || _personalSurface.Kind is not { } kind) return;
        SavePersonalOrigin(app);
        var frame = new PersonalReturn(kind, scope.UserId, scope.SessionStamp, origin.Target, origin.Generation);
        ClosePersonal(app, scope, false); _personalReturn = frame;
        long serial = ++_personalNavigationSerial;
        var navigating = app.NavigateAsync(target);
        var started = app.Capture().Navigation?.Current;
        await navigating;
        var latest = app.Capture();
        if (_personalNavigationSerial == serial && ReferenceEquals(_personalReturn, frame) && latest.Authentication.UserId == frame.UserId && latest.Authentication.SessionStamp == frame.Stamp &&
            latest.Navigation is { Pending: false, Error: null, Current: { } destination } && started?.Generation == destination.Generation && destination.Target == target)
            _personalReturn = frame with { Destination = destination.Target, DestinationGeneration = destination.Generation };
        else if (ReferenceEquals(_personalReturn, frame)) _personalReturn = null;
        _personalInvalidate?.Invoke();
    }
    private bool CanReturnPersonal(ILiveApplication app)
    {
        var state = app.Capture();
        return _personalReturn is { DestinationGeneration: { } generation } frame && !PersonalOpen &&
            state.Authentication.UserId == frame.UserId && state.Authentication.SessionStamp == frame.Stamp &&
            state.Navigation?.Current?.Generation == generation && state.Navigation.Current.Target == frame.Destination;
    }
    private async Task<bool> ReturnPersonalAsync(ILiveApplication app)
    {
        if (!CanReturnPersonal(app) || _personalReturn is not { } frame) return false;
        _personalReturn = null;
        long serial = ++_personalNavigationSerial;
        var returning = frame.DestinationGeneration != frame.OriginGeneration && app.Capture().Navigation?.CanBack == true ? app.BackAsync() : Task.CompletedTask;
        var started = app.Capture().Navigation?.Current;
        await returning;
        var latest = app.Capture();
        if (_personalNavigationSerial == serial && !PersonalOpen && latest.Authentication.UserId == frame.UserId && latest.Authentication.SessionStamp == frame.Stamp &&
            latest.Navigation is { Pending: false, Error: null, Current: { } origin } && origin.Generation == started?.Generation && origin.Target == frame.Origin)
            await OpenPersonalAsync(app, frame.Kind);
        return true;
    }
    private void ConversationGotFocus(object sender, RoutedEventArgs args)
    {
        if (PersonalOpen) return;
        for (DependencyObject? element = args.OriginalSource as DependencyObject; element is not null; element = VisualTreeHelper.GetParent(element))
            if (element is TextBox input) { _conversationFocus = new(input); break; }
    }
    private Element PersonalToolbar(ILiveApplication app, LiveSnapshot state, SproytPalette palette)
    {
        Element Open(string text, NativePersonalSurfaceKind kind, string id) => Button(text, async () => await OpenPersonalAsync(app, kind))
            .MinHeight(36).AutomationId(id).AutomationName(text).IsEnabled(state.ShowConversation && state.Chat.Connected && state.Authentication.Phase == NativeAuthPhase.Active);
        return VStack(4,
            HStack(4, Open("Personar", NativePersonalSurfaceKind.People, "live-personal-people"), Open("Innboks", NativePersonalSurfaceKind.Unread, "live-personal-inbox"),
                Open("Profil", NativePersonalSurfaceKind.Profile, "live-personal-profile")),
            Button(_personalReturn?.Kind == NativePersonalSurfaceKind.People ? "Tilbake til personar" : "Tilbake til innboksen", async () => await ReturnPersonalAsync(app)).MinHeight(36).AutomationId("live-personal-return")
                .AutomationName("Tilbake til personleg vising")
                .IsVisible(CanReturnPersonal(app))).Padding(12, 0, 12, 4).Background(palette.Chrome);
    }
    private Element PersonalView(ILiveApplication app, LiveSnapshot state, SproytPalette palette)
    {
        if (_personalSurface.Scope is not { } scope || !PersonalCurrent(app, scope)) return Empty();
        async Task OpenDirect(NativePersonalScope captured, string user)
        {
            if (!PersonalCurrent(app, captured)) return;
            string? channel = await _people!.OpenDirectAsync(captured, user);
            if (channel is not null && PersonalCurrent(app, captured)) await OpenPersonalSourceAsync(app, captured, new(channel));
        }
        async Task ReadDirect(NativePersonalScope captured)
        {
            if (!PersonalCurrent(app, captured)) return;
            string? channel = await _people!.RefreshUncertainDirectAsync(captured);
            if (channel is not null && PersonalCurrent(app, captured)) await OpenPersonalSourceAsync(app, captured, new(channel));
        }
        Element content = _personalSurface.Kind switch
        {
            NativePersonalSurfaceKind.People => NativePeopleView.Render(_people!.Capture(), palette,
                new(_people.Query, _people.LoadAsync, OpenDirect, ReadDirect, _people.DismissUncertainDirect)),
            NativePersonalSurfaceKind.Profile => NativeProfileView.Render(_profile!.Capture(), palette,
                new(_profile.EditName, _profile.EditStatusText, _profile.EditStatusEmoji, _profile.LoadAsync,
                    _profile.SaveNameAsync, _profile.SaveStatusAsync, _profile.ClearStatusAsync, _profile.ReadBackAsync)),
            _ => NativeInboxView.Render(_inbox!, palette, new(
                () => ClosePersonal(app, scope), tab => { _ = OpenPersonalAsync(app, tab switch { NativeInboxTab.Mentions => NativePersonalSurfaceKind.Mentions,
                    NativeInboxTab.Tasks => NativePersonalSurfaceKind.Tasks, _ => NativePersonalSurfaceKind.Unread }); },
                () => { _ = LoadPersonalAsync(scope); }, id => { _ = _inbox!.MarkMentionReadAsync(scope, id, app.PersonalCommands); },
                id => { _ = _inbox!.ReadBackMentionAsync(scope, id, app.PersonalCommands); },
                (id, done) => { _ = _inbox!.SetTaskDoneAsync(scope, id, done, app.PersonalCommands); },
                id => { _ = _inbox!.ReadBackTaskAsync(scope, id, app.PersonalCommands); }, target => { _ = OpenPersonalSourceAsync(app, scope, target); }))
        };
        return Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star()],
            HStack(8, TextBlock(_personalSurface.Kind == NativePersonalSurfaceKind.Profile ? "Profil og status" : _personalSurface.Kind == NativePersonalSurfaceKind.People ? "Personar" : "Innboks")
                    .FontSize(22).Bold().Foreground(palette.Ink), Button("Tilbake til samtalen", () => ClosePersonal(app, scope)).MinHeight(40)
                    .AutomationId("live-personal-close")).Padding(12).Grid(row: 0),
            content.IsEnabled(state.Chat.Connected && state.Authentication.Phase == NativeAuthPhase.Active).Grid(row: 1))
            .Background(palette.Paper).AutomationId("live-personal-surface");
    }
}
