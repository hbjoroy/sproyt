using System.Net;
using System.Text;
using System.Text.Json;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace Sproyt.WindowsProbe;

// Deliberately synthetic HTTP and authority. Never contacts the configured host.
internal sealed class NativeMediaViewFixture : IDisposable
{
    internal const string Id = "c8940bd1-9acf-4108-8e8b-499b02a57482";
    internal static string GalleryId(int index) => $"c8940bd1-9acf-4108-8e8b-{index:D12}";
    private sealed class Handler(NativeAuthConfig config, string user) : HttpMessageHandler
    {
        private long _generation;
        private static string Proof(string purpose, long generation) => Convert.ToBase64String(System.Security.Cryptography.SHA256.HashData(Encoding.UTF8.GetBytes(purpose + generation)))
            .TrimEnd('=').Replace('+', '-').Replace('/', '_');
        private static string Access(long generation) => "sproyt-access." + Proof("access", generation);
        private static string Refresh(long generation) => "sproyt-refresh." + Proof("refresh", generation);
        private static readonly string Family = new('F', 43);
        private static readonly byte[] Landscape = Png(2400,1200), Portrait = Png(1200,2400);
        private static byte[] Png(uint width,uint height)
        {
            using var stream=new InMemoryRandomAccessStream();
            var encoder=BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId,stream).AsTask().GetAwaiter().GetResult();
            byte[] pixels=new byte[checked(width*height*4)];
            for(int index=0;index<pixels.Length;index+=4)
            {pixels[index]=90;pixels[index+1]=150;pixels[index+2]=50;pixels[index+3]=255;}
            encoder.SetPixelData(BitmapPixelFormat.Bgra8,BitmapAlphaMode.Straight,width,height,96,96,pixels);
            encoder.FlushAsync().AsTask().GetAwaiter().GetResult(); stream.Seek(0);
            using var input=stream.AsStreamForRead(); using var output=new MemoryStream(); input.CopyTo(output); return output.ToArray();
        }
        internal object Grant => GrantAt(Volatile.Read(ref _generation));
        private object GrantAt(long generation) => new { scope=config.Scope, environment=config.Environment, instance_id=config.Instance,
            user_id=user, family=Family, generation, epoch=0, display_name="UI-prøve",
            access_token=Access(generation), refresh_token=Refresh(generation),
            revoke_token="sproyt-revoke."+new string('V',43), expires_at=DateTimeOffset.UtcNow.ToUnixTimeSeconds()+300 };
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            if(request.RequestUri!.Host!="native.example.invalid" || request.Headers.Contains("Cookie")) throw new InvalidOperationException("Fixture origin mismatch.");
            object payload;
            switch(request.RequestUri.AbsolutePath)
            {
                case "/api/v1/native/info": payload=new {scope=config.Scope,environment=config.Environment,instance_id=config.Instance,issuer=config.Issuer}; break;
                case "/api/v1/native/session": payload=new {user_id=user,family=Family,generation=Volatile.Read(ref _generation),epoch=0}; break;
                case "/api/v1/native/refresh":
                    using (var body = JsonDocument.Parse(request.Content!.ReadAsStringAsync(cancellationToken).GetAwaiter().GetResult()))
                    {
                        long expected = body.RootElement.GetProperty("generation").GetInt64();
                        if (request.Headers.Authorization?.Parameter != Refresh(expected)) throw new InvalidOperationException("Fixture refresh is not authorized.");
                        long actual = Interlocked.CompareExchange(ref _generation, expected + 1, expected);
                        if (actual != expected && actual != expected + 1) throw new InvalidOperationException("Fixture refresh generation mismatch.");
                        payload = GrantAt(expected + 1);
                    }
                    break;
                case var document when document == "/api/v1/media/" + LiveUiFixture.DocumentMediaId:
                    if (request.Headers.Authorization?.Parameter != Access(Volatile.Read(ref _generation))) throw new InvalidOperationException("Fixture document is not authorized.");
                    var pdf = new ByteArrayContent(Encoding.ASCII.GetBytes("%PDF-1.7\nsynthetic fixture document\n")); pdf.Headers.ContentType = new("application/pdf");
                    return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = pdf });
                case var path when path == "/api/v1/media/"+Id || path == "/api/v1/media/"+Id+"/preview" ||
                    Enumerable.Range(0,24).Any(index => path == "/api/v1/media/"+GalleryId(index) || path == "/api/v1/media/"+GalleryId(index)+"/preview"):
                    if(request.Headers.Authorization?.Parameter!=Access(Volatile.Read(ref _generation))) throw new InvalidOperationException("Fixture media is not authorized.");
                    bool gallery=path.Contains("c8940bd1-9acf-4108-8e8b-000000",StringComparison.Ordinal);
                    bool portrait=gallery && Enumerable.Range(0,12).Any(index=>path.Contains(GalleryId(index*2+1),StringComparison.Ordinal));
                    var image=new ByteArrayContent(gallery ? portrait ? Portrait : Landscape : Convert.FromBase64String("R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=="));
                    image.Headers.ContentType=new(gallery ? "image/png" : "image/gif");
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
