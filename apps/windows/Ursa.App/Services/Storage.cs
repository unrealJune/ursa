using System.Text.Json;

namespace Ursa.App.Services;

/// <summary>One protection choice: any <c>K</c> of <c>N</c> pieces rebuild a file.</summary>
public sealed record Choice(int K, int N, int Survives, long LimitBytes, double? OfflineHours, bool Available, bool Recommended)
{
    public string Name => K == 1 ? $"{N} copies" : $"{(double)N / K:0.##}×";
    public string Shape => K == 1 ? "" : $"{K} of {N}";
    public bool Is(int k, int n) => K == k && N == n;

    public string OfflineText => !Available || OfflineHours is not { } h ? "—"
        : h < 1.0 / 60 ? "< 1 min/yr"
        : h < 1 ? $"{Math.Round(h * 60)} min/yr"
        : $"{h:0.#} h/yr";

    public static Choice FromJson(JsonElement c) => new(
        c.GetProperty("k").GetInt32(), c.GetProperty("n").GetInt32(), c.GetProperty("survives").GetInt32(),
        c.GetProperty("limit_bytes").GetInt64(),
        c.TryGetProperty("offline_hours", out var h) && h.ValueKind == JsonValueKind.Number ? h.GetDouble() : null,
        c.GetProperty("available").GetBoolean(), c.GetProperty("recommended").GetBoolean());
}

public sealed record Member(string Name, bool Own, bool Reachable, double? Up30d, long UsedBytes, long? GivesYou)
{
    public string UpText => Up30d is { } u ? $"{u * 100:0.#}% up" : "—";
}

public sealed record Hosted(string Name, long UsedBytes, long QuotaBytes);

public sealed record DiskUse(long Total, long Free, long Yours, long Friends, long Reserved);

public sealed record MonthBytes(string Month, long Bytes)
{
    public string Short => DateTime.TryParse(Month + "-01", out var d) ? d.ToString("MMM") : Month;
}

/// <summary>What the host's <c>storage</c> command reports: everything the Pool and Usage pages show.</summary>
public sealed record StorageReport(
    int K, int N, bool Chosen, int Files, int PendingFiles, int LeftoverPieces,
    long StoredBytes, long Photos, long Videos, long OtherFiles,
    long LimitBytes, IReadOnlyList<Choice> Choices,
    long GiveBytes, long HomeBytes, long FriendsBytes,
    IReadOnlyList<Member> Members, IReadOnlyList<Hosted> Hosting, DiskUse? Disk, IReadOnlyList<MonthBytes> Growth)
{
    public Choice Current => Choices.FirstOrDefault(c => c.Is(K, N)) ?? Choices[0];

    public static StorageReport FromJson(JsonElement r)
    {
        var p = r.GetProperty("protection");
        var kinds = r.GetProperty("kinds");
        DiskUse? disk = r.TryGetProperty("disk", out var d) && d.ValueKind == JsonValueKind.Object
            ? new DiskUse(d.GetProperty("total_bytes").GetInt64(), d.GetProperty("free_bytes").GetInt64(), d.GetProperty("yours_bytes").GetInt64(),
                d.GetProperty("friends_bytes").GetInt64(), d.GetProperty("reserved_bytes").GetInt64())
            : null;
        return new StorageReport(
            p.GetProperty("k").GetInt32(), p.GetProperty("n").GetInt32(), r.GetProperty("chosen").GetBoolean(),
            r.GetProperty("files").GetInt32(), r.GetProperty("pending_files").GetInt32(),
            r.TryGetProperty("leftover_pieces", out var lp) ? lp.GetInt32() : 0,
            r.GetProperty("stored_bytes").GetInt64(), kinds.GetProperty("photos").GetInt64(), kinds.GetProperty("videos").GetInt64(), kinds.GetProperty("files").GetInt64(),
            r.GetProperty("limit_bytes").GetInt64(),
            r.GetProperty("choices").EnumerateArray().Select(Choice.FromJson).ToList(),
            r.GetProperty("give_bytes").GetInt64(), r.GetProperty("home_bytes").GetInt64(), r.GetProperty("friends_bytes").GetInt64(),
            r.GetProperty("members").EnumerateArray().Select(m => new Member(
                m.GetProperty("name").GetString() ?? "", m.GetProperty("own").GetBoolean(), m.GetProperty("reachable").GetBoolean(),
                m.TryGetProperty("up_30d", out var u) && u.ValueKind == JsonValueKind.Number ? u.GetDouble() : null,
                m.GetProperty("used_bytes").GetInt64(),
                m.TryGetProperty("gives_you", out var g) && g.ValueKind == JsonValueKind.Number ? g.GetInt64() : null)).ToList(),
            r.GetProperty("hosting").EnumerateArray().Select(h => new Hosted(
                h.GetProperty("name").GetString() ?? "", h.GetProperty("used_bytes").GetInt64(), h.GetProperty("quota_bytes").GetInt64())).ToList(),
            disk,
            r.GetProperty("growth").EnumerateArray().Select(m => new MonthBytes(m.GetProperty("month").GetString() ?? "", m.GetProperty("bytes").GetInt64())).ToList());
    }

    /// <summary>Sizes as people read them on this screen: decimal units, like disks are sold.</summary>
    public static string Size(long b) => b switch
    {
        >= 1_000_000_000_000_000 => "∞",
        >= 1_000_000_000_000 => $"{b / 1e12:0.##} TB",
        >= 1_000_000_000 => $"{b / 1e9:0.#} GB",
        >= 1_000_000 => $"{b / 1e6:0} MB",
        >= 1_000 => $"{b / 1e3:0} KB",
        _ => $"{b} B",
    };
}
