using System.Net;

namespace Sproyt.WindowsProbe;

// Internal CI browser simulator for the synthetic loopback provider only.
internal static class SessionLabBrowser
{
    internal static async Task FollowAsync(Uri address, Uri broker, Uri callback, CancellationToken cancellation)
    {
        using var http = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = true, UseProxy = false })
            { Timeout = TimeSpan.FromSeconds(12) };
        for (int hop = 0; hop < 10; hop++)
        {
            if (address.Scheme != "http" || address.Host != "127.0.0.1" ||
                (address.Authority != broker.Authority && address.Authority != callback.Authority))
                throw new InvalidOperationException("Unexpected synthetic browser destination.");
            using var request = new HttpRequestMessage(HttpMethod.Get, address);
            if (address.Authority == broker.Authority && address.AbsolutePath == "/provider/authorize")
                request.Headers.Add("X-Probe-Account", "alice");
            using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellation);
            if (address.Authority == callback.Authority && address.AbsolutePath == "/return" && response.StatusCode == HttpStatusCode.Found) return;
            if (response.StatusCode != HttpStatusCode.Found || response.Headers.Location is null)
                throw new InvalidOperationException("Synthetic browser did not complete.");
            address = response.Headers.Location.IsAbsoluteUri ? response.Headers.Location : new Uri(address, response.Headers.Location);
        }
        throw new InvalidOperationException("Synthetic browser hop limit.");
    }
}
