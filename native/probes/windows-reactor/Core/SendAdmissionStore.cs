namespace Sproyt.WindowsProbe;

// Durable consent for replaying this account's local outbox in one login family.
// Contains no token; independent of refresh generation and browser credentials.
internal sealed class SendAdmissionStore : IDisposable
{
    private readonly WindowsProtectedJournal _journal;
    private readonly string _instance, _account;
    private sealed record Consent(int Schema, string Instance, string Account, string? Family);
    internal string? Family { get; private set; }
    internal SendAdmissionStore(string directory, string instance, string account)
    {
        _instance = instance; _account = account;
        bool exists = File.Exists(Path.Combine(directory, "state.bin"));
        _journal = new(directory, create: !exists);
        try
        {
            var consent = exists ? _journal.Read<Consent>() : new(1, instance, account, null);
            if (consent.Schema != 1 || consent.Instance != instance || consent.Account != account || consent.Family?.Length > 128)
                throw new InvalidOperationException("Outbox replay consent binding mismatch.");
            Family = consent.Family;
            if (!exists) _journal.Write(consent);
        }
        catch { _journal.Dispose(); throw; }
    }
    internal void Approve(string family)
    {
        if (family.Length is < 1 or > 128) throw new InvalidOperationException("Invalid replay consent.");
        _journal.Write(new Consent(1, _instance, _account, family));
        Family = family;
    }
    public void Dispose() => _journal.Dispose();
}
