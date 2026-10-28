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
        double durationSec = 0.0;
        double elapsedSec = 0.0;
        juce::Array<Output> outputs;
        juce::String error;
        juce::StringArray log;
    };

    SeparationJob();
    ~SeparationJob() override;

    /** Returns false (and records why) if a run is already active or the engine is missing. */
    bool start (const juce::File& input, const juce::File& outDir, const EngineLocation& location = locateEngine());
    void cancel();

    /** Rebuilds a finished state from a saved project, if the stems still exist. */
    void restore (const juce::File& input, const juce::Array<Output>& outputs);

    Snapshot snapshot() const;
    juce::uint32 version() const noexcept { return stateVersion.load(); }
    bool isActive() const;

    static juce::File defaultOutputFolder (const juce::File& input);

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
