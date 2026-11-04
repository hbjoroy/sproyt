using Sproyt.WindowsProbe;

internal static class CoverContracts
{
    internal static async Task RunAsync(NativeMessageActionScope scope, NativeMessage message)
    {
        var actions = new NativeMessageActionController();
        actions.Bind(scope);
        actions.Open(scope, message, NativeMessageActionMode.Edit);
        actions.ChangeInput(scope, message.Id, NativeMessageActionMode.Edit, "dirty edit");
        actions.SetCovered(true);
        var calls = new List<NativeMessageMutation>();
        var noRead = new NativeMessageActionCommands(m => { calls.Add(m); return Task.CompletedTask; },
            _ => throw new Exception("A covered action started readback."));
        Require(actions.Available(scope, message) && !actions.CanOpen(scope, message), "Cover changed resource availability or left input enabled.");
        actions.Open(scope, message, NativeMessageActionMode.Delete);
        actions.ChangeInput(scope, message.Id, NativeMessageActionMode.Edit, "stale keystroke");
        actions.Cancel(scope, message.Id);
        await actions.ConfirmAsync(scope, message, noRead);
        actions.Observe(scope, [message]);
        Require(actions.Mode == NativeMessageActionMode.Edit && actions.Input == "dirty edit" && calls.Count == 0, "Covered input changed the draft or issued a write.");

        actions.SetCovered(false);
        await actions.ConfirmAsync(scope, message, noRead);
        Require(actions.Mode == NativeMessageActionMode.Closed && calls.Single().Value!.Contains("dirty edit"), "Uncover failed to resume the preserved edit.");
        actions.SetCovered(true);
        await actions.ToggleAsync(scope, message, "👍", false, noRead);
        Require(calls.Count == 1, "A retained reaction button wrote while covered.");

        actions.SetCovered(false);
        actions.Open(scope, message, NativeMessageActionMode.Edit);
        actions.ChangeInput(scope, message.Id, NativeMessageActionMode.Edit, "new edit");
        actions.SetCovered(true);
        actions.Observe(scope, [message with { DeletedAt = DateTimeOffset.UtcNow }]);
        Require(actions.Mode == NativeMessageActionMode.Closed, "Remote deletion left a hidden edit open.");

        actions.SetCovered(false);
        actions.Open(scope, message, NativeMessageActionMode.Delete);
        var finishWrite = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var finishRead = new TaskCompletionSource<bool?>(TaskCreationOptions.RunContinuationsAsynchronously);
        int reads = 0;
        var pending = new NativeMessageActionCommands(_ => finishWrite.Task, _ => { reads++; return finishRead.Task; });
        var writing = actions.ConfirmAsync(scope, message, pending);
        Require(actions.Busy, "Write did not start.");
        actions.SetCovered(true);
        finishWrite.SetException(NativeCommandException.Uncertain("existing-write"));
        await writing;
        Require(actions.NeedsReadBack && !actions.Busy, "Cover invalidated the admitted write's uncertain outcome.");
        await actions.CheckAsync(scope, message.Id, pending);
        Require(reads == 0 && actions.NeedsReadBack, "Retained check started under cover.");
        actions.SetCovered(false);
        var reading = actions.CheckAsync(scope, message.Id, pending);
        Require(reads == 1 && actions.Busy, "Explicit readback was not admitted after uncover.");
        actions.SetCovered(true);
        finishRead.SetResult(true); await reading;
        Require(actions.Mode == NativeMessageActionMode.Closed && !actions.NeedsReadBack, "Cover prevented an admitted readback from settling.");
    }

    private static void Require(bool condition, string detail)
    {
        if (!condition) throw new InvalidOperationException(detail);
    }
}
