using System.Text.Json;
using Sproyt.WindowsProbe;

internal static class Program
{
    private static int Main(string[] args)
    {
        try
        {
            if (args.Length != 1 || !Path.IsPathFullyQualified(args[0]))
                throw new ArgumentException("Supply an absolute, test-owned artifact directory.");

            Action[] checks = [OpeningAndFit, OriginalPixelSize, FitBelowFivePercent, PanningAndResize,
                ManualZoomAndToggle, PinchAtZoomLimits, MovingPinchCenter, InvalidMeasurementsAndInput];
            foreach (var check in checks) check();

            string run = Path.Combine(args[0], "image-viewport-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(run);
            string receipt = Path.Combine(run, "receipt.json");
            File.WriteAllText(receipt, JsonSerializer.Serialize(new
            {
                passed = true, scope = "native_image_viewport_contracts",
                source = "linked production viewport geometry with synthetic dimensions and DIP inputs",
                checks = checks.Select(check => check.Method.Name)
            }));
            Console.WriteLine(receipt);
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error);
            return 1;
        }
    }

    private static void OpeningAndFit()
    {
        var view = new NativeImageViewport(1600, 1200);
        view.Resize(1280, 720);
        Check(view.Ready, "resize should initialize the view");
        Near(.8, view.Scale); Near(.6, view.FitScale);
        view.ZoomBy(1 / 1.4); Near(4.0 / 7, view.Scale);
        view.Fit(); Near(.6, view.Scale);
        view.Resize(390, 844); Near(.24375, view.Scale);

        var portrait = new NativeImageViewport(400, 2400);
        portrait.Resize(1280, 720); Near(1, portrait.Scale);
        portrait.Fit(); Near(.3, portrait.Scale);
    }

    private static void OriginalPixelSize()
    {
        var view = new NativeImageViewport(2000, 1000);
        view.Resize(390, 844); Near(.195, view.Scale);
        view.ActualSize(); Near(1, view.Scale);
        view.PanBy(400, 400); Near(400, view.PanX); Near(78, view.PanY);
        view.Resize(820, 1000); Near(1, view.Scale); Near(400, view.PanX); Near(0, view.PanY);
        view.ActualSize(); Near(0, view.PanX);
    }

    private static void FitBelowFivePercent()
    {
        var view = new NativeImageViewport(8000, 24000);
        view.Resize(300, 240);
        Near(.0375, view.Scale); Near(.01, view.FitScale); Near(.01, view.MinScale);
        view.ZoomBy(.001); Near(.01, view.Scale);
        view.ActualSize(); view.Fit(); Near(.01, view.Scale);
        view.Resize(800, 480); Near(.02, view.Scale); Near(.02, view.MinScale);
    }

    private static void PanningAndResize()
    {
        var view = new NativeImageViewport(1600, 1800);
        view.Resize(1280, 720); view.PanBy(200, 200); Near(0, view.PanX); Near(200, view.PanY);
        view.PanBy(0, 400); Near(360, view.PanY);
        view.PanBy(0, -1000); Near(-360, view.PanY);
        view.Resize(820, 600); Near(.5125, view.Scale); Near(-161.25, view.PanY);
        view.Resize(820, 1000); Near(0, view.PanY);
        view.PanBy(-100, 100); Near(0, view.PanX); Near(0, view.PanY);
    }

    private static void ManualZoomAndToggle()
    {
        var view = new NativeImageViewport(200, 100);
        view.Resize(1280, 720); Near(1, view.Scale); Near(1, view.FitScale);
        view.ToggleZoom(); Near(2.5, view.Scale);
        view.ToggleZoom(); Near(1, view.Scale);
        view.ZoomBy(1 / 1.4); Near(1 / 1.4, view.Scale);
        view.ZoomBy(double.MaxValue); Near(5, view.Scale);
        view.ZoomBy(double.MaxValue); Near(5, view.Scale);
        view.ZoomBy(double.Epsilon); Near(.05, view.Scale);

        var wide = new NativeImageViewport(1600, 1200);
        wide.Resize(1280, 720); wide.ToggleZoom(); Near(1, wide.Scale);
        wide.ToggleZoom(); Near(.6, wide.Scale);
        wide.Resize(390, 844); Near(.24375, wide.Scale);
    }

    private static void PinchAtZoomLimits()
    {
        var view = new NativeImageViewport(1600, 1200);
        view.Resize(1280, 720);
        view.Pinch(.8, 0, 0, 740, 360, 740, 360, 10);
        Near(5, view.Scale); Near(-525, view.PanX); Near(0, view.PanY);
        // Second move still refers to the start of the same pinch.
        view.Pinch(.8, 0, 0, 740, 360, 740, 360, .01);
        Near(.05, view.Scale); Near(0, view.PanX); Near(0, view.PanY);
        view.Pinch(.8, 0, 0, 740, 360, 740, 360, 0); Near(.05, view.Scale);
    }

    private static void MovingPinchCenter()
    {
        var view = new NativeImageViewport(1600, 1200);
        view.Resize(800, 600); view.ActualSize(); view.PanBy(20, -30);
        view.Pinch(1, 20, -30, 500, 315, 550, 340, 2);
        Near(2, view.Scale); Near(-10, view.PanX); Near(-50, view.PanY);
        view.Resize(640, 480); Near(2, view.Scale);
        view.Fit(); Near(0, view.PanX); Near(0, view.PanY);
    }

    private static void InvalidMeasurementsAndInput()
    {
        foreach (double extent in new[] { 0d, -1, double.NaN, double.PositiveInfinity, double.MaxValue })
        {
            RejectDimension(() => new NativeImageViewport(extent, 200));
            RejectDimension(() => new NativeImageViewport(200, extent));
        }

        var view = new NativeImageViewport(1200, 800);
        view.Resize(0, 200); view.Resize(double.NaN, 500); view.Resize(500, double.PositiveInfinity);
        view.ActualSize(); view.PanBy(25, 10); view.Fit(); view.ToggleZoom(); view.ZoomBy(2);
        Check(!view.Ready, "unmeasured view accepted input");
        view.Resize(600, 400);
        view.Resize(double.PositiveInfinity, 200); view.Resize(300, -10);
        view.ZoomBy(double.NaN); view.ZoomBy(double.PositiveInfinity); view.ZoomBy(-1);
        view.PanBy(double.NaN, 20); view.PanBy(10, double.NegativeInfinity);
        view.Pinch(0, 0, 0, 300, 200, 300, 200, 2);
        view.Pinch(.5, 0, 0, 300, 200, 300, double.PositiveInfinity, 2);
        view.Pinch(.5, 0, 0, 300, 200, 300, 200, double.NaN);
        Near(.5, view.Scale); Near(0, view.PanX); Near(0, view.PanY);
        // Rejected operations must not switch out of the opening fit mode.
        view.Resize(300, 400); Near(.25, view.Scale);
    }

    private static void RejectDimension(Func<NativeImageViewport> create)
    {
        try { create(); }
        catch (ArgumentOutOfRangeException) { return; }
        throw new InvalidOperationException("Expected the invalid original dimension to be rejected.");
    }

    private static void Near(double expected, double actual)
    {
        Check(double.IsFinite(actual) && Math.Abs(expected - actual) < 1e-9, $"Expected {expected}, got {actual}.");
    }

    private static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }
}
