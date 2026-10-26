using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Synthetic HTTP/view fixture, without live grants or production data.
internal sealed class LiveUiFixture : ILiveApplication
{
    private const string User = "00000000-0000-0000-0000-000000000001";
    private string _selected = "channel-a";
    private readonly Dictionary<string, DraftView> _drafts = new() { ["channel-a"] = new("",0,0), ["channel-b"] = new("",0,0) };
    private readonly List<NativeMessage> _messages = [];
    private readonly NativeMediaViewFixture _mediaFixture = new(User);
    public event Action? Changed;
    public NativeAuthSnapshot Authentication => _mediaFixture.Owner.Snapshot;
    public NativeChatSnapshot Chat => new(true, false, null,
        [new("channel-a", "Prat", "UI-prøve", "fixture-circle"), new("channel-b", "Idear", "UI-prøve", "fixture-circle")], _selected,
        _messages.Where(message => message.ChannelId == _selected).ToArray(), User);
    public WorkspaceSnapshot Workspace => new(true, false, false, false, null, null, _drafts, []);
    public LiveSnapshot Capture() => new(Authentication, Chat, Workspace, true, null);
    public Task LoginAsync() => Task.CompletedTask;
    public NativeMediaLibrary? Media => _mediaFixture.Library;
    internal void AddMedia()
    {
        _messages.Add(new(Guid.NewGuid().ToString(),_selected,User,"UI-prøve","Her er eit bilete\n[[media:"+NativeMediaViewFixture.Id+"|image/gif|probe.gif]]\n[[invite:"+new string('A',43)+"]]",DateTimeOffset.Now,2));
        Changed?.Invoke();
    }
    internal void RenameMedia() { _messages[^1]=_messages[^1] with {Body=_messages[^1].Body.Replace("probe.gif","renamed.gif",StringComparison.Ordinal)}; Changed?.Invoke(); }
    internal void AddGallery()
    {
        _messages.Clear();
        for (int index=0;index<24;index++)
            _messages.Add(new(Guid.NewGuid().ToString(),_selected,User,"UI-prøve",
                "[[media:"+NativeMediaViewFixture.GalleryId(index)+"|image/gif|gallery-"+index+".gif]]",DateTimeOffset.Now,(ulong)index+1));
        Changed?.Invoke();
    }
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
    public ValueTask DisposeAsync() { _mediaFixture.Dispose(); return ValueTask.CompletedTask; }
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
            ((LiveUiFixture)Program.Live).AddMedia();
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.Id).Source is not null);
            if(NativeMessageView.ScopeFor(Program.Live.Media,Program.Live.Authentication with { UserId="ffffffff-ffff-ffff-ffff-ffffffffffff" }) is not null ||
                NativeMessageView.ScopeFor(Program.Live.Media,Program.Live.Authentication with { SessionStamp="999:another-family:0" }) is not null)
                throw new InvalidOperationException("Media presentation accepted the wrong viewer snapshot.");
            ((LiveUiFixture)Program.Live).RenameMedia();
            await Until(() => Find<TextBlock>(root,"native-media-name-"+NativeMediaViewFixture.Id).Text=="renamed.gif" && Find<Image>(root,"native-image-"+NativeMediaViewFixture.Id).Source is not null);
            if(Find<TextBlock>(root,"live-group-circle:fixture-circle").Text!="UI-PRØVE") throw new InvalidOperationException("Circle heading missing.");
            _=Find<StackPanel>(root,"native-content-invite");
            var held = new List<NativePreviewLease>();
            try
            {
                for(int index=0;index<7;index++) { var lease=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None); lease.Decoded(); held.Add(lease); }
                bool bounded=false; try { using var extra=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None); } catch(NativePreviewCapacityException) { bounded=true; }
                if(!bounded)throw new InvalidOperationException("Preview bitmap admission is unbounded.");
                held[0].Dispose();
                using var replacement=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
            }
            finally { foreach(var lease in held)lease.Dispose(); }
            var decodingA=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
            var decodingB=await Program.Live.Media.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
            try
            {
                if(!decodingA.BeginDecode() || !decodingB.BeginDecode())throw new InvalidOperationException("Decode fixture admission failed.");
                decodingA.Dispose(); decodingB.Dispose();
                var queued=Program.Live.Media.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
                await Task.Delay(50);
                if(queued.IsCompleted)throw new InvalidOperationException("Unmount admitted too many native decoders.");
                decodingA.Decoded();
                using var admitted=await queued.WaitAsync(TimeSpan.FromSeconds(2));
            }
            finally { decodingA.Decoded(); decodingB.Decoded(); decodingA.Dispose(); decodingB.Dispose(); }
            string temporary;
            using(var original=await Program.Live.Media!.OriginalAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),null,CancellationToken.None))
            { temporary=original.Path; if(!File.Exists(temporary) || original.ContentType!="image/gif")throw new InvalidOperationException("Original fixture failed."); }
            if(File.Exists(temporary))throw new InvalidOperationException("Media file lease was not cleaned.");
            // Realized rows can outlive their viewport. More than eight attachments
            // must not let cached rows starve the visible images of bitmap slots.
            ((LiveUiFixture)Program.Live).AddGallery();
            var history=Find<ScrollViewer>(root,"live-history");
            await Until(() =>
            {
                root.UpdateLayout(); history.ChangeView(null,history.ScrollableHeight,null,true);
                return Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23)).Source is not null;
            });
            double height=history.ScrollableHeight;
            history.ChangeView(null,0,null,true);
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(0)).Source is not null);
            await Until(() => PreviewReleased(root,NativeMediaViewFixture.GalleryId(23)));
            if(Math.Abs(history.ScrollableHeight-height)>2)throw new InvalidOperationException("Releasing previews changed message geometry.");
            // Rapidly leave/re-enter while admission/decoding can still be pending.
            history.ChangeView(null,history.ScrollableHeight,null,true); await Task.Delay(20);
            history.ChangeView(null,0,null,true); await Task.Delay(20);
            history.ChangeView(null,history.ScrollableHeight,null,true);
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23)).Source is not null);
            await Until(() => PreviewReleased(root,NativeMediaViewFixture.GalleryId(0)));
            window.SetSize(360,720);
            await Until(() => root.ActualWidth < 800 && Find<TextBox>(root,"live-composer").ActualWidth > 200);
            await Program.PrepareShutdownAsync();
            File.WriteAllText(Path.Combine(folder,"live-ui-result.json"),JsonSerializer.Serialize(new
            {passed=true,scope="live_chat_view_fixture",actualWinUi=true,channelDrafts=true,sendBinding=true,compactComposer=true,circleGroups=true,decodedMediaPreview=true,boundedPreviewLeases=true,viewportPreviewGallery=24,viewportRelease=true,stablePreviewGeometry=true,rapidPreviewReentry=true,macroCard=true,temporaryOriginalCleanup=true,
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
        for(int attempt=0;attempt<400;attempt++)
        {
            try { if(condition())return; } catch(KeyNotFoundException) { /* The next native render may create the control. */ }
            await Task.Delay(25);
        }
        throw new InvalidOperationException("Live view fixture did not settle.");
    }
    private static T Find<T>(DependencyObject root,string id) where T:FrameworkElement
    {
        if(root is T found && AutomationProperties.GetAutomationId(found)==id)return found;
        for(int index=0;index<VisualTreeHelper.GetChildrenCount(root);index++)
        { try{return Find<T>(VisualTreeHelper.GetChild(root,index),id);}catch(KeyNotFoundException){} }
        throw new KeyNotFoundException(id);
    }
    private static bool PreviewReleased(DependencyObject root,string id)
    {
        try { return Find<Image>(root,"native-image-"+id).Source is null; }
        catch(KeyNotFoundException) { return true; } // Fully recycled is also released.
    }
}
