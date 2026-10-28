#include "SeparationJob.h"

namespace
{
    constexpr int maxLogLines = 400;
}

SeparationJob::SeparationJob() : juce::Thread ("VoxSplit separation") {}

SeparationJob::~SeparationJob()
{
    cancel();
    stopThread (5000);
}

template <typename Fn>
void SeparationJob::update (Fn&& fn)
{
    {
        const juce::ScopedLock sl (lock);
        fn (state);
    }
    ++stateVersion;
}

juce::File SeparationJob::defaultOutputFolder (const juce::File& input)
{
    return juce::File::getSpecialLocation (juce::File::userMusicDirectory)
        .getChildFile ("VoxSplit")
        .getChildFile (input.getFileNameWithoutExtension());
}

void SeparationJob::requestEstimate (const juce::File& input, std::shared_ptr<Estimate> into,
                                     const EngineLocation& loc)
{
    into->bestSec = -1.0;
    into->fastSec = -1.0;
    const auto ticket = ++into->version;

    // The lambda owns a reference to the result, so it stays valid even if the
    // plugin is closed before the engine answers.
    juce::Thread::launch ([input, into, ticket, loc] {
        if (! loc.isValid())
            return;

        juce::ChildProcess child;
        const juce::StringArray args { loc.engine.getFullPathName(), "estimate",
                                       "--input=" + input.getFullPathName(),
                                       "--models-dir=" + loc.modelsDir.getFullPathName() };
        if (! child.start (args, juce::ChildProcess::wantStdOut))
            return;

        const auto output = child.readAllProcessOutput();
        for (const auto& line : juce::StringArray::fromLines (output))
        {
            const auto json = juce::JSON::parse (line);
            if (json.getProperty ("event", {}).toString() == "estimate" && into->version == ticket)
            {
                into->bestSec = (double) json.getProperty ("best_sec", -1.0);
                into->fastSec = (double) json.getProperty ("fast_sec", -1.0);
                ++into->version;
            }
        }
    });
}

bool SeparationJob::isActive() const
{
    return isThreadRunning();
}

bool SeparationJob::start (const juce::File& input, const juce::File& outputFolder, Quality quality,
                           const EngineLocation& loc)
{
    if (isThreadRunning())
        return false;

    // A previous run's thread has exited but may not have been joined yet.
    stopThread (1000);

    if (! loc.isValid())
    {
        update ([&] (Snapshot& s) {
            s = {};
            s.phase = Phase::failed;
            s.input = input;
            s.error = loc.problem;
        });
        return false;
    }

    outDir = outputFolder;

    // Every option is a single "--flag=value" argv element. ChildProcess passes
    // the array straight to execvp: no shell, so spaces, quotes, unicode and
    // leading dashes in paths are all safe.
    args.clear();
    args.add (loc.engine.getFullPathName());
    args.add ("separate");
    args.add ("--input=" + input.getFullPathName());
    args.add ("--out-dir=" + outDir.getFullPathName());
    args.add ("--models-dir=" + loc.modelsDir.getFullPathName());
    args.add (quality == Quality::best ? "--quality=best" : "--quality=fast");

    cancelRequested = false;
    sawDone = sawCancelled = false;

    update ([&] (Snapshot& s) {
        s = {};
        s.phase = Phase::preparing;
        s.input = input;
    });

    startThread (juce::Thread::Priority::normal);
    return true;
}

void SeparationJob::cancel()
{
    cancelRequested = true;
    signalThreadShouldExit();

    // Killing the child closes its stdout, which unblocks the worker's read.
    const juce::ScopedLock sl (childLock);
    if (child != nullptr)
        child->kill();
}

void SeparationJob::restore (const juce::File& input, const juce::Array<Output>& outputs)
{
    if (isThreadRunning())
        return;

    const bool allExist = ! outputs.isEmpty()
                       && std::all_of (outputs.begin(), outputs.end(), [] (const Output& o) { return o.file.existsAsFile(); });

    update ([&] (Snapshot& s) {
        s = {};
        s.input = input;
        if (allExist)
        {
            s.phase = Phase::finished;
            s.outputs = outputs;
            s.overall = s.fraction = 1.0f;
        }
    });
}

SeparationJob::Snapshot SeparationJob::snapshot() const
{
    const juce::ScopedLock sl (lock);
    return state;
}

void SeparationJob::appendLog (const juce::String& line)
{
    update ([&] (Snapshot& s) {
        s.log.add (line);
        if (s.log.size() > maxLogLines)
            s.log.removeRange (0, s.log.size() - maxLogLines);
    });
}

void SeparationJob::run()
{
    auto process = std::make_unique<juce::ChildProcess>();

    // stdout carries JSON events; stderr is merged in and shown as log text.
    if (! process->start (args, juce::ChildProcess::wantStdOut | juce::ChildProcess::wantStdErr))
    {
        update ([] (Snapshot& s) {
            s.phase = Phase::failed;
            s.error = "Couldn't launch the VoxSplit engine.";
        });
        return;
    }

    {
        const juce::ScopedLock sl (childLock);
        child = std::move (process);

        // cancel() may have run before the child existed.
        if (cancelRequested)
            child->kill();
    }

    // ChildProcess reads with fread(), which blocks until the requested byte
    // count arrives. Asking for one byte at a time returns as soon as anything
    // is available; stdio buffering keeps it cheap.
    juce::MemoryOutputStream pending;
    char byte = 0;

    for (;;)
    {
        if (child->readProcessOutput (&byte, 1) <= 0)
            break;

        if (byte == '\n')
        {
            handleLine (pending.toString().trimEnd());
            pending.reset();
        }
        else
        {
            pending.writeByte (byte);
        }
    }

    if (pending.getDataSize() > 0)
        handleLine (pending.toString().trimEnd());

    child->waitForProcessToFinish (5000);
    const auto exitCode = child->getExitCode();

    {
        const juce::ScopedLock sl (childLock);
        child.reset();
    }

    if (cancelRequested || sawCancelled)
    {
        // ChildProcess::kill() is SIGKILL, so the engine can't clean up after itself.
        for (const auto& f : outDir.findChildFiles (juce::File::findFiles, false, "*.partial"))
            f.deleteFile();

        update ([] (Snapshot& s) { s.phase = Phase::cancelled; });
        return;
    }

    update ([&] (Snapshot& s) {
        if (sawDone && exitCode == 0)
        {
            s.phase = Phase::finished;
            s.overall = s.fraction = 1.0f;
        }
        else
        {
            s.phase = Phase::failed;
            if (s.error.isEmpty())
                s.error = "The engine stopped unexpectedly (exit code " + juce::String (exitCode) + ").";
        }
    });
}

void SeparationJob::handleLine (const juce::String& line)
{
    if (line.isEmpty())
        return;

    if (! line.startsWithChar ('{'))
    {
        appendLog (line);
        return;
    }

    const auto json = juce::JSON::parse (line);
    const auto event = json.getProperty ("event", {}).toString();

    if (event.isEmpty())
    {
        appendLog (line);
        return;
    }

    if (event == "start")
    {
        update ([&] (Snapshot& s) {
            s.stages = (int) json.getProperty ("stages", 2);
            s.durationSec = (double) json.getProperty ("duration_sec", 0.0);
            s.etaSec = (double) json.getProperty ("estimate_sec", -1.0);
        });
    }
    else if (event == "stage")
    {
        const auto model = json.getProperty ("model", {}).toString();
        const auto device = json.getProperty ("device", {}).toString();
        appendLog ("Stage " + json.getProperty ("index", 0).toString() + ": " + model + " on " + device);
        update ([&] (Snapshot& s) {
            s.phase = Phase::running;
            s.stage = json.getProperty ("stage", {}).toString();
            s.stageIndex = (int) json.getProperty ("index", 0);
            s.stages = (int) json.getProperty ("stages", s.stages);
            s.device = device;
            s.fraction = 0.0f;
        });
    }
    else if (event == "progress")
    {
        update ([&] (Snapshot& s) {
            s.fraction = (float) (double) json.getProperty ("fraction", 0.0);
            s.overall = (float) (double) json.getProperty ("overall", 0.0);
            s.etaSec = (double) json.getProperty ("eta_sec", s.etaSec);
        });
    }
    else if (event == "log")
    {
        appendLog (json.getProperty ("message", {}).toString());
    }
    else if (event == "output")
    {
        const Output out { json.getProperty ("stem", {}).toString(),
                           juce::File (json.getProperty ("path", {}).toString()) };
        appendLog ("Wrote " + out.file.getFullPathName());
        update ([&] (Snapshot& s) { s.outputs.add (out); });
    }
    else if (event == "done")
    {
        sawDone = true;
        update ([&] (Snapshot& s) { s.elapsedSec = (double) json.getProperty ("elapsed_sec", 0.0); });
    }
    else if (event == "cancelled")
    {
        sawCancelled = true;
    }
    else if (event == "error")
    {
        const auto message = json.getProperty ("message", "Unknown error").toString();
        appendLog ("Error: " + message);
        update ([&] (Snapshot& s) { s.error = message; });
    }
}
