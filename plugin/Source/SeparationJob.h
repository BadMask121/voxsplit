#pragma once

#include <juce_core/juce_core.h>

#include "EngineLocator.h"

/**
    Runs voxsplit-engine as a child process on its own thread and turns its
    JSON-Lines stdout into a state snapshot (docs/02-engine-protocol.md).

    Threading contract:
      - start(), cancel() and restore() are called from the message thread.
      - run() (the worker) only ever writes the snapshot under `lock`; it never
        calls into the UI, so closing the editor mid-run can't deadlock or
        touch freed memory.
      - The editor polls snapshot() each frame when version() changes.
      - The audio thread never touches this object.
*/
class SeparationJob final : private juce::Thread
{
public:
    enum class Phase { idle, preparing, running, finished, failed, cancelled };

    /** best: BS-RoFormer karaoke (cleaner, slower). fast: MDX-Net karaoke, 3 passes. */
    enum class Quality { best, fast };

    /** leadBacking: lead_vocals.wav + backing_vocals.wav. vocalRemover: vocals.wav + instrumental.wav. */
    enum class Tool { leadBacking, vocalRemover };

    /** What a run should do; mirrors the engine's flags. */
    struct Settings
    {
        Tool tool = Tool::leadBacking;
        Quality quality = Quality::best;
        int passes = 1;      // karaoke passes (lead/backing only)
        int vocalPasses = 1; // Kim Vocal 2 passes; 0 = file is vocals (lead/backing only)
    };

    struct Output
    {
        juce::String stem; // "lead", "backing", "instrumental"
        juce::File file;
    };

    struct Snapshot
    {
        Phase phase = Phase::idle;
        juce::File input;
        juce::String stage;      // "vocals" | "karaoke"
        int stageIndex = 0, stages = 0;
        float fraction = 0.0f;   // within the current stage
        float overall = 0.0f;
        juce::String device;     // "coreml", "cpu", ...
        juce::String model;      // the current stage's model: diagnostics and tests only, never shown
        double durationSec = 0.0;
        double elapsedSec = 0.0;
        double etaSec = -1.0;     // seconds left, -1 until the engine says

        juce::Array<Output> outputs;
        juce::String error;
        juce::StringArray log;
    };

    SeparationJob();
    ~SeparationJob() override;

    /** Returns false (and records why) if a run is already active or the engine is missing. */
    bool start (const juce::File& input, const juce::File& outDir, const Settings& settings,
                const EngineLocation& location = locateEngine());
    void cancel();

    /** Rebuilds a finished state from a saved project, if the stems still exist. */
    void restore (const juce::File& input, const juce::Array<Output>& outputs);

    Snapshot snapshot() const;
    juce::uint32 version() const noexcept { return stateVersion.load(); }
    bool isActive() const;

    static juce::File defaultOutputFolder (const juce::File& input);

    /** Expected run times for a file at both qualities, from `voxsplit-engine estimate`.
        Filled in on a background thread; read it from the UI by polling `version`. */
    struct Estimate
    {
        // Seconds with one vocal + one karaoke pass, and per extra pass of each model.
        std::atomic<double> bestSec { -1.0 }, bestPassSec { 0.0 }, fastSec { -1.0 }, fastPassSec { 0.0 },
                            vocalSec { 0.0 }, vocalPassSec { 0.0 };
        std::atomic<juce::uint32> version { 0 };

        /** Expected total for a run, or a negative number while still estimating. */
        double total (const Settings& s) const
        {
            if (s.tool == Tool::vocalRemover)
                return bestSec.load() < 0.0 ? -1.0
                                            : vocalSec.load() + (double) juce::jmax (0, s.vocalPasses - 1) * vocalPassSec.load();
            return total (s.quality, s.passes, s.vocalPasses);
        }

        double total (Quality q, int passes, int vocalPasses) const
        {
            const auto base = q == Quality::best ? bestSec.load() : fastSec.load();
            const auto extra = q == Quality::best ? bestPassSec.load() : fastPassSec.load();
            if (base < 0.0)
                return -1.0;
            const auto vocals = vocalPasses == 0 ? -vocalSec.load()
                                                 : (double) (vocalPasses - 1) * vocalPassSec.load();
            return base + (double) juce::jmax (0, passes - 1) * extra + vocals;
        }
    };

    /** Starts estimating `input` in the background; the result lands in `into`. */
    static void requestEstimate (const juce::File& input, std::shared_ptr<Estimate> into,
                                 const EngineLocation& location = locateEngine());

private:
    void run() override;
    void handleLine (const juce::String& line);
    void appendLog (const juce::String& line);
    template <typename Fn> void update (Fn&& fn);

    mutable juce::CriticalSection lock;
    Snapshot state;
    std::atomic<juce::uint32> stateVersion { 0 };

    juce::CriticalSection childLock;
    std::unique_ptr<juce::ChildProcess> child;
    std::atomic<bool> cancelRequested { false };
    bool sawDone = false, sawCancelled = false;

    juce::StringArray args;
    juce::File outDir;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (SeparationJob)
};
