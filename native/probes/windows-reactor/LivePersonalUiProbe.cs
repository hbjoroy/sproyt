using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Drives real mounted controls with credential-free in-process projections.
// It never opens a physical picker, accesses user files or reaches a service.
internal static class LivePersonalUiProbe
{
    private const string RootOne = "conversation-root-one", ReplyOne = "conversation-reply-one";
    private const string RootDraft = "Personleg kanalutkast", ThreadDraft = "Personleg trådutkast";
    internal static async Task RunAsync(FrameworkElement root, LiveUiFixture fixture)
    {
        bool wasActive = Program.LiveWindowActive;
        try
        {
            Program.LiveWindowActive = false;
            fixture.PrepareContent("Kjelde for personleg UI-prøve");
            var personal = fixture.PersonalForProbe; personal.Reset();
            await Until("fresh personal root", () => fixture.Chat.SelectedChannel == "channel-a" &&
                Find<TextBox>(root, "live-composer") is { IsLoaded: true, Text: "" });
            var editor = Find<TextBox>(root, "live-composer")!;
            editor.Text = RootDraft; editor.Focus(FocusState.Programmatic); editor.Select(4, 5);
            await Until("root draft persisted", () => fixture.Workspace.Drafts["channel-a"].ComposerText == RootDraft);
            var retainedSend = LiveChatComponent.SendForProbe!;
            var retainedDrop = LiveChatComponent.RootDropForProbe!;
            var auth = fixture.Authentication; var nav = fixture.Capture().Navigation!.Current!;
            var rootScope = new NativeMessageActionScope(auth.UserId!, auth.SessionStamp!, "channel-a", nav.Generation);
            int accepted = fixture.AcceptedMessageCount;

            await Open(root, NativePersonalSurfaceKind.Profile);
            await Until("own profile loaded", () => Find<TextBox>(root, "live-profile-name")?.Text == personal.OwnProfile.DisplayName &&
                Find<Button>(root, "live-profile-save-name") is { IsEnabled: true });
            Require(editor.IsLoaded && ReferenceEquals(editor, Find<TextBox>(root, "live-composer", includeHidden: true)) &&
                Find<TextBox>(root, "live-composer") is null, "Opening profile remounted the composer or exposed it through the covered tree.");
            await retainedSend();
            Require(!await retainedDrop.DropForProbeAsync(() => Task.FromResult<IReadOnlyList<NativeFileDropItem>>([new("C:/synthetic-personal/covered.pdf")])),
                "A retained root drop was admitted through the personal surface.");
            Require(fixture.CaptureUploadTarget(rootScope) is null && fixture.AcceptedMessageCount == accepted &&
                fixture.Workspace.Drafts["channel-a"].ComposerText == RootDraft, "Covered surface admitted a new upload or sent chat.");
            int reads = fixture.ReadReports.Count;
            Program.LiveWindowActive = true; LiveChatComponent.TimelineForProbe?.ObserveSoon();
            await Task.Delay(250);
            Require(fixture.ReadReports.Count == reads, "A covered conversation emitted a read receipt.");
            Program.LiveWindowActive = false;

            Find<TextBox>(root, "live-profile-name")!.Text = "Native UI-namn";
            Find<TextBox>(root, "live-profile-status-text")!.Text = "Prøver WinUI";
            Find<TextBox>(root, "live-profile-status-emoji")!.Text = "🌿";
            Require(personal.ProfileWrites == 0 && personal.StatusWrites == 0, "Typing profile fields automatically saved them.");
            await Click(root, "live-profile-save-name");
            await Until("canonical own name", () => personal.OwnProfile.DisplayName == "Native UI-namn" &&
                Find<TextBlock>(root, "live-personal-display-name")?.Text == "Native UI-namn");
            Require(Find<TextBox>(root, "live-profile-status-text")?.Text == "Prøver WinUI", "Name acknowledgement cleared independent status draft.");
            await Click(root, "live-profile-save-status");
            await Until("explicit status acknowledgement", () => personal.OwnProfile.StatusText == "Prøver WinUI" && personal.OwnProfile.StatusEmoji == "🌿");
            Find<TextBox>(root, "live-profile-name")!.Text = "Bevart profilutkast";
            await Close(root);
            await Until("same root composer returned", () => ReferenceEquals(editor, Find<TextBox>(root, "live-composer")) && editor.Text == RootDraft);
            Require(editor.SelectionStart == 4 && editor.SelectionLength == 5 && Find<TextBlock>(root, "live-personal-display-name")?.Text == "Native UI-namn",
                "Closing profile lost selection or reverted canonical name to OIDC metadata.");
            await Open(root, NativePersonalSurfaceKind.Profile);
            await Until("private profile draft reopened", () => Find<TextBox>(root, "live-profile-name")?.Text == "Bevart profilutkast" &&
                Find<Button>(root, "live-profile-save-name") is { IsEnabled: true });
            await Click(root, "live-profile-reload");
            await Until("profile read completed", () => Find<Button>(root, "live-profile-reload") is { IsEnabled: true });
            Require(Find<TextBox>(root, "live-profile-name")?.Text == "Bevart profilutkast" && personal.ProfileWrites == 1, "Read/reopen replaced dirty fields or replayed profile write.");
            personal.FailNextProfileAfterCommit = true;
            Find<TextBox>(root, "live-profile-name")!.Text = "Stadfesta UI-namn";
            await Click(root, "live-profile-save-name");
            await Until("uncertain profile offers readback", () => Find<Button>(root, "live-profile-readback") is { IsEnabled: true });
            Require(personal.ProfileWrites == 2, "Unknown profile outcome was replayed.");
            await Click(root, "live-profile-readback");
            await Until("profile readback canonical header", () => Find<TextBlock>(root, "live-personal-display-name")?.Text == "Stadfesta UI-namn" &&
                Find<Button>(root, "live-profile-readback") is null);
            Require(personal.ProfileWrites == 2, "Readback wrote the name again.");
            await Click(root, "live-profile-clear-status");
            await Until("status cleared explicitly", () => personal.OwnProfile.StatusText == "" && personal.OwnProfile.StatusEmoji == "");
            var oldClose = LiveChatComponent.ClosePersonalForProbe!;
            await Open(root, NativePersonalSurfaceKind.People);
            var peopleScope = LiveChatComponent.PersonalScopeForProbe;
            oldClose(); await Task.Delay(50);
            Require(LiveChatComponent.PersonalScopeForProbe == peopleScope && Find<FrameworkElement>(root, "live-personal-surface") is not null,
                "An old close callback uncovered a newly opened personal visit.");

            await Until("two distinct people", () => Find<Button>(root, "live-person-open-" + NativePersonalUiFixtureCommands.PersonOneId) is not null &&
                Find<Button>(root, "live-person-open-" + NativePersonalUiFixtureCommands.PersonTwoId) is not null);
            Require(Find<Button>(root, "live-person-open-" + auth.UserId) is null, "People directory exposed a self-DM action.");
            Find<TextBox>(root, "live-people-query")!.Text = "anna-to";
            await Until("handle filter", () => Find<Button>(root, "live-person-open-" + NativePersonalUiFixtureCommands.PersonOneId) is null &&
                Find<Button>(root, "live-person-open-" + NativePersonalUiFixtureCommands.PersonTwoId) is not null);
            personal.FailNextDirectAfterCommit = true;
            await Click(root, "live-person-open-" + NativePersonalUiFixtureCommands.PersonTwoId);
            await Until("unknown DM readback", () => Find<Button>(root, "live-people-readback") is { IsEnabled: true });
            Require(personal.DirectExecutions == 1 && fixture.Chat.SelectedChannel == "channel-a" && fixture.AcceptedMessageCount == accepted,
                "Unknown DM acknowledgement navigated or sent chat automatically.");
            await Click(root, "live-people-readback");
            await Until("canonical direct destination", () => fixture.Chat.Channels.Any(channel => channel.Id == fixture.Chat.SelectedChannel &&
                channel.IsDirect && channel.DirectUserId == NativePersonalUiFixtureCommands.PersonTwoId) && LiveChatComponent.PersonalScopeForProbe is null);
            Require(personal.LastDirectUserId == NativePersonalUiFixtureCommands.PersonTwoId && personal.DirectExecutions == 1 &&
                personal.ChannelRefreshes > 0 && fixture.AcceptedMessageCount == accepted, "People readback confused equal display names or replayed DM creation.");
            await Click(root, "live-personal-return");
            await Until("people return frame", () => fixture.Chat.SelectedChannel == "channel-a" && Find<TextBox>(root, "live-people-query")?.Text == "anna-to");
            await Close(root);
            Require(fixture.Workspace.Drafts["channel-a"].ComposerText == RootDraft, "DM return lost original root draft.");

            await LiveChatComponent.NavigateForProbe!(new("channel-a", ThreadId: RootOne));
            await Until("thread draft surface", () => Find<TextBox>(root, "live-thread-composer") is { IsLoaded: true });
            var threadEditor = Find<TextBox>(root, "live-thread-composer")!;
            threadEditor.Text = ThreadDraft; threadEditor.Focus(FocusState.Programmatic); threadEditor.Select(3, 4);
            string threadKey = new ComposerTarget("channel-a", RootOne).Key;
            await Until("thread draft retained", () => fixture.Workspace.Drafts[threadKey].ComposerText == ThreadDraft);
            await Open(root, NativePersonalSurfaceKind.Mentions);
            Require(threadEditor.IsLoaded && ReferenceEquals(threadEditor, Find<TextBox>(root, "live-thread-composer", includeHidden: true)) &&
                Find<TextBox>(root, "live-thread-composer") is null, "Inbox remounted or exposed the covered thread composer.");
            await Until("reply mention row", () => Find<Button>(root, "live-inbox-mention-" + ReplyOne + "-source") is not null);
            personal.FailNextMentionAfterCommit = true;
            await Click(root, "live-inbox-mention-" + RootOne + "-read");
            await Until("mention readback action", () => Find<Button>(root, "live-inbox-mention-" + RootOne + "-read-back") is { IsEnabled: true });
            await Click(root, "live-inbox-mention-" + RootOne + "-read-back");
            await Until("mention server acknowledgement", () => personal.MentionRead(RootOne) &&
                Find<Button>(root, "live-inbox-mention-" + RootOne + "-read-back") is null);
            Require(personal.MentionWrites == 1, "Mention readback repeated a mutation.");
            await Click(root, "live-inbox-mention-" + ReplyOne + "-source");
            await Until("exact reply source", () => LiveChatComponent.PersonalScopeForProbe is null && fixture.Capture().Navigation?.Current?.Target is
                { ChannelId: "channel-a", MessageId: ReplyOne, ThreadId: RootOne } && fixture.Capture().Threads?.Visit?.RootMessageId == RootOne);
            Require(fixture.Workspace.Drafts[threadKey].ComposerText == ThreadDraft, "Reply source navigation modified thread draft.");
            await Click(root, "live-personal-return");
            await Until("mention inbox returned", () => Find<Button>(root, "live-inbox-tab-tasks") is not null && LiveChatComponent.PersonalScopeForProbe is not null);
            await Click(root, "live-inbox-tab-tasks");
            string taskPrefix = "live-inbox-task-" + NativePersonalUiFixtureCommands.TaskId;
            await Until("assigned task row", () => Find<Button>(root, taskPrefix + "-toggle") is { IsEnabled: true });
            await Click(root, taskPrefix + "-toggle");
            await Until("task completed", () => personal.TaskDone(NativePersonalUiFixtureCommands.TaskId) && Find<Button>(root, taskPrefix + "-toggle") is { IsEnabled: true });
            await Click(root, taskPrefix + "-toggle");
            await Until("task reopened", () => !personal.TaskDone(NativePersonalUiFixtureCommands.TaskId));
            Require(personal.TaskWrites == 2, "Task toggle did not require an explicit acknowledged action each time.");
            await Close(root);
            await Until("thread composer returned from inbox", () => Find<TextBox>(root, "live-thread-composer")?.Text == ThreadDraft);
            Require(fixture.Workspace.Drafts["channel-a"].ComposerText == RootDraft && fixture.AcceptedMessageCount == accepted,
                "Personal feature work changed root draft or sent messages.");
            await fixture.NavigateAsync(new("channel-a"));
            await Until("final root restored", () => Find<TextBox>(root, "live-composer")?.Text == RootDraft && fixture.Capture().Threads?.Visit is null);
            if (Program.LiveUiProbeDirectory is { } directory)
                await File.WriteAllTextAsync(Path.Combine(directory, "personal-ui-result.json"), JsonSerializer.Serialize(new
                { passed = true, actualWinUi = true, profileExplicitSave = true, canonicalHeader = true, dirtyReopen = true,
                    equalNamePeople = true, directReadback = true, mentions = true, taskToggle = true, exactReplySource = true,
                    inboxReturn = true, coveredSendDropRead = true, staleCloseFence = true, rootThreadDrafts = true,
                    productionAuthentication = false, actualTransport = false, storageAcceptance = false }));
        }
        finally
        {
            LiveChatComponent.ClosePersonalForProbe?.Invoke();
            await fixture.NavigateAsync(new("channel-a")); Program.LiveWindowActive = wasActive;
        }
    }
    private static async Task Open(FrameworkElement root, NativePersonalSurfaceKind kind)
    {
        string id = kind switch { NativePersonalSurfaceKind.Profile => "live-personal-profile", NativePersonalSurfaceKind.People => "live-personal-people", _ => "live-personal-inbox" };
        if ((kind is NativePersonalSurfaceKind.Unread or NativePersonalSurfaceKind.People or NativePersonalSurfaceKind.Profile) && Find<Button>(root, id) is not null)
            await Click(root, id);
        else await LiveChatComponent.OpenPersonalForProbe!(kind);
        await Until("personal surface " + kind, () => Find<FrameworkElement>(root, "live-personal-surface") is not null && LiveChatComponent.PersonalScopeForProbe is not null);
    }
    private static async Task Close(FrameworkElement root)
    { await Click(root, "live-personal-close"); await Until("personal surface closed", () => LiveChatComponent.PersonalScopeForProbe is null); }
    private static async Task Click(FrameworkElement root, string id)
    {
        await Until("button " + id, () => Find<Button>(root, id) is { IsLoaded: true, IsEnabled: true });
        var button = Find<Button>(root, id)!;
        var peer = FrameworkElementAutomationPeer.CreatePeerForElement(button) ?? new ButtonAutomationPeer(button);
        if (peer.GetPattern(PatternInterface.Invoke) is not IInvokeProvider invoke) throw new InvalidOperationException("Missing button Invoke: " + id);
        invoke.Invoke(); await Task.Delay(30);
    }
    private static async Task Until(string name, Func<bool> condition)
    {
        for (int attempt = 0; attempt < 400; attempt++) { try { if (condition()) return; } catch (KeyNotFoundException) { } await Task.Delay(25); }
        throw new InvalidOperationException("Personal UI fixture did not settle: " + name);
    }
    private static IEnumerable<T> Descendants<T>(DependencyObject parent, bool includeHidden) where T : FrameworkElement
    {
        if (!includeHidden && parent is UIElement { Visibility: not Visibility.Visible }) yield break;
        if (parent is T typed) yield return typed;
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++)
            foreach (var child in Descendants<T>(VisualTreeHelper.GetChild(parent, index), includeHidden)) yield return child;
    }
    private static T? Find<T>(DependencyObject root, string id, bool includeHidden = false) where T : FrameworkElement =>
        Descendants<T>(root, includeHidden).FirstOrDefault(element => AutomationProperties.GetAutomationId(element) == id);
    private static void Require(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
