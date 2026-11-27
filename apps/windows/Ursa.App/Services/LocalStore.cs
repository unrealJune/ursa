using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace Ursa.App.Services;

/// <summary>The app's own files under %LOCALAPPDATA%\Ursa. Secrets are DPAPI-protected for this user.</summary>
public static class LocalStore
{
    /// <summary>URSA_DATA_DIR runs a second, separate copy while developing.</summary>
    public static string Root { get; } =
        Environment.GetEnvironmentVariable("URSA_DATA_DIR") is { Length: > 0 } dir ? dir :
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Ursa");

    public static string HostDataDir => Path.Combine(Root, "host");
    public static string ViewCache => Path.Combine(Root, "view");
    public static string ThumbScratch => Path.Combine(Root, "thumbs-out");

    private static readonly byte[] AccountEntropy = Encoding.UTF8.GetBytes("ursa account key v1");
    private static readonly byte[] TokenEntropy = Encoding.UTF8.GetBytes("ursa relay token v1");

    private static string AccountKeyPath => Path.Combine(Root, "account.key");

    /// <summary>The account root (hex), DPAPI-protected for this Windows user; null before onboarding.</summary>
    public static string? LoadAccountKey()
    {
        if (!File.Exists(AccountKeyPath)) return null;
        byte[] key = ProtectedData.Unprotect(File.ReadAllBytes(AccountKeyPath), AccountEntropy, DataProtectionScope.CurrentUser);
        return Convert.ToHexStringLower(key);
    }

    public static void SaveAccountKey(string hex)
    {
        Directory.CreateDirectory(Root);
        File.WriteAllBytes(AccountKeyPath, ProtectedData.Protect(Convert.FromHexString(hex), AccountEntropy, DataProtectionScope.CurrentUser));
    }

    /// <summary>Signing out of this PC: the library stays on the nodes; only this copy of the key goes.</summary>
    public static void ForgetAccountKey()
    {
        if (File.Exists(AccountKeyPath)) File.Delete(AccountKeyPath);
    }

    public sealed class Settings
    {
        /// <summary>Street Cryptid's relays (S2: n0's public relays carried almost nothing).</summary>
        public List<string> Relays { get; set; } = ["https://iroh-euw.streetcrypt.id", "https://iroh-wus.streetcrypt.id"];
        public string? RelayTokenProtected { get; set; }

        /// <summary>Send diagnostics to the developer: null until the user chooses (then the build's default).</summary>
        public bool? SendDiagnostics { get; set; }

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
