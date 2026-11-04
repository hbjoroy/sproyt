using System.Text.Json;
using Sproyt.WindowsProbe;

await Probe.RunAsync(args.Single());

internal static partial class Probe
{
    internal const string User="11111111-1111-4111-8111-111111111111",OtherUser="22222222-2222-4222-8222-222222222222",
        Channel="33333333-3333-4333-8333-333333333333",OtherChannel="44444444-4444-4444-8444-444444444444";
    private static void Check(bool value,string message){if(!value)throw new InvalidOperationException(message);}
    internal static async Task RunAsync(string path)
    {
        foreach(var (phase,user,stamp) in new[]{(NativeAuthPhase.Offline,(string?)User,(string?)"1:family-a:2"),
            (NativeAuthPhase.SignedOut,(string?)null,(string?)null),(NativeAuthPhase.Active,(string?)OtherUser,(string?)"2:family-b:1"),
            (NativeAuthPhase.Active,(string?)User,(string?)"2:family-new:1")})
        {
            var app=new LiveApplication();await app.OpenForProbeAsync();var target=app.TargetForProbe();using var stream=new HeldStream();
            var staging=app.AddUploadStreamAsync(target,stream,"held.txt","text/plain");await stream.Entered.Task.WaitAsync(TimeSpan.FromSeconds(3));
            using var waitingStream=new HeldStream();var waiting=app.AddUploadStreamAsync(target,waitingStream,"waiting.txt","text/plain");
            app.ScopeForProbe(phase,user,stamp);
            await Task.WhenAll(staging,waiting).WaitAsync(TimeSpan.FromSeconds(3));
            Check(stream.Cancelled&&!waitingStream.Entered.Task.IsCompleted,"Scope loss did not cancel active staging and admission wait");
            await app.CloseForProbeAsync().WaitAsync(TimeSpan.FromSeconds(3));
        }
        {
            var app=new LiveApplication();await app.OpenForProbeAsync();var target=app.TargetForProbe();using var stream=new HeldStream();
            var staging=app.AddUploadStreamAsync(target,stream,"preserved.txt","text/plain");await stream.Entered.Task.WaitAsync(TimeSpan.FromSeconds(3));
            app.ScopeForProbe(NativeAuthPhase.Active,User,"1:family-a:99");app.NavigateForProbe();
            Check(!stream.Cancelled&&!staging.IsCompleted,"Rotation or navigation cancelled an admitted same-family source");
            stream.Release.TrySetResult();await staging.WaitAsync(TimeSpan.FromSeconds(3));
            Check(app.JobsForProbe().Jobs.Single().Target==target,"Picker/navigation retargeted the original upload");
            await app.CloseForProbeAsync();
        }
        {
            var app=new LiveApplication();await app.OpenForProbeAsync();using var stream=new HeldStream();
            var staging=app.AddUploadStreamAsync(app.TargetForProbe(),stream,"closing.txt","text/plain");await stream.Entered.Task.WaitAsync(TimeSpan.FromSeconds(3));
            await app.CloseForProbeAsync().WaitAsync(TimeSpan.FromSeconds(3));await staging.WaitAsync(TimeSpan.FromSeconds(3));
            Check(stream.Cancelled,"Close waited for a source that was never cancelled");
        }
        {
            // Suspend A outside the auth gate inside a real staging-cancellation
            // callback, then register B. A must not later overwrite B's queue.
            var app=new LiveApplication();await app.OpenForProbeAsync();using var stream=new HeldStream{BlockCancellation=true};
            var staging=app.AddUploadStreamAsync(app.TargetForProbe(),stream,"race.txt","text/plain");await stream.Entered.Task.WaitAsync(TimeSpan.FromSeconds(3));
            var oldBind=Task.Run(()=>app.ScopeForProbe(NativeAuthPhase.Active,User,"2:family-intermediate:1"));
            await stream.CancellationEntered.Task.WaitAsync(TimeSpan.FromSeconds(3));
            await Task.Run(()=>app.ScopeForProbe(NativeAuthPhase.Active,User,"3:family-current:1")).WaitAsync(TimeSpan.FromSeconds(3));
            Check(app.JobsForProbe().ActiveFamilyStamp=="3:family-current","B could not register while A cancellation was suspended");
            stream.CancellationRelease.Set();await oldBind.WaitAsync(TimeSpan.FromSeconds(3));await staging.WaitAsync(TimeSpan.FromSeconds(3));
            Check(app.JobsForProbe().ActiveFamilyStamp=="3:family-current","A stale callback overwrote B's queue scope");
            await app.CloseForProbeAsync();
        }
        {
            var app=new LiveApplication();await app.OpenForProbeAsync();
            foreach(string code in new[]{"text_too_large","capacity","workspace_busy"})Check(!await app.AttachForProbeAsync(code),"Definitive rejection became a journal failure: "+code);
            foreach(string code in new[]{"unknown_outcome","upload_pending"})
            {
                bool threw=false;try{await app.AttachForProbeAsync(code);}catch(NativeProbeException error){threw=error.Code==code;}
                Check(threw,"An uncertain handoff was discarded instead of reconciled: "+code);
            }
            await app.CloseForProbeAsync();
        }
        NativeUploadJob HeldHandoff()
        {
            string job="55555555-5555-4555-8555-555555555555",source="66666666-6666-4666-8666-666666666666",media="77777777-7777-4777-8777-777777777777";
            return new(job,new(User,"1:family-a",Channel),new(source,"held.txt","text/plain",1,new string('0',64)),job,
                UploadId:"88888888-8888-4888-8888-888888888888",NextChunk:1,UploadedBytes:1,State:NativeUploadState.Ready,
                Attachment:new(media,"held.txt","text/plain",1,new string('0',64)),Error:"Awaiting previous handoff",HandoffStarted:true);
        }
        {
            var app=new LiveApplication();var job=HeldHandoff();await app.OpenHeldHandoffForProbeAsync(job,hold:true);
            app.ScopeForProbe(NativeAuthPhase.Active,User,"2:family-b:1");
            var removing=app.RemoveUploadAsync(User,"2:family-b:1",job.Id);await app.SettlementEnteredForProbe().WaitAsync(TimeSpan.FromSeconds(3));
            Check(!removing.IsCompleted&&app.JobsForProbe().Jobs.Single().Id==job.Id&&!app.ReceiptSettledForProbe(),"Old-family handoff retired before receipt settled");
            app.ReleaseSettlementForProbe();await removing.WaitAsync(TimeSpan.FromSeconds(3));
            Check(app.ReceiptSettledForProbe()&&app.JobsForProbe().Jobs.Count==0,"Settled old-family handoff was not retired");
            await app.CloseForProbeAsync();
        }
        {
            var app=new LiveApplication();var job=HeldHandoff();await app.OpenHeldHandoffForProbeAsync(job,failure:"unknown_outcome");
            app.ScopeForProbe(NativeAuthPhase.Active,User,"2:family-b:1");await app.RemoveUploadAsync(User,"2:family-b:1",job.Id);
            Check(!app.ReceiptSettledForProbe()&&app.JobsForProbe().Jobs.Single().Id==job.Id,"Uncertain local settlement discarded its journal record");
            await app.CloseForProbeAsync();
        }
        {
            var app=new LiveApplication();var job=HeldHandoff();await app.OpenHeldHandoffForProbeAsync(job,hold:true);
            app.ScopeForProbe(NativeAuthPhase.Active,User,"2:family-b:1");
            var removing=app.RemoveUploadAsync(User,"2:family-b:1",job.Id);await app.SettlementEnteredForProbe().WaitAsync(TimeSpan.FromSeconds(3));
            app.ScopeForProbe(NativeAuthPhase.Active,OtherUser,"3:other-user:1");app.ReleaseSettlementForProbe();await removing.WaitAsync(TimeSpan.FromSeconds(3));
            Check(app.JobsForProbe().Jobs.Count==0&&app.SourceRetainedForProbe(job.Source.SourceId),"Late old-account removal retired the wrong scope or exposed its metadata");
            await app.CloseForProbeAsync();
        }
        await CheckCountedAdmissionAsync();
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
        await File.WriteAllTextAsync(path,JsonSerializer.Serialize(new{passed=true,actual_application_source=true,actual_queue_source=true,synthetic_auth_disk_workspace=true,
            stage_cancelled_on_offline_logout_user_family=true,admission_wait_cancelled=true,rotation_navigation_preserve_target=true,close_cancels_before_disk_disposal=true,
            stale_scope_registration_rejected=true,cancellation_outside_auth_gate=true,definitive_capacity_keeps_retryable_ready=true,unknown_handoff_preserved=true,
            old_family_retirement_waits_for_existing_receipt=true,uncertain_settlement_retains_job=true,account_change_fences_late_retirement=true,
            counted_durable_admission=true,partial_batch_count=true,cancelled_batch_count=true,scoped_staging_errors=true,journal_failure_unconfirmed=true,
            actual_network=false,actual_disk_journal=false,actual_winui=false},new JsonSerializerOptions{WriteIndented=true}));
        Console.WriteLine("PASS actual upload application admission, staging lifetime and handoff classification");
    }
    private sealed class HeldStream : Stream
    {
        internal TaskCompletionSource Entered{get;}=new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal TaskCompletionSource Release{get;}=new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal TaskCompletionSource CancellationEntered{get;}=new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal ManualResetEventSlim CancellationRelease{get;}=new(false);
        internal bool BlockCancellation,Cancelled;
        private bool _read;
        public override bool CanRead=>true;public override bool CanSeek=>true;public override bool CanWrite=>false;public override long Length=>1;public override long Position{get;set;}
        public override async ValueTask<int> ReadAsync(Memory<byte> buffer,CancellationToken token=default)
        {
            if(_read)return 0;
            using var registration=token.Register(()=>{Cancelled=true;CancellationEntered.TrySetResult();if(BlockCancellation)CancellationRelease.Wait();Release.TrySetCanceled(token);});
            Entered.TrySetResult();await Release.Task;token.ThrowIfCancellationRequested();_read=true;buffer.Span[0]=1;return 1;
        }
        public override int Read(byte[] buffer,int offset,int count)=>throw new NotSupportedException();
        public override long Seek(long offset,SeekOrigin origin)=>throw new NotSupportedException();public override void Flush(){}public override void SetLength(long value)=>throw new NotSupportedException();public override void Write(byte[] buffer,int offset,int count)=>throw new NotSupportedException();
        protected override void Dispose(bool disposing){CancellationRelease.Set();CancellationRelease.Dispose();base.Dispose(disposing);}
    }
}
