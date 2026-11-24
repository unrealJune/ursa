using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Ursa.App.Services;

/// <summary>The app's own files under %LOCALAPPDATA%\Ursa. Secrets are DPAPI-protected for this user.</summary>
public static class LocalStore
{
    public static string Root { get; } =
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Ursa");

    public static string HostDataDir => Path.Combine(Root, "host");
    public static string ViewCache => Path.Combine(Root, "view");
    public static string ThumbScratch => Path.Combine(Root, "thumbs-out");

    private static readonly byte[] AccountEntropy = Encoding.UTF8.GetBytes("ursa account key v1");
    private static readonly byte[] TokenEntropy = Encoding.UTF8.GetBytes("ursa relay token v1");

    /// <summary>
    /// The account key: made on first run, then kept DPAPI-protected. Losing it loses the
    /// library, which is why Settings offers to show it for safekeeping.
    /// </summary>
    public static string AccountKeyHex()
    {
        Directory.CreateDirectory(Root);
        string path = Path.Combine(Root, "account.key");
        if (File.Exists(path))
        {
            byte[] key = ProtectedData.Unprotect(File.ReadAllBytes(path), AccountEntropy, DataProtectionScope.CurrentUser);
            return Convert.ToHexStringLower(key);
        }
        byte[] fresh = RandomNumberGenerator.GetBytes(32);
        File.WriteAllBytes(path, ProtectedData.Protect(fresh, AccountEntropy, DataProtectionScope.CurrentUser));
        return Convert.ToHexStringLower(fresh);
    }

    public sealed class Settings
    {
        /// <summary>Street Cryptid's relays (S2: n0's public relays carried almost nothing).</summary>
        public List<string> Relays { get; set; } = ["https://iroh-euw.streetcrypt.id", "https://iroh-wus.streetcrypt.id"];
        public string? RelayTokenProtected { get; set; }

        public string? RelayToken
        {
            get => RelayTokenProtected is null ? null : Encoding.UTF8.GetString(
                ProtectedData.Unprotect(Convert.FromBase64String(RelayTokenProtected), TokenEntropy, DataProtectionScope.CurrentUser));
            set => RelayTokenProtected = string.IsNullOrWhiteSpace(value) ? null : Convert.ToBase64String(
                ProtectedData.Protect(Encoding.UTF8.GetBytes(value.Trim()), TokenEntropy, DataProtectionScope.CurrentUser));
        }
    }

    private static string SettingsPath => Path.Combine(Root, "settings.json");

    public static Settings LoadSettings()
    {
        try
        {
            if (File.Exists(SettingsPath))
                return JsonSerializer.Deserialize<Settings>(File.ReadAllText(SettingsPath)) ?? new Settings();
        }
        catch { }
        return new Settings();
    }

    public static void SaveSettings(Settings s)
    {
        Directory.CreateDirectory(Root);
        File.WriteAllText(SettingsPath, JsonSerializer.Serialize(s, new JsonSerializerOptions { WriteIndented = true }));
    }
}
