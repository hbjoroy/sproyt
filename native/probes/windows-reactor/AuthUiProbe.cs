using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

internal static class AuthUiProbe
{
    private static int _started;
    private const string Draft = "Dette utkastet ventar gjennom innlogging og omstart.";
    internal static void Attach(FrameworkElement root)
    {
        if (Program.SessionUiProbeDirectory is null) return;
        root.Loaded += (_, _) => { if (Interlocked.Exchange(ref _started, 1) == 0) root.DispatcherQueue.TryEnqueue(() => Run(root)); };
        if (root.IsLoaded && Interlocked.Exchange(ref _started, 1) == 0) root.DispatcherQueue.TryEnqueue(() => Run(root));
    }

    private static async void Run(FrameworkElement root)
    {
        string name = Program.SessionUiCancellation ? "session-ui-cancel" : Program.SessionUiResume ? "session-ui-resume" : "session-ui";
        string folder = Program.SessionUiProbeDirectory!;
        var window = ReactorApp.PrimaryWindow!;
        window.Closed += (_, _) => File.WriteAllText(Path.Combine(folder, name + "-closed.txt"), "closed");
        try
        {
            var authentication = Program.Authentication ?? throw new InvalidOperationException("Session UI owner missing.");
            await Until(() => root.ActualWidth > 400 && Program.Workspace.State.Ready);
            var editor = Find<TextBox>(root, "composer");
            if (Program.SessionUiCancellation)
            {
                var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                authentication.BeforeLoginAdmissionForProbe = async () => { entered.TrySetResult(); await release.Task; };
                var login = authentication.LoginAsync();
                await entered.Task.WaitAsync(TimeSpan.FromSeconds(10));
                await authentication.LogoutAsync();
                release.TrySetResult();
                await login;
                Check(authentication.Snapshot.Phase == NativeAuthPhase.SignedOut, "Delayed login reopened a logged-out owner.");
            }
            else if (Program.SessionUiResume)
            {
                await Until(() => authentication.Snapshot.Phase == NativeAuthPhase.Active);
                Check(editor.Text == Draft, "Restart did not restore the account draft.");
                await authentication.LogoutAsync();
                await Until(() => authentication.Snapshot.Phase == NativeAuthPhase.SignedOut);
            }
            else
            {
                Check(authentication.Snapshot.Phase == NativeAuthPhase.SignedOut, "Fresh UI was not signed out.");
                editor.Text = Draft;
                await Until(() => Saved());
                await authentication.LoginAsync();
                await Until(() => authentication.Snapshot.Phase == NativeAuthPhase.Active);
                await Until(() => Find<TextBlock>(root, "authentication-status").Text.Contains("Alice", StringComparison.Ordinal));
                Check(editor.Text == Draft, "Login replaced the draft.");
                await authentication.LogoutAsync();
                await Until(() => authentication.Snapshot.Phase == NativeAuthPhase.SignedOut && !authentication.Snapshot.PendingRevocation);
                Check(editor.Text == Draft, "Logout removed the draft.");
                await Until(() => !Find<Button>(root, "queue-message").IsEnabled);
                await authentication.LoginAsync();
                await Until(() => authentication.Snapshot.Phase == NativeAuthPhase.Active);
            }
            if (!Program.SessionUiCancellation) Check(editor.Text == Draft && Saved(), "Account draft was not durable.");
            await Program.PrepareShutdownAsync();
            File.WriteAllText(Path.Combine(folder, name + "-result.json"), JsonSerializer.Serialize(new
            {
                passed = true, syntheticSession = true, actualWinUi = true, accountScopedStore = true,
                draftPreserved = true, nativeProcessRestart = Program.SessionUiResume,
                simulatedBrowser = true, productionAuthentication = false,
                delayedLoginAdmissionFenced = Program.SessionUiCancellation,
            }));
            Program.CloseAfterShutdown(window);
        }
        catch
        {
            File.WriteAllText(Path.Combine(folder, name + "-failure.txt"), "Native session UI verification failed.");
            try { await Program.PrepareShutdownAsync(); } catch { }
            Program.CloseAfterShutdown(window);
            Environment.ExitCode = 1;
        }
    }
    private static bool Saved() => Program.Workspace.State.Drafts["prat"] is var draft && draft.Body == Draft && draft.Revision == draft.SavedRevision;
    private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
    private static async Task Until(Func<bool> condition)
    {
        for (int attempt = 0; attempt < 400; attempt++) { if (condition()) return; await Task.Delay(25); }
        throw new InvalidOperationException("Native session UI verification timed out.");
    }
    private static T Find<T>(DependencyObject root, string id) where T : FrameworkElement
    {
        if (root is T found && AutomationProperties.GetAutomationId(found) == id) return found;
        for (int index = 0; index < VisualTreeHelper.GetChildrenCount(root); index++)
        {
            try { return Find<T>(VisualTreeHelper.GetChild(root, index), id); } catch (KeyNotFoundException) { }
        }
        throw new KeyNotFoundException();
    }
}
