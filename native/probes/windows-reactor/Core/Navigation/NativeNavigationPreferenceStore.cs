using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Optional presentation preferences only. No channel names, search terms,
// history, drafts or credentials are serialized. The account is always explicit.
internal sealed class NativeNavigationPreferenceStore(string directory) : INativeNavigationPreferenceStore
{
    private readonly string _directory = Path.GetFullPath(directory);
    private sealed record Stored(int Version, string UserId, string DirectoryScope);

    public NativeNavigationPreferences Read(string userId)
    {
        string file = FilePath(userId);
        if (!File.Exists(file)) return new();
        if (new FileInfo(file).Length > 4096) return new();
        var value = JsonSerializer.Deserialize<Stored>(File.ReadAllText(file));
        return value is { Version: 1 } && value.UserId == userId && value.DirectoryScope == "direct"
            ? new(NativeNavigationDirectoryScope.Direct) : new();
    }

    public bool Write(string userId, NativeNavigationPreferences preferences)
    {
        string file = FilePath(userId);
        Directory.CreateDirectory(_directory);
        string temporary = file + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            File.WriteAllText(temporary, JsonSerializer.Serialize(new Stored(1, userId,
                preferences.DirectoryScope == NativeNavigationDirectoryScope.Direct ? "direct" : "circles")));
            File.Move(temporary, file, overwrite: true); return true;
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }

    private string FilePath(string userId)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(userId);
        string account = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(userId))).ToLowerInvariant();
        return Path.Combine(_directory, "navigation-v1-" + account + ".json");
    }
}
