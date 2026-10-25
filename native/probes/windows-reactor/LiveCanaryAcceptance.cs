using Microsoft.UI.Xaml;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Explicit local diagnostic entry. It reads real channels but never posts a message.
internal static class LiveCanaryAcceptance
{
    private static int _started;
    internal static void Attach(FrameworkElement root)
    {
        if (Program.CanaryAcceptanceDirectory is null || Interlocked.Exchange(ref _started,1) != 0) return;
        root.DispatcherQueue.TryEnqueue(() => Run());
    }
    private static async void Run()
    {
        string folder = Program.CanaryAcceptanceDirectory!;
        Directory.CreateDirectory(folder);
        try
        {
            var app = Program.Live!;
            if (app.Authentication.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline)) await app.LoginAsync();
            else await app.ResumeAsync();
            LiveSnapshot snapshot = app.Capture();
            for (int attempt=0; attempt<600; attempt++)
            {
                snapshot = app.Capture();
                if (snapshot.ShowConversation && snapshot.Chat.Connected && !snapshot.Chat.Loading) break;
                await Task.Delay(100);
            }
            if (!snapshot.ShowConversation || !snapshot.Chat.Connected || snapshot.Authentication.UserId != snapshot.Chat.UserId)
                throw new InvalidOperationException("Real Canary chat was not ready.");
            File.WriteAllText(Path.Combine(folder,"canary-native-acceptance.json"),JsonSerializer.Serialize(new
            {
                passed=true,scope="native_windows_real_canary_read",realAuthentication=true,realWebSocket=true,
                realChannels=true,channelCount=snapshot.Chat.Channels.Count,historyCount=snapshot.Chat.Messages.Count,
                sameAccount=true,actualMessageSent=false,productionChanged=false,
            }));
        }
        catch
        {
            File.WriteAllText(Path.Combine(folder,"canary-native-acceptance-failed.txt"),
                "Canary browser authentication or live chat verification was not completed.");
        }
    }
}
