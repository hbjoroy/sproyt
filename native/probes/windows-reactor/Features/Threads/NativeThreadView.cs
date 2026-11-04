using Microsoft.UI.Input;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.System;
using Windows.UI.Core;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record NativeThreadDirectoryBindings(Action Close, Action Retry, Action<string> Open);

internal sealed record NativeThreadDetailBindings(Func<NativeMessagePresentation, bool, Element> RenderMessage,
    string DraftBody, Action<string> DraftChanged, Action Send, Action Close, Action Retry,
    bool CanSend, bool ComposerEnabled = true, IReadOnlyList<NativeMessage>? PendingReplies = null,
    Action<ScrollViewer>? ViewportMounted = null, Action? ViewportUnmounted = null,
    Action<TextBox>? ComposerMounted = null, Element? DeliveryNotice = null, Element? UploadTools = null,
    Func<NativeMessage, NativeMessagePresentation>? PresentMessage = null);

// The host supplies the same message renderer/actions used in the channel and
// the durable, target-bound composer. This view owns neither tokens nor drafts.
internal static class NativeThreadView
{
    internal static Element RenderDirectory(NativeThreadFeatureSnapshot state, SproytPalette palette,
        NativeThreadDirectoryBindings actions)
    {
        if (!state.DirectoryOpen) return Empty();
        Element Row(NativeThreadDirectoryItem item)
        {
            string title = item.Root is { DeletedAt: not null } ? "Sletta melding" : item.Root?.Author ?? "Tråd";
            string preview = item.Root is { DeletedAt: not null } ? "Den opphavlege meldinga er sletta." : item.Root is null
                ? "Opne for å hente den opphavlege meldinga." : Preview(item.Root.Body);
            string answers = item.Summary.ReplyCount == 1 ? "1 svar" : $"{item.Summary.ReplyCount} svar";
            string unread = item.Summary.UnreadCount == 0 ? "" : $" · {item.Summary.UnreadCount} ulest";
            return Button(VStack(5,
                TextBlock(title).SemiBold().FontSize(14).Foreground(palette.Ink),
                TextBlock(preview).TextWrapping().Foreground(palette.Ink),
                TextBlock(answers + unread).FontSize(12).Foreground(palette.Muted)),
                () => actions.Open(item.Summary.RootMessageId))
                .HAlign(HorizontalAlignment.Stretch).HorizontalContentAlignment(HorizontalAlignment.Stretch)
                .Padding(16, 12).Margin(4, 2).MinHeight(44).Background(palette.Surface)
                .AutomationName("Opne tråd, " + answers + unread).AutomationId("live-thread-directory-" + item.Summary.RootMessageId);
        }
        return Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star()],
            VStack(6,
                Header("Trådar i samtalen", palette, actions.Close),
                Status(state, palette, actions.Retry),
                TextBlock("Ingen trådar med svar i denne samtalen enno.").TextWrapping().Foreground(palette.Muted)
                    .IsVisible(!state.Loading && state.Error is null && state.Directory.Count == 0).Margin(16, 8)
            ).Grid(row: 0),
            LazyVStack<NativeThreadDirectoryItem>(state.Directory, item => item.Summary.RootMessageId, (item, _) => Row(item))
                .AutomationName("Trådar i samtalen").AutomationId("live-thread-directory-list").Grid(row: 1)
        ).Background(palette.Paper).AutomationId("live-thread-directory");
    }

    internal static Element RenderDetail(NativeThreadFeatureSnapshot state, SproytPalette palette,
        NativeThreadDetailBindings bindings)
    {
        if (!state.DetailOpen || state.Visit is not { RootMessageId: string rootId } visit) return Empty();
        var thread = state.Thread;
        var messages = (thread?.Root is { } root ? new[] { root } : Array.Empty<NativeMessage>())
            .Concat(thread?.Replies ?? [])
            .Concat((bindings.PendingReplies ?? []).Where(message => message.ChannelId == visit.ChannelId && message.ParentMessageId == rootId))
            .ToArray();
        var presentations = messages.Select(message => bindings.PresentMessage?.Invoke(message)
            ?? throw new InvalidOperationException("The host must freeze each thread row's presentation inputs.")).ToArray();
        var editor = TextBox(bindings.DraftBody, bindings.DraftChanged, placeholderText: "Skriv eit svar i tråden …")
            .AcceptsReturn(true).TextWrapping(TextWrapping.Wrap).MinHeight(44).MaxHeight(180)
            .AutomationName("Skriv eit svar i tråden").AutomationId("live-thread-composer")
            .IsEnabled(bindings.ComposerEnabled)
            .OnMount(element =>
            {
                if (element is not TextBox input) return;
                bindings.ComposerMounted?.Invoke(input);
                input.PreviewKeyDown += (_, args) =>
                {
                    if (args.Key == VirtualKey.Enter && (InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & CoreVirtualKeyStates.Down) != 0)
                    {
                        args.Handled = true;
                        // Re-read admission in the host callback as well; this
                        // native handler can outlive the render's projection.
                        bindings.Send();
                    }
                };
            }).WithKey("live-thread-composer-" + visit.ChannelId + "-" + rootId + "-" + visit.Generation);
        return Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star(), GridSize.Auto, GridSize.Auto],
            VStack(5,
                Header("Tråd", palette, bindings.Close),
                Status(state, palette, bindings.Retry),
                TextBlock("Den opphavlege meldinga er ikkje lasta.").TextWrapping().Foreground(palette.Muted)
                    .Margin(16, 4).IsVisible(!state.Loading && state.Error is null && thread?.Root is null)
            ).Grid(row: 0),
            LazyVStack<NativeMessagePresentation>(presentations, item => item.Message.Id, (item, _) =>
                bindings.RenderMessage(item, item.Message.Id == rootId))
                .AutomationName("Meldingar i tråden").AutomationId("live-thread-history")
                .OnMount(element => { if (element is ScrollViewer scroller) bindings.ViewportMounted?.Invoke(scroller); })
                .OnUnmount(_ => bindings.ViewportUnmounted?.Invoke())
                .WithKey("live-thread-history-" + visit.ChannelId + "-" + rootId).Grid(row: 1),
            (bindings.DeliveryNotice ?? Empty()).Grid(row: 2),
            VStack(4, bindings.UploadTools ?? Empty(), Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
                editor.Grid(column: 0),
                Button("Send svar", bindings.Send).MinHeight(44).Padding(12, 8).Margin(8, 0, 0, 0)
                    .IsEnabled(bindings.CanSend).AutomationName("Send svar i tråden").AutomationId("live-thread-send").Grid(column: 1)
            )).Padding(12, 8).Background(palette.Chrome).Grid(row: 3)
        ).Background(palette.Paper).AutomationId("live-thread-pane");
    }

    private static Element Header(string title, SproytPalette palette, Action close) =>
        Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
            TextBlock(title).FontSize(21).SemiBold().Foreground(palette.Ink).VAlign(VerticalAlignment.Center).Grid(column: 0),
            Button("Lukk", close).MinHeight(44).Padding(12, 8).AutomationName("Lukk trådvisinga").AutomationId("live-thread-close").Grid(column: 1)
        ).Padding(16, 8).Background(palette.Chrome);

    private static Element Status(NativeThreadFeatureSnapshot state, SproytPalette palette, Action retry) =>
        VStack(5,
            TextBlock(state.Loading ? "Hentar tråden …" : state.Error ?? "").TextWrapping()
                .Foreground(state.Error is null ? palette.Muted : palette.Error).IsVisible(state.Loading || state.Error is not null),
            Button("Prøv igjen", retry).MinHeight(44).AutomationName("Hent tråden på nytt").AutomationId("live-thread-retry")
                .IsVisible(!state.Loading && state.Error is not null),
            TextBlock(state.DirectoryOpen ? "Trådoversikta er avgrensa til 2000 trådar i denne Windows-versjonen."
                : "Trådhistorikken er avgrensa til 2000 svar i denne Windows-versjonen.").TextWrapping()
                .Foreground(palette.Muted).IsVisible(state.AtCapacity)
        ).Margin(16, 4);

    private static string Preview(string body) => string.Concat(body.Replace('\r', ' ').Replace('\n', ' ').EnumerateRunes().Take(160).Select(rune => rune.ToString()));
}
