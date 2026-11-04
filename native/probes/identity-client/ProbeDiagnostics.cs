using System.Diagnostics;
using System.Text;

// Only code-owned labels and source line numbers are permitted on stderr.
// Never format an exception, request, child output, runtime path or argument.
internal static class ProbeDiagnostics
{
    internal enum Stage
    {
        Command, HttpListener, HttpBrokerLaunch, HttpBrokerReady, HttpRequests,
        SessionBrokerLaunch, SessionBrokerReady, SessionRequests, SessionOwner,
        SessionOffline, SessionRefreshRace, SessionLateLogin, SessionChildCheckpoint
    }
    internal static Stage Phase { get; set; }

    internal static async Task<string> ReadReadyAsync(StreamReader reader)
    {
        using var stop = new CancellationTokenSource();
        async Task<string> ReadLine()
        {
            var line = new StringBuilder();
            var unit = new char[1];
            while (line.Length <= 4096)
            {
                if (await reader.ReadAsync(unit.AsMemory(), stop.Token) == 0) throw new EndOfStreamException();
                if (unit[0] == '\n') return line.ToString();
                line.Append(unit[0]);
            }
            throw new InvalidDataException();
        }
        var pending = ReadLine();
        try { return await pending.WaitAsync(TimeSpan.FromSeconds(20)); }
        catch (TimeoutException)
        {
            // Keep the 20-second acceptance deadline. Observe for at most five
            // more seconds solely to distinguish a slow ready line from silence.
            string observation;
            try
            {
                string late = await pending.WaitAsync(TimeSpan.FromSeconds(5));
                observation = late.StartsWith("N01_READY ", StringComparison.Ordinal) ? "late-ready" : "unexpected-prefix";
            }
            catch (TimeoutException) { observation = "still-waiting"; }
            catch (Exception) { observation = "read-ended"; }
            Console.Error.WriteLine($"Identity broker readiness phase={Phase} observation={observation}");
            throw;
        }
        finally { stop.Cancel(); }
    }

    internal static void Report(Exception error)
    {
        // Source filenames come from the allowlist, not an arbitrary PDB path.
        var sites = new List<string>();
        foreach (var frame in new StackTrace(error, true).GetFrames())
        {
            if (frame.GetMethod()?.Module.Assembly != typeof(Program).Assembly || frame.GetFileLineNumber() == 0) continue;
            var file = Path.GetFileName(frame.GetFileName());
            if (file is not ("Program.cs" or "NativeSessionProbe.cs" or "ProbeDiagnostics.cs" or "NativeAuthSession.cs"
                or "WindowsProtectedJournal.cs" or "DesktopHandoffProbe.cs" or "DesktopHandoffClient.cs")) continue;
            sites.Add($"{file}:{frame.GetFileLineNumber()}");
            if (sites.Count == 3) break;
        }
        // GetType().Name is compiled type metadata and contains no instance data.
        Console.Error.WriteLine($"FAIL identity probe phase={Phase} type={error.GetType().Name} site={string.Join(",", sites)}");
    }
}
