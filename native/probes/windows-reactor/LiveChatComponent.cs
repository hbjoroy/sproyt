using Microsoft.UI.Input;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Text;
using Windows.System;
using Windows.UI.Core;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

// Uses real server projections. No demo channels or manufactured chat history.
internal sealed class LiveChatComponent : Component
{
    internal static Func<Task>? SendForProbe;
    private ScrollViewer? _scroller;
    private ChatViewportFollow? _follow;
    private readonly Dictionary<string, double> _positions = [];
    private string? _visibleChannel;
    private string? _lastMessage;
    private int _navigation;
    private int _revision;

    public override Element Render()
    {
        var app = Program.Live;
        var (_, invalidate) = UseState(0, threadSafe: true);
        PilotComponent.InvalidateForClose = () => invalidate(Interlocked.Increment(ref _revision));
        var (dark, setDark) = UseState(Application.Current.RequestedTheme == ApplicationTheme.Dark);
        var (showList, setShowList) = UseState(false);
        bool wide = UseBreakpoint(800);
        UseEffect(() =>
        {
            void Changed() => invalidate(Interlocked.Increment(ref _revision));
            Program.LiveStateChanged += Changed;
            if (app is not null) app.Changed += Changed;
            else _ = Program.OpenLiveAsync();
            return () => { Program.LiveStateChanged -= Changed; if (app is not null) app.Changed -= Changed; _follow?.Dispose(); };
        }, app is null ? "starting" : "ready");
        if (app is null)
        {
            var startup = dark ? SproytPalette.Dark : SproytPalette.Light;
            async void TryOpen() => await Program.OpenLiveAsync();
            return Grid([GridSize.Star()], [GridSize.Star()], VStack(16,
                TextBlock("Sprøyt").FontSize(32).Bold().Foreground(startup.Ink),
                TextBlock(Program.LiveError ?? "Koplar til Canary …").TextWrapping().Foreground(startup.Muted),
                Button("Prøv igjen", TryOpen).MinHeight(44).IsVisible(Program.LiveError is not null)
            ).Padding(32).VAlign(VerticalAlignment.Center)).Background(startup.Paper)
                .OnMount(element => PilotComponent.RootForClose = element).AutomationId("live-startup");
        }
        var state = app.Capture(); var auth = state.Authentication; var chat = state.Chat;
        var palette = dark ? SproytPalette.Dark : SproytPalette.Light;
        string? channel = state.ShowConversation ? chat.SelectedChannel : null;
        var selected = chat.Channels.FirstOrDefault(item => item.Id == channel);
        var rows = state.ShowConversation ? chat.Messages.Concat((state.Workspace?.Outbox ?? [])
            .Where(item => item.Channel == channel).Select(item => new NativeMessage("pending:" + item.RequestId,
                item.Channel, auth.UserId ?? "", auth.DisplayName ?? "Du", item.Body, DateTimeOffset.MinValue, ulong.MaxValue))).ToArray() : [];
        var draft = channel is not null && state.Workspace?.Drafts.TryGetValue(channel, out var current) == true ? current : null;
        bool canSend = !state.ConfirmPendingSends && draft is not null && !string.IsNullOrWhiteSpace(draft.Body) && Encoding.UTF8.GetByteCount(draft.Body) <= 1024 &&
            !Program.CloseAdmissionClosed && state.Workspace is { Ready: true, Closing: false, Sending: false, NeedsRetry: false } &&
            state.Workspace.Outbox.Count < 8 && auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline;

        async void Login() { if (!Program.CloseAdmissionClosed) await app.LoginAsync(); }
        async void Logout() { if (!Program.CloseAdmissionClosed) await app.LogoutAsync(); }
        async void Retry() { if (!Program.CloseAdmissionClosed) await app.ResumeAsync(); }
        async void Select(string id)
        {
            if (Program.CloseAdmissionClosed) return;
            if (_visibleChannel is not null && _scroller is not null) _positions[_visibleChannel] = _scroller.VerticalOffset;
            setShowList(false); await app.SelectAsync(id);
        }
        async Task SendCurrent()
        {
            var latest = app.Capture();
            if (channel is null || latest.Chat.SelectedChannel != channel || !latest.ShowConversation || Program.CloseAdmissionClosed ||
                latest.ConfirmPendingSends || latest.Workspace is not { Ready: true, Closing: false, Sending: false, NeedsRetry: false } ||
                !latest.Workspace.Drafts.TryGetValue(channel, out var text) || string.IsNullOrWhiteSpace(text.Body) ||
                Encoding.UTF8.GetByteCount(text.Body) > 1024 || latest.Workspace.Outbox.Count >= 8 ||
                latest.Authentication.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline)) return;
            await app.SendAsync(channel);
            if (channel == app.Chat.SelectedChannel && app.Workspace?.Drafts[channel].Body == "")
                { _positions.Remove(channel); Reveal(channel, force: true); }
        }
        async void Send() => await SendCurrent();
        if (Program.LiveUiProbeDirectory is not null) SendForProbe = SendCurrent;

        string status = auth.Phase switch
        {
            NativeAuthPhase.Active when chat.Connected => "● Tilkopla",
            NativeAuthPhase.Active => "◌ Koplar til samtalen …",
            NativeAuthPhase.Offline => "◌ Fråkopla · utkast og sendekø er tekne vare på",
            NativeAuthPhase.SigningIn => "◌ Fullfør innlogginga i nettlesaren",
            NativeAuthPhase.AuthRequired => "○ Logg inn på nytt · utkastet er bevart",
            _ => "○ Logga ut",
        };
        bool loginVisible = auth.Phase is NativeAuthPhase.SignedOut or NativeAuthPhase.SigningIn or NativeAuthPhase.AuthRequired;
        var masthead = Grid([GridSize.Auto, GridSize.Star(), GridSize.Auto, GridSize.Auto], [GridSize.Auto],
            TextBlock("Sprøyt").FontSize(23).Bold().Foreground(palette.Ink).VAlign(VerticalAlignment.Center).Grid(column: 0),
            VStack(2, TextBlock(auth.DisplayName ?? "Canary").FontSize(13).SemiBold().Foreground(palette.Ink),
                TextBlock(status).FontSize(12).Foreground(palette.Muted)).Margin(16, 0).Grid(column: 1),
            Button(auth.Phase == NativeAuthPhase.SigningIn ? "Avbryt" : loginVisible ? "Logg inn" : "Logg ut", () =>
                { if (auth.Phase == NativeAuthPhase.SigningIn) Logout(); else if (loginVisible) Login(); else Logout(); })
                .AutomationId("live-authentication").AutomationName("Innlogging og utlogging").MinHeight(44).Grid(column: 2),
            Button("◐", () => setDark(!dark)).AutomationName("Byt lyst og mørkt tema").Size(44, 44).Margin(8, 0, 0, 0).Grid(column: 3)
        ).Padding(16, 8).Background(palette.Chrome).Grid(columnSpan: 2);

        Element ChannelRow(NativeChannel item) => Button(
            TextBlock((item.IsDirect ? "" : "# ") + item.Name).FontSize(16).TextWrapping().Foreground(palette.Ink), () => Select(item.Id))
            .HorizontalContentAlignment(HorizontalAlignment.Stretch).HAlign(HorizontalAlignment.Stretch)
            .MinHeight(44).Padding(12, 8).Margin(4, 0).Background(item.Id == channel ? palette.Selection : palette.Chrome)
            .BorderThickness(new Thickness(0)).CornerRadius(2).AutomationName("Opne " + item.Name).AutomationId("live-channel-" + item.Id);
        var list = ScrollViewer(VStack(4,
            new Element[] { TextBlock("SAMTALAR").FontFamily("Cascadia Mono").FontSize(12).Foreground(palette.Muted).Margin(16, 12) }
                .Concat(state.ShowConversation ? NativeMessageContent.Groups(chat.Channels).SelectMany(group =>
                    new Element[] { TextBlock(group.Name.ToUpperInvariant()).FontFamily("Cascadia Mono").FontSize(12).Foreground(palette.Muted).Margin(16, 16, 12, 6).AutomationId("live-group-" + group.Id) }
                        .Concat(group.Channels.Select(ChannelRow))) : [])
                .ToArray())).Background(palette.Chrome).IsVisible(state.ShowConversation && (wide || showList))
            .Grid(row: 1, column: 0, columnSpan: wide ? 1 : 2);

        Element Message(NativeMessage message) => VStack(5,
            Border(Empty()).Height(1).Width(56).Background(palette.Rule).HAlign(HorizontalAlignment.Left),
            HStack(8, TextBlock(message.Author).FontSize(14).SemiBold().Foreground(palette.Ink),
                TextBlock(message.Id.StartsWith("pending:", StringComparison.Ordinal) ? "Lagra på denne eininga" : message.SentAt.ToLocalTime().ToString("HH:mm"))
                    .FontSize(12).Foreground(palette.Muted)),
            NativeMessageView.Render(app, message, palette, auth)
        ).Padding(16, 8).AutomationId("live-message-" + message.Id).WithKey(message.Id);
        var history = LazyVStack<NativeMessage>(rows, item => item.Id, (item, _) => Message(item))
            .AutomationId("live-history").AutomationName("Meldingar")
            .OnMount(element =>
            {
                if (element is not ScrollViewer scroller) return;
                _scroller = scroller;
                _follow = new(scroller, () => !Program.CloseAdmissionClosed, () => { if (_visibleChannel is string id) Reveal(id, force: true); });
                Reveal(channel, force: false);
            })
            .OnUnmount(_ => { _follow?.Dispose(); _follow = null; _scroller = null; })
            .WithKey("live-history").Grid(row: 1);
        if (_visibleChannel != channel)
        {
            _visibleChannel = channel; _lastMessage = null; Reveal(channel, force: false);
        }
        string? last = rows.LastOrDefault()?.Id;
        if (last != _lastMessage) { _lastMessage = last; _follow?.Transition(); }

        var editor = TextBox(draft?.Body ?? "", text =>
        {
            if (channel is not null && app.Workspace?.Drafts.TryGetValue(channel, out var latest) == true && latest.Body != text)
                app.Edit(channel, text);
        }, placeholderText: "Skriv ei melding …").AcceptsReturn(true).TextWrapping(TextWrapping.Wrap).MinHeight(44).MaxHeight(180)
            .AutomationId("live-composer").AutomationName("Skriv ei melding")
            .IsEnabled(draft is not null && !Program.CloseAdmissionClosed)
            .OnMount(element =>
            {
                if (element is not TextBox input) return;
                PilotComponent.EditorForClose = input;
                input.PreviewKeyDown += (_, args) =>
                {
                    if (args.Key == VirtualKey.Enter && (InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & CoreVirtualKeyStates.Down) != 0)
                    { args.Handled = true; Send(); }
                };
            }).WithKey("live-composer-" + channel);
        string notice = state.ConfirmPendingSends ? "Du har lagra meldingar frå førre innlogging. Stadfest før dei blir sende." : state.Error ?? (Encoding.UTF8.GetByteCount(draft?.Body ?? "") > 1024 ? "Meldinga er for lang for denne første Windows-versjonen." :
            state.Workspace?.Outbox.Count > 0 ? $"{state.Workspace.Outbox.Count} meldingar ventar på sendekvittering." : "");
        var composer = Border(Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto], editor.Grid(column: 0),
            Button("↑", Send).AutomationId("live-send").AutomationName("Send meldinga").Size(48, 48).Margin(8, 0, 0, 0)
                .Foreground(palette.AccentInk).Background(palette.Accent).IsEnabled(canSend).Grid(column: 1)
        )).Padding(12).BorderThickness(new Thickness(0, 2, 0, 0)).BorderBrush(palette.Rule).Grid(row: 3);
        var conversation = Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star(), GridSize.Auto, GridSize.Auto],
            HStack(12, Button("←", () => setShowList(true)).AutomationName("Vis samtalar").Size(44, 44).IsVisible(!wide),
                VStack(2, TextBlock(selected?.IsDirect == true ? "Direkte" : selected?.CircleName ?? "Felles").FontSize(12).Foreground(palette.Muted),
                    TextBlock(selected is null ? "Samtalar" : (selected.IsDirect ? "" : "# ") + selected.Name).FontSize(21).Bold().Foreground(palette.Ink)))
                .Padding(12, 8).Grid(row: 0), history,
            Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto], TextBlock(notice).TextWrapping().FontSize(12).Foreground(palette.Muted).Grid(column: 0),
                Button(state.ConfirmPendingSends ? "Send ventande" : "Prøv igjen", async () =>
                    { if (state.ConfirmPendingSends && auth.UserId is string user && auth.SessionStamp is string stamp)
                        await app.ConfirmPendingSendsAsync(user, stamp); else Retry(); })
                    .AutomationName("Stadfest ventande meldingar eller prøv igjen").MinHeight(44).IsVisible(notice.Length > 0).Grid(column: 1))
                .Padding(12, 4).IsVisible(notice.Length > 0).Grid(row: 2), composer
        ).IsVisible(state.ShowConversation && (wide || !showList)).Grid(row: 1, column: 1);

        string invitation = auth.Phase == NativeAuthPhase.SigningIn ? "Vel kontoen din i nettlesaren og stadfest innlogginga."
            : auth.Phase == NativeAuthPhase.AuthRequired ? "Logg inn igjen. Utkast og ventande meldingar ventar på deg."
            : auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline ? "Lastar samtalane dine …" : "Logg inn med den vanlege Sprøyt-kontoen din.";
        var welcome = VStack(16, TextBlock("Velkomen til Sprøyt").FontSize(28).Bold().Foreground(palette.Ink),
            TextBlock(invitation).TextWrapping().FontSize(16).Foreground(palette.Muted),
            TextBlock(state.Error ?? "").TextWrapping().Foreground(palette.Muted),
            Button("Logg inn med Sprøyt", Login).MinHeight(44).IsVisible(loginVisible).AutomationName("Logg inn med Sprøyt")
        ).Padding(32).VAlign(VerticalAlignment.Center).IsVisible(!state.ShowConversation).Grid(row: 1, columnSpan: 2);
        return Grid([wide ? GridSize.Px(244) : GridSize.Px(0), GridSize.Star()], [GridSize.Auto, GridSize.Star()], masthead, list, conversation, welcome)
            .Background(palette.Paper).RequestedTheme(dark ? ElementTheme.Dark : ElementTheme.Light)
            .OnMount(element => { PilotComponent.RootForClose = element; Program.Keyboard?.Attach(element as Grid); LiveUiProbe.Attach(element); LiveCanaryAcceptance.Attach(element); })
            .OnUnmount(_ => { PilotComponent.RootForClose = null; }).AutomationId("sproyt-live-root");
    }

    private async void Reveal(string? channel, bool force)
    {
        int navigation = ++_navigation;
        if (channel is null) return;
        for (int attempt = 0; attempt < 30; attempt++)
        {
            await Task.Delay(25);
            if (navigation != _navigation || channel != _visibleChannel || Program.CloseAdmissionClosed) return;
            if (_scroller is null || _scroller.ViewportHeight <= 0) continue;
            double target = !force && _positions.TryGetValue(channel, out double saved) ? saved : _scroller.ScrollableHeight;
            _scroller.ChangeView(null, target, null, true);
            _follow?.Synchronize(); return;
        }
    }
}
