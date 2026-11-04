namespace Sproyt.WindowsProbe;

internal static class NativeUploadPresentation
{
    internal static bool BlocksSend(NativeUploadSnapshot? uploads, string? user, string? family, string channel, string? parent) =>
        uploads?.Jobs.Any(job => job.Target.UserId == user && job.Target.FamilyStamp == family &&
            job.Target.ChannelId == channel && job.Target.ParentMessageId == parent &&
            (job.State is NativeUploadState.Queued or NativeUploadState.Uploading or NativeUploadState.Finalizing ||
                job.State == NativeUploadState.Ready && (job.HandoffStarted || job.Error is null))) == true;

    internal static NativeContentPart Attachment(string marker) =>
        NativeMessageContent.Parse(marker).FirstOrDefault(part => part.Kind == "media") ?? new("media", "Vedlegg");
}
