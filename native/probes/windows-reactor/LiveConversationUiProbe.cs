using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Sproyt.WindowsProbe;

// Drives native controls on the real conversation surface. Session, server and
// persistence remain synthetic; transport/storage have their own contracts.
internal static class LiveConversationUiProbe
{
    private const string RootOne = "conversation-root-one", RootTwo = "conversation-root-two", Reply = "conversation-reply-one";

    internal static async Task RunAsync(FrameworkElement root, LiveUiFixture fixture)
    {
        bool wasActive = Program.LiveWindowActive;
        try
        {
            Program.LiveWindowActive = true;
            fixture.PrepareConversations();
            await Until("conversation roots", () => fixture.Chat.Messages.Count == 2 &&
                Find<Button>(root, "live-open-thread-" + RootOne) is { IsLoaded: true } &&
                Find<TextBox>(root, "live-composer") is { IsLoaded: true, Text: "" });
            Find<TextBox>(root, "live-composer")!.Text = "Utkast i kanalen";
            await Until("channel draft", () => fixture.Workspace.Drafts["channel-a"].Body == "Utkast i kanalen");

            Program.LiveWindowActive = false;
            await Click(root, "live-open-thread-" + RootOne);
            await Until("first thread", () => fixture.Capture().Threads?.Visit?.RootMessageId == RootOne &&
                Find<FrameworkElement>(root, "live-message-" + RootOne) is { IsLoaded: true } &&
                Find<TextBox>(root, "live-thread-composer") is { IsLoaded: true, Text: "" });
            await Task.Delay(350);
            Require(fixture.ThreadReadReports.Count == 0, "Inactive thread emitted a read receipt.");
            Program.LiveWindowActive = true; LiveChatComponent.ThreadTimelineForProbe?.ObserveSoon();
            await Until("visible thread read", () => fixture.ThreadReadReports.Count > 0);
            Find<TextBox>(root, "live-thread-composer")!.Text = "Utkast i første tråd";
            string firstKey = new ComposerTarget("channel-a", RootOne).Key, secondKey = new ComposerTarget("channel-a", RootTwo).Key;
            await Until("first thread draft", () => fixture.Workspace.Drafts[firstKey].Body == "Utkast i første tråd");
            await Click(root, "live-thread-close");
            await Until("root composer return", () => fixture.Capture().Threads?.Visit is null && Find<TextBox>(root, "live-composer")?.Text == "Utkast i kanalen");

            await Click(root, "live-open-thread-" + RootTwo);
            await Until("second thread", () => fixture.Capture().Threads?.Visit?.RootMessageId == RootTwo &&
                Find<FrameworkElement>(root, "live-message-" + RootTwo) is { IsLoaded: true } &&
                Find<TextBox>(root, "live-thread-composer") is { IsLoaded: true, Text: "" });
            Find<TextBox>(root, "live-thread-composer")!.Text = "Utkast i andre tråd";
            await Until("second thread draft", () => fixture.Workspace.Drafts[secondKey].Body == "Utkast i andre tråd");
            await Click(root, "live-thread-close");
            await Until("channel after second thread", () => fixture.Capture().Threads?.Visit is null);
            await Click(root, "live-open-thread-" + RootOne);
            await Until("retained first thread draft", () => fixture.Capture().Threads?.Visit?.RootMessageId == RootOne && Find<TextBox>(root, "live-thread-composer")?.Text == "Utkast i første tråd");
            await Click(root, "live-thread-send");
            await Until("parent-bound reply send", () => fixture.LastEnqueuedReply is { ParentMessageId: RootOne, Channel: "channel-a", Body: "Utkast i første tråd" } &&
                Find<TextBox>(root, "live-thread-composer")?.Text == "");
            Require(fixture.Workspace.Drafts["channel-a"].Body == "Utkast i kanalen" && fixture.Workspace.Drafts[secondKey].Body == "Utkast i andre tråd",
                "Reply submission cleared a different composer target.");

            await Click(root, "live-message-actions-conversation-reply-other");
            await Until("another author's menu", () => Find<Button>(root, "live-message-cancel-conversation-reply-other") is { IsLoaded: true });
            Require(Find<Button>(root, "live-message-edit-conversation-reply-other") is null &&
                Find<Button>(root, "live-message-delete-conversation-reply-other") is null, "Another author's reply exposed edit/delete controls.");
            await Click(root, "live-message-cancel-conversation-reply-other");

            await Edit(root, fixture, Reply, "Endra svar i tråden");
            await OpenAction(root, Reply, "delete");
            await Click(root, "live-message-cancel-" + Reply);
            Require(fixture.Message(Reply)?.DeletedAt is null, "Cancelling reply deletion committed it.");

            await OpenAction(root, Reply, "choose-reaction");
            Find<TextBox>(root, "live-message-emoji-input-" + Reply)!.Text = "👍";
            await Click(root, "live-message-toggle-" + Reply);
            await Until("reply reaction", () => fixture.Chat.Conversation!.Reactions.Any(reaction => reaction.MessageId == Reply && reaction.Emoji == "👍" && reaction.ReactedByMe && reaction.Count == 1));
            await Click(root, "live-message-reaction-👍-" + Reply);
            await Until("reply reaction removed", () => fixture.Chat.Conversation!.Reactions.All(reaction => reaction.MessageId != Reply || reaction.Emoji != "👍"));

            await OpenAction(root, Reply, "choose-reaction");
            Find<TextBox>(root, "live-message-emoji-input-" + Reply)!.Text = "🎉";
            fixture.FailNextMutationAfterCommit = true;
            int before = fixture.MutationExecutions;
            await Click(root, "live-message-toggle-" + Reply);
            await Until("uncertain reaction requires readback", () => Find<Button>(root, "live-message-readback-" + Reply) is { IsLoaded: true, IsEnabled: true });
            Require(fixture.MutationExecutions == before + 1, "An uncertain toggle was automatically replayed.");
            await Click(root, "live-message-readback-" + Reply);
            await Until("readback resolved committed reaction", () => fixture.MutationReadBacks > 0 && Find<Button>(root, "live-message-actions-" + Reply) is { IsLoaded: true });
            Require(fixture.MutationExecutions == before + 1 && fixture.Chat.Conversation!.Reactions.Any(reaction => reaction.MessageId == Reply && reaction.Emoji == "🎉" && reaction.ReactedByMe),
                "Reaction readback toggled a committed change again.");

            await OpenAction(root, Reply, "delete");
            await Click(root, "live-message-confirm-delete-" + Reply);
            await Until("reply tombstone", () => fixture.Message(Reply)?.DeletedAt is not null && Text(Find<FrameworkElement>(root, "live-message-" + Reply)).Contains("Meldinga er sletta.", StringComparison.Ordinal));
            Require(!Text(Find<FrameworkElement>(root, "live-message-" + Reply)).Contains("Endra svar i tråden", StringComparison.Ordinal), "Deleted reply leaked its old body.");

            await Click(root, "live-thread-close");
            await Until("root actions restored", () => fixture.Capture().Threads?.Visit is null);
            await Edit(root, fixture, RootOne, "Endra opphavleg melding");
            await OpenAction(root, RootOne, "choose-reaction");
            Find<TextBox>(root, "live-message-emoji-input-" + RootOne)!.Text = "❤️";
            await Click(root, "live-message-toggle-" + RootOne);
            await Until("root reaction", () => fixture.Chat.Conversation!.Reactions.Any(reaction => reaction.MessageId == RootOne && reaction.Emoji == "❤️" && reaction.ReactedByMe));
            await OpenAction(root, RootOne, "delete");
            await Click(root, "live-message-cancel-" + RootOne);
            Require(fixture.Message(RootOne)?.DeletedAt is null, "Cancelling root deletion committed it.");
            await OpenAction(root, RootOne, "delete");
            await Click(root, "live-message-confirm-delete-" + RootOne);
            await Until("root tombstone", () => fixture.Message(RootOne)?.DeletedAt is not null && Find<Button>(root, "live-open-thread-" + RootOne) is { IsLoaded: true });
            Require(!Text(Find<FrameworkElement>(root, "live-message-" + RootOne)).Contains("Endra opphavleg melding", StringComparison.Ordinal), "Deleted root leaked its old body.");

            await Click(root, "live-thread-directory-open");
            await Until("thread directory", () => fixture.Capture().Threads?.DirectoryOpen == true && Find<Button>(root, "live-thread-directory-" + RootOne) is { IsLoaded: true });
            await Click(root, "live-thread-directory-" + RootOne);
            await Until("deleted root thread remains openable", () => fixture.Capture().Threads is { DetailOpen: true, Thread.Root.DeletedAt: not null } && Find<TextBox>(root, "live-thread-composer") is { IsLoaded: true });
            Require(fixture.Chat.Conversation?.Thread?.Replies.Any(message => message.DeletedAt is null) == true, "Deleting root lost the retained thread replies.");
            await Click(root, "live-thread-close");
            await Until("final root draft", () => fixture.Capture().Threads?.Visit is null && Find<TextBox>(root, "live-composer")?.Text == "Utkast i kanalen");
            Require(fixture.ThreadReadReports.Count > 0 && fixture.ThreadReadReports.All(report => report.Sequence > 2), "Thread reading reported the parent as a reply.");
        }
        finally { Program.LiveWindowActive = wasActive; }
    }

    private static async Task Edit(FrameworkElement root, LiveUiFixture fixture, string id, string body)
    {
        await OpenAction(root, id, "edit");
        Find<TextBox>(root, "live-message-edit-input-" + id)!.Text = body;
        // WinUI can defer TextChanged until this dispatcher turn returns.
        // Let the actual input callback update the edit owner before Invoke.
        await Task.Delay(80);
        await Click(root, "live-message-save-" + id);
        await Until("edited message " + id, () => fixture.Message(id) is { EditedAt: not null } message && message.Body == body,
            () => $"executions={fixture.MutationExecutions}; body={fixture.Message(id)?.Body}; input={Find<TextBox>(root, "live-message-edit-input-" + id)?.Text}; error={Text(Find<FrameworkElement>(root, "live-message-action-error-" + id))}");
    }

    private static async Task OpenAction(FrameworkElement root, string id, string action)
    {
        await Click(root, "live-message-actions-" + id);
        await Click(root, "live-message-" + action + "-" + id);
        if (action == "edit") await Until("edit input " + id, () => Find<TextBox>(root, "live-message-edit-input-" + id) is { IsLoaded: true });
        else if (action == "choose-reaction") await Until("emoji input " + id, () => Find<TextBox>(root, "live-message-emoji-input-" + id) is { IsLoaded: true });
        else if (action == "delete") await Until("delete confirmation " + id, () => Find<Button>(root, "live-message-confirm-delete-" + id) is { IsLoaded: true });
    }

    private static async Task Click(FrameworkElement root, string id)
    {
        await Until("button " + id, () => Find<Button>(root, id) is { IsLoaded: true, IsEnabled: true });
        var button = Find<Button>(root, id)!;
        if (new ButtonAutomationPeer(button).GetPattern(PatternInterface.Invoke) is not IInvokeProvider invoke)
            throw new InvalidOperationException("Native button did not expose Invoke: " + id);
        invoke.Invoke();
        await Task.Delay(30);
    }

    private static async Task Until(string step, Func<bool> condition, Func<string>? detail = null)
    {
        for (int attempt = 0; attempt < 400; attempt++)
        {
            try { if (condition()) return; } catch (KeyNotFoundException) { }
            await Task.Delay(25);
        }
        throw new InvalidOperationException("Conversation UI fixture did not settle: " + step + (detail is null ? "" : "; " + detail()));
    }

    private static T? Find<T>(DependencyObject? parent, string id) where T : FrameworkElement
    {
        if (parent is null || parent is UIElement { Visibility: not Visibility.Visible }) return null;
        if (parent is T element && AutomationProperties.GetAutomationId(element) == id) return element;
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++)
            if (Find<T>(VisualTreeHelper.GetChild(parent, index), id) is { } found) return found;
        return null;
    }

    private static string Text(DependencyObject? parent)
    {
        if (parent is null) return "";
        string text = parent is TextBlock block ? block.Text : "";
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++) text += " " + Text(VisualTreeHelper.GetChild(parent, index));
        return text;
    }
    private static void Require(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
