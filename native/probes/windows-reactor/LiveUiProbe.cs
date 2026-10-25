using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// View-only fixture, without tokens, server authentication or production data.
internal sealed class LiveUiFixture : ILiveApplication
{
    private const string User = "00000000-0000-0000-0000-000000000001";
    private string _selected = "channel-a";
    private readonly Dictionary<string, DraftView> _drafts = new() { ["channel-a"] = new("",0,0), ["channel-b"] = new("",0,0) };
    private readonly List<NativeMessage> _messages = [];
    public event Action? Changed;
    public NativeAuthSnapshot Authentication => new(NativeAuthPhase.Active, User, 1, false, false, DisplayName: "UI-prøve");
    public NativeChatSnapshot Chat => new(true, false, null,
        [new("channel-a", "Prat", "UI-prøve"), new("channel-b", "Idear", "UI-prøve")], _selected,
        _messages.Where(message => message.ChannelId == _selected).ToArray(), User);
    public WorkspaceSnapshot Workspace => new(true, false, false, false, null, null, _drafts, []);
    public LiveSnapshot Capture() => new(Authentication, Chat, Workspace, true, null);
    public Task LoginAsync() => Task.CompletedTask;
    public Task LogoutAsync() => Task.CompletedTask;
    public Task ResumeAsync() => Task.CompletedTask;
    public Task ConfirmPendingSendsAsync(string expectedUser, string expectedStamp) => Task.CompletedTask;
    public Task SelectAsync(string channel) { _selected = channel; Changed?.Invoke(); return Task.CompletedTask; }
    public void Edit(string channel, string body) { var old = _drafts[channel]; _drafts[channel] = new(body, old.Revision+1, old.Revision+1); Changed?.Invoke(); }
    public Task SendAsync(string channel)
    {
        string body = _drafts[channel].Body;
        _messages.Add(new(Guid.NewGuid().ToString(), channel, User, "UI-prøve", body, DateTimeOffset.Now, (ulong)_messages.Count+1));
        Edit(channel, ""); return Task.CompletedTask;
    }
    public ValueTask DisposeAsync() => ValueTask.CompletedTask;
}

internal static class LiveUiProbe
{
    private static int _started;
    internal static void Attach(FrameworkElement root)
    {
        if (Program.LiveUiProbeDirectory is null) return;
        root.Loaded += (_, _) => { if (Interlocked.Exchange(ref _started,1)==0) root.DispatcherQueue.TryEnqueue(() => Run(root)); };
        if (root.IsLoaded && Interlocked.Exchange(ref _started,1)==0) root.DispatcherQueue.TryEnqueue(() => Run(root));
    }
    private static async void Run(FrameworkElement root)
    {
        string folder = Program.LiveUiProbeDirectory!;
        var window = ReactorApp.PrimaryWindow!;
        window.Closed += (_,_) => File.WriteAllText(Path.Combine(folder,"live-ui-closed.txt"),"closed");
        try
        {
            await Until(() => root.ActualWidth > 500);
            var input = Find<TextBox>(root,"live-composer"); input.Text = "Eit utkast i første kanal";
            await Until(() => Program.Live!.Workspace!.Drafts["channel-a"].Body == input.Text);
            await Program.Live!.SelectAsync("channel-b");
            await Until(() => Find<TextBox>(root,"live-composer").Text == "");
            Find<TextBox>(root,"live-composer").Text = "Eit anna utkast";
            await Until(() => Program.Live.Workspace!.Drafts["channel-b"].Body == "Eit anna utkast");
            await Program.Live.SelectAsync("channel-a");
            await Until(() => Find<TextBox>(root,"live-composer").Text == "Eit utkast i første kanal");
            await LiveChatComponent.SendForProbe!();
            await Until(() => Find<TextBox>(root,"live-composer").Text == "" && Program.Live.Chat.Messages.Count == 1);
            window.SetSize(360,720);
            await Until(() => root.ActualWidth < 800 && Find<TextBox>(root,"live-composer").ActualWidth > 200);
            await Program.PrepareShutdownAsync();
            File.WriteAllText(Path.Combine(folder,"live-ui-result.json"),JsonSerializer.Serialize(new
            {passed=true,scope="live_chat_view_fixture",actualWinUi=true,channelDrafts=true,sendBinding=true,compactComposer=true,
                productionAuthentication=false,actualTransport=false,storageAcceptance=false}));
            Program.CloseAfterShutdown(window);
        }
        catch (Exception error)
        {
            File.WriteAllText(Path.Combine(folder,"live-ui-failure.txt"), error.GetType().Name + ": " + error.Message);
            try { await Program.PrepareShutdownAsync(); } catch { }
            Environment.ExitCode=1; Program.CloseAfterShutdown(window);
        }
    }
    private static async Task Until(Func<bool> condition)
    {
        for(int attempt=0;attempt<400;attempt++){ if(condition())return; await Task.Delay(25); }
        throw new InvalidOperationException("Live view fixture did not settle.");
    }
    private static T Find<T>(DependencyObject root,string id) where T:FrameworkElement
    {
        if(root is T found && AutomationProperties.GetAutomationId(found)==id)return found;
        for(int index=0;index<VisualTreeHelper.GetChildrenCount(root);index++)
        { try{return Find<T>(VisualTreeHelper.GetChild(root,index),id);}catch(KeyNotFoundException){} }
        throw new KeyNotFoundException(id);
    }
}
