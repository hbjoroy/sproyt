using System.Net;
using System.Text;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Deliberately synthetic HTTP and authority. Never contacts the configured host.
internal sealed class NativeMediaViewFixture : IDisposable
{
    internal const string Id = "c8940bd1-9acf-4108-8e8b-499b02a57482";
    private sealed class Handler(NativeAuthConfig config, string user) : HttpMessageHandler
    {
        private static readonly string Family = new('F', 43);
        internal object Grant => new { scope=config.Scope, environment=config.Environment, instance_id=config.Instance,
            user_id=user, family=Family, generation=0, epoch=0, display_name="UI-prøve",
            access_token="sproyt-access."+new string('A',43), refresh_token="sproyt-refresh."+new string('R',43),
            revoke_token="sproyt-revoke."+new string('V',43), expires_at=DateTimeOffset.UtcNow.ToUnixTimeSeconds()+120 };
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            if(request.RequestUri!.Host!="native.example.invalid" || request.Headers.Contains("Cookie")) throw new InvalidOperationException("Fixture origin mismatch.");
            object payload;
            switch(request.RequestUri.AbsolutePath)
            {
                case "/api/v1/native/info": payload=new {scope=config.Scope,environment=config.Environment,instance_id=config.Instance,issuer=config.Issuer}; break;
                case "/api/v1/native/session": payload=new {user_id=user,family=Family,generation=0,epoch=0}; break;
                case "/api/v1/media/"+Id:
                case "/api/v1/media/"+Id+"/preview":
                    if(request.Headers.Authorization?.Parameter!="sproyt-access."+new string('A',43)) throw new InvalidOperationException("Fixture media is not authorized.");
                    var image=new ByteArrayContent(Convert.FromBase64String("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=="));
                    image.Headers.ContentType=new("image/gif");
                    return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK){Content=image});
                default: throw new InvalidOperationException("Unexpected fixture route.");
            }
            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK){Content=new StringContent(JsonSerializer.Serialize(payload),Encoding.UTF8,"application/json")});
        }
    }
    internal NativeAuthSession Owner { get; }
    internal NativeMediaLibrary Library { get; }
    internal NativeMediaViewFixture(string user)
    {
        string directory=Path.Combine(Program.LiveUiProbeDirectory!,"synthetic-media");
        var config=new NativeAuthConfig("https://native.example.invalid",new string('I',43),"https://identity.example.invalid/", "", "sproyt_native_v1","canary");
        var handler=new Handler(config,user);
        Owner=NativeAuthSession.Open(Path.Combine(directory,"owner"),config,handler:handler);
        long ticket=Owner.BeginLoginAsync().GetAwaiter().GetResult();
        Owner.CompleteLoginAsync(ticket,JsonSerializer.Serialize(new {user_id=user,native_session=handler.Grant})).GetAwaiter().GetResult();
        Library=new(Owner,directory);
    }
    public void Dispose(){Library.Dispose();Owner.Dispose();}
}
