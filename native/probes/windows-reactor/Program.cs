using Microsoft.UI.Reactor;
using System.Runtime.InteropServices;

namespace Sproyt.WindowsProbe;

internal static class Program
{
    internal static ChatWorkspace Workspace { get; private set; } = null!;
    internal static NativeAuthentication? Authentication { get; private set; }
    internal static string DataDirectory { get; private set; } = "";
    internal static string? UiProbeDirectory { get; private set; }
    internal static bool UiResume { get; private set; }
    internal static string? SessionUiProbeDirectory { get; private set; }
    internal static bool SessionUiResume { get; private set; }
    internal static bool SessionUiCancellation { get; private set; }
    internal static bool KeyboardViewportAvailable { get; private set; }
    internal static KeyboardViewport? Keyboard { get; private set; }
    internal static string LibraryPath => Path.Combine(AppContext.BaseDirectory, "sproyt_native_probe.dll");
    private static bool _closing;
    private static bool _preparingShutdown;
    private static bool _allowFinalClose;
    internal static bool CloseAdmissionClosed => _closing || _preparingShutdown;

    [STAThread]
    private static void Main(string[] args)
    {
        int diagnosticCount = 0;
        void Log(Exception exception) => File.AppendAllText(Path.Combine(AppContext.BaseDirectory, "startup.log"), DateTime.UtcNow.ToString("O") + " " + exception + Environment.NewLine);
        AppDomain.CurrentDomain.FirstChanceException += (_, args) =>
        {
            if (args.Exception is COMException && Interlocked.Increment(ref diagnosticCount) <= 12) Log(args.Exception);
        };
        AppDomain.CurrentDomain.UnhandledException += (_, args) => { if (args.ExceptionObject is Exception exception) Log(exception); };
        ReactorApplication.OnUnhandledException = exception => { Log(exception); return false; };
        try
        {
            if (args.Length == 4 && args[0] == "--desktop-child")
            {
                Environment.ExitCode = DesktopHandoffClient.Run(args[1], args[2], args[3]).GetAwaiter().GetResult();
                return;
            }
            string library = LibraryPath;
            if (args.Length == 2 && args[0] == "--self-test")
            {
                string testRoot = Path.GetFullPath(args[1]);
                Directory.CreateDirectory(testRoot);
                string sessionResult = SessionProbe.RunAsync(Path.Combine(testRoot, $"session-{Guid.NewGuid():N}.sqlite"), library).GetAwaiter().GetResult();
                string workspaceResult = WorkspaceProbe.RunAsync(Path.Combine(testRoot, $"workspace-{Guid.NewGuid():N}.sqlite"), library).GetAwaiter().GetResult();
                File.WriteAllText(Path.Combine(testRoot, "core-result.txt"), sessionResult + Environment.NewLine + workspaceResult);
                return;
            }
            UiResume = args.Length == 2 && args[0] == "--ui-resume";
            bool sessionMode = args.Length == 2 && args[0] is "--session-config" or "--session-ui-smoke" or "--session-ui-resume" or "--session-ui-cancel";
            SessionUiResume = args.Length == 2 && args[0] == "--session-ui-resume";
            SessionUiCancellation = args.Length == 2 && args[0] == "--session-ui-cancel";
            SessionUiProbeDirectory = args.Length == 2 && args[0] is "--session-ui-smoke" or "--session-ui-resume" or "--session-ui-cancel"
                ? Path.GetDirectoryName(Path.GetFullPath(args[1])) : null;
            UiProbeDirectory = args.Length == 2 && args[0] is "--ui-smoke" or "--ui-resume" ? Path.GetFullPath(args[1]) : null;
            DataDirectory = UiProbeDirectory is not null ? Path.Combine(UiProbeDirectory, "data") : args.Length == 2 && args[0] == "--data-dir"
                ? Path.GetFullPath(args[1])
                : sessionMode
                ? Path.Combine(Path.GetDirectoryName(Path.GetFullPath(args[1]))!, "windows-data")
                : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Sproyt", "WindowsProbe");
            Directory.CreateDirectory(DataDirectory);
            if (sessionMode)
                Authentication = new(args[1], Path.Combine(DataDirectory, "authentication"));
            string database = Authentication is null ? Path.Combine(DataDirectory, "demo-v1.sqlite")
                : Path.Combine(DataDirectory, "account-" + Authentication.UserId + ".sqlite");
            var session = NativeSession.OpenAsync(database, library, Authentication?.UserId ?? NativeSession.FixtureAccount).GetAwaiter().GetResult();
            Workspace = new(new NativeWorkspaceStoreAdapter(session), ["prat", "idear"]);
            Workspace.InitializeAsync().GetAwaiter().GetResult();
            ReactorApp.WindowOpened += (_, window) =>
            {
                var keyboard = Keyboard = new KeyboardViewport(window);
                KeyboardViewportAvailable = keyboard.Available;
                keyboard.Attach(PilotComponent.RootForClose as Microsoft.UI.Xaml.Controls.Grid);
                window.NativeWindow.Activated += (_, activated) =>
                {
                    if (!CloseAdmissionClosed && activated.WindowActivationState != Microsoft.UI.Xaml.WindowActivationState.Deactivated
                        && Authentication is not null) _ = Authentication.ResumeAsync();
                };
                if (Authentication is not null) _ = Authentication.ResumeAsync();
                window.Closed += (_, _) => { keyboard.Dispose(); Keyboard = null; };
                window.Closing += (_, closing) =>
                {
                    if (_allowFinalClose) return;
                    closing.Cancel = true;
                    RequestClose(window);
                };
            };
            ReactorApp.Run<PilotComponent>(new WindowSpec
            {
                Title = "Sprøyt · Windows-prøve", Width = 1120, Height = 760, MinWidth = 360, MinHeight = 460,
            });
        }
        catch (Exception exception) { Log(exception); Environment.ExitCode = 1; }
    }

    private static async void RequestClose(ReactorWindow window)
    {
        if (_closing) return;
        _closing = true;
        try
        {
            await PrepareShutdownAsync();
            CloseAfterShutdown(window);
        }
        catch
        {
            // The coordinator keeps recoverable drafts editable, or exposes a
            // terminal read-only snapshot. The window stays open to explain it.
        }
        finally { _closing = false; PilotComponent.InvalidateForClose?.Invoke(); }
    }

    internal static async Task PrepareShutdownAsync()
    {
        _preparingShutdown = true;
        var root = PilotComponent.RootForClose;
        var editor = PilotComponent.EditorForClose;
        try
        {
            if (root is not null)
            {
                // WinUI TextChanged can be deferred after Text was changed.
                // Freeze new native input, then let already-posted binding
                // events reach the coordinator before closing its admission.
                root.IsHitTestVisible = false;
                if (editor is not null) editor.IsReadOnly = true;
                var flushed = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                if (!root.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, () => flushed.TrySetResult()))
                    throw new InvalidOperationException("The UI dispatcher closed before editor events were flushed.");
                await flushed.Task.WaitAsync(TimeSpan.FromSeconds(10));
            }
            await Workspace.ShutdownAsync();
            if (Authentication is not null) await Authentication.DisposeAsync();
        }
        catch
        {
            _preparingShutdown = false;
            if (root is not null) root.IsHitTestVisible = true;
            if (editor is not null) editor.IsReadOnly = Workspace.State.Terminal;
            throw;
        }
    }

    internal static void CloseAfterShutdown(ReactorWindow window)
    {
        _allowFinalClose = true;
        window.Close();
    }
}
