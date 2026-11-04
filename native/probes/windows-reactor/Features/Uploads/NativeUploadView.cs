using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

internal sealed record NativeUploadActions(Action<string> Retry, Action<string> Cancel, Action<string> Remove);

internal static class NativeUploadView
{
    internal static Element Render(NativeUploadSnapshot state, NativeUploadTarget? composer, SproytPalette palette, NativeUploadActions actions)
    {
        Element Row(NativeUploadJob job)
        {
            bool held = job.Target.FamilyStamp != state.ActiveFamilyStamp;
            string context = composer is null || job.Target.ChannelId != composer.ChannelId ? " · Ei anna samtale" :
                job.Target.ParentMessageId != composer.ParentMessageId ? " · Eit anna utkast" : "";
            string status = held ? "Frå ei førre innlogging. Fjern opplastinga og vel fila på nytt." : job.State switch
            {
                NativeUploadState.Queued => "Ventar på opplasting",
                NativeUploadState.Uploading => "Lastar opp " + (job.UploadedBytes * 100 / job.Source.SizeBytes) + " %",
                NativeUploadState.Finalizing => "Fila er lasta opp. Gjer vedlegget klart …",
                NativeUploadState.Ready when job.HandoffStarted => "Vedlegget ventar på stadfesting i utkastet",
                NativeUploadState.Ready => "Klart vedlegg ventar på plass i utkastet",
                NativeUploadState.Attached => "Lagt til i utkastet",
                NativeUploadState.Cancelled => "Opplastinga er avbroten",
                _ => job.Error ?? "Opplastinga feila"
            };
            bool remove = held || job.State is NativeUploadState.Cancelled or NativeUploadState.Attached or NativeUploadState.Failed;
            bool retry = !held && job.Retryable && job.State != NativeUploadState.Cancelled &&
                (state.StorageError is not null || job.State == NativeUploadState.Failed || job.State == NativeUploadState.Ready && (job.Error is not null || job.HandoffStarted));
            return VStack(4,
                TextBlock(job.Source.FileName + context).SemiBold().TextWrapping().Foreground(palette.Ink),
                TextBlock(status).TextWrapping().Foreground(job.State == NativeUploadState.Failed ? palette.Error : palette.Muted)
                    .AutomationId("live-upload-status-" + job.Id),
                TextBlock(job.Error ?? "").TextWrapping().Foreground(palette.Error)
                    .IsVisible(!held && job.Error is not null && job.State != NativeUploadState.Failed),
                HStack(8,
                    Button("Prøv igjen", () => actions.Retry(job.Id)).MinHeight(44).IsVisible(retry)
                        .AutomationName("Prøv opplastinga på nytt").AutomationId("live-upload-retry-" + job.Id),
                    Button("Avbryt", () => actions.Cancel(job.Id)).MinHeight(44).IsVisible(!remove).IsEnabled(!(job.State == NativeUploadState.Ready && job.HandoffStarted))
                        .AutomationName("Avbryt opplastinga").AutomationId("live-upload-cancel-" + job.Id),
                    Button("Fjern", () => actions.Remove(job.Id)).MinHeight(44).IsVisible(remove)
                        .AutomationName("Fjern opplastinga frå køen").AutomationId("live-upload-remove-" + job.Id))
            ).Padding(12, 8).Background(palette.Surface).AutomationId("live-upload-row-" + job.Id).WithKey(job.Id);
        }
        return VStack(6,
            TextBlock(state.StorageError ?? "").TextWrapping().Foreground(palette.Error).IsVisible(state.StorageError is not null),
            VStack(6, state.Jobs.Select(Row).ToArray()),
            TextBlock("Ferdige vedlegg blir lagde til utkastet. Trykk Send når du er klar.").TextWrapping().FontSize(12)
                .Foreground(palette.Muted).IsVisible(state.Jobs.Count != 0)
        ).AutomationId("live-upload-queue");
    }
}
