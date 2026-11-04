using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal static class NativeMessageActionsView
{
    internal static Element Render(NativeMessageActionController owner, NativeMessageActionScope scope,
        NativeMessage message, IReadOnlyList<NativeReactionChip> reactions, SproytPalette palette,
        NativeMessageActionCommands commands, bool reactionsReady = true, Action? refreshReactions = null)
    {
        string id = message.Id;
        if (!owner.Available(scope, message)) return Empty();
        bool active = owner.MessageId == id;
        NativeMessageActionMode mode = active ? owner.Mode : NativeMessageActionMode.Closed;
        bool enabled = owner.CanOpen(scope, message);
        Element ActionButton(string label, string key, Action action, bool allowed = true) =>
            Button(label, action).IsEnabled(allowed).MinHeight(36).Padding(8, 4)
                .AutomationName(label).AutomationId($"live-message-{key}-{id}");
        void Open(NativeMessageActionMode value) => owner.Open(scope, message, value);
        Element Choice(string emoji) => ActionButton(emoji, "emoji-" + emoji,
            async () => await owner.ToggleAsync(scope, message, emoji, reactions.Any(c => c.Emoji == emoji && c.Mine), commands), enabled && reactionsReady);
        var rows = new List<Element>();
        foreach (var chip in reactions.Where(c => c.Count != 0))
        {
            string text = $"{chip.Emoji} {chip.Count}" + (chip.Mine ? " · Du" : "");
            string names = chip.Names.Count > 0 ? string.Join(", ", chip.Names) : "Ingen namn tilgjengelege";
            rows.Add(ActionButton(text, "reaction-" + chip.Emoji,
                async () => await owner.ToggleAsync(scope, message, chip.Emoji, chip.Mine, commands),
                enabled && reactionsReady && mode is NativeMessageActionMode.Closed or NativeMessageActionMode.Reactions)
                .HAlign(HorizontalAlignment.Left).AutomationName(text + ". " + names)
                .OnMount(native =>
                {
                    if (native is not FrameworkElement target) return;
                    ToolTipService.SetToolTip(target, names);
                    target.ContextFlyout = new Flyout
                    {
                        Content = new Microsoft.UI.Xaml.Controls.TextBlock { Text = names, TextWrapping = TextWrapping.Wrap, MaxWidth = 280 }
                    };
                }).WithKey(chip.Emoji + ":" + names));
        }
        if (mode == NativeMessageActionMode.Closed)
            rows.Add(ActionButton("Meldingsval", "actions", () => Open(NativeMessageActionMode.Menu), enabled));
        if (mode == NativeMessageActionMode.Menu)
        {
            rows.Add(ActionButton("Reager", "choose-reaction", () => Open(NativeMessageActionMode.Reactions), enabled));
            if (message.AuthorId == scope.UserId)
                rows.Add(HStack(4, ActionButton("Rediger", "edit", () => Open(NativeMessageActionMode.Edit), enabled),
                    ActionButton("Slett", "delete", () => Open(NativeMessageActionMode.Delete), enabled)));
        }
        if (mode == NativeMessageActionMode.Edit)
        {
            rows.Add(TextBox(owner.Input, text => owner.ChangeInput(scope, id, mode, text))
                .AcceptsReturn(true).TextWrapping(TextWrapping.Wrap).MaxHeight(180).IsEnabled(enabled)
                .AutomationName("Endra meldingstekst").AutomationId("live-message-edit-input-" + id));
            if (owner.HasAttachments) rows.Add(TextBlock("Vedlegga blir verande på meldinga.").Foreground(palette.Muted).TextWrapping());
            rows.Add(ActionButton("Lagre endring", "save", async () => await owner.ConfirmAsync(scope, message, commands), enabled && owner.CanSave));
        }
        if (mode == NativeMessageActionMode.Delete)
        {
            rows.Add(TextBlock("Slette denne meldinga?").Foreground(palette.Ink).TextWrapping());
            rows.Add(ActionButton("Stadfest sletting", "confirm-delete", async () => await owner.ConfirmAsync(scope, message, commands), enabled));
        }
        if (mode == NativeMessageActionMode.Reactions)
        {
            if (!reactionsReady)
                rows.Add(ActionButton("Hent reaksjonar", "refresh-reactions", () => refreshReactions?.Invoke(), !owner.Busy));
            rows.Add(HStack(4, Choice("👍"), Choice("❤️"), Choice("🎉"), Choice("😂")));
            rows.Add(TextBox(owner.Input, text => owner.ChangeInput(scope, id, mode, text)).PlaceholderText("Skriv eller lim inn ein emoji")
                .IsEnabled(enabled).AutomationName("Eigen emoji").AutomationId("live-message-emoji-input-" + id));
            string emoji = owner.Input.Trim();
            rows.Add(ActionButton("Bruk emoji", "toggle", async () => await owner.ToggleAsync(scope, message, emoji,
                reactions.Any(c => c.Emoji == emoji && c.Mine), commands), enabled && reactionsReady && NativeMessageActionController.ValidEmoji(emoji)));
        }
        if (active)
        {
            if (owner.Error is { } error) rows.Add(TextBlock(error).TextWrapping().Foreground(palette.Error).AutomationId("live-message-action-error-" + id));
            if (owner.NeedsReadBack)
                rows.Add(ActionButton("Kontroller endringa", "readback", async () => await owner.CheckAsync(scope, id, commands), !owner.Busy));
            else rows.Add(ActionButton(mode == NativeMessageActionMode.Menu ? "Lukk" : "Avbryt", "cancel", () => owner.Cancel(scope, id), !owner.Busy));
        }
        return VStack(4, rows.ToArray()).HAlign(HorizontalAlignment.Left);
    }
}
