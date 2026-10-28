// Headless tests for the plugin's non-UI logic: the separation job driving the
// real engine binary, and stem preview rendering. Requires a built engine and
// downloaded models (scripts/build-engine.sh, scripts/fetch-models.sh).

#include <juce_audio_devices/juce_audio_devices.h>
#include <juce_audio_formats/juce_audio_formats.h>

#include "../Source/SeparationJob.h"
#include "../Source/StemPreview.h"

namespace
{
    const juce::File repo { juce::String (VOXSPLIT_REPO_ROOT) };

    EngineLocation repoEngine()
    {
        return { repo.getChildFile ("engine/target/release/voxsplit-engine"), repo.getChildFile ("models"), {} };
    }

    using Quality = SeparationJob::Quality;

    /** A stereo WAV with a tone and a little noise. */
    juce::File writeTestWav (const juce::File& file, double seconds, double rate)
    {
        file.deleteFile();
        juce::WavAudioFormat wav;
        std::unique_ptr<juce::OutputStream> stream = std::make_unique<juce::FileOutputStream> (file);
        auto writer = wav.createWriterFor (stream, juce::AudioFormatWriterOptions{}
                                                       .withSampleRate (rate)
                                                       .withNumChannels (2)
                                                       .withBitsPerSample (24));
        jassert (writer != nullptr);

        const auto n = (int) (seconds * rate);
        juce::AudioBuffer<float> buffer (2, n);
        juce::Random random (42);
        for (int i = 0; i < n; ++i)
        {
            const auto v = 0.3f * std::sin (juce::MathConstants<float>::twoPi * 440.0f * (float) i / (float) rate)
                         + 0.02f * (random.nextFloat() * 2.0f - 1.0f);
            buffer.setSample (0, i, v);
            buffer.setSample (1, i, v * 0.8f);
        }
        writer->writeFromAudioSampleBuffer (buffer, 0, n);
        return file;
    }

    bool waitFor (std::function<bool()> condition, int timeoutMs)
    {
        const auto end = juce::Time::getMillisecondCounter() + (juce::uint32) timeoutMs;
        while (juce::Time::getMillisecondCounter() < end)
        {
            if (condition())
                return true;
            juce::Thread::sleep (20);
        }
        return condition();
    }
}

class SeparationJobTests final : public juce::UnitTest
{
public:
    SeparationJobTests() : juce::UnitTest ("SeparationJob", "VoxSplit") {}

    void runTest() override
    {
        const auto loc = repoEngine();
        if (! loc.engine.existsAsFile() || ! loc.modelsDir.getChildFile ("BS_Roformer_Karaoke.onnx").existsAsFile())
        {
            logMessage ("SKIPPED: build the engine and fetch models first");
            return;
        }

        juce::TemporaryFile tempDir;
        const auto dir = tempDir.getFile();
        dir.createDirectory();
        // Spaces, quotes, unicode and a leading dash: argv must carry them verbatim.
        const auto input = writeTestWav (dir.getChildFile ("-it's a \"test\" sång.wav"), 12.0, 48000.0);
        const auto outDir = dir.getChildFile ("out dir");

        beginTest ("runs the engine and reports both stems");
        {
            SeparationJob job;
            expect (job.start (input, outDir, Quality::fast, 1, loc));
            expect (! job.start (input, outDir, Quality::fast, 1, loc), "a second start while running must be refused");
            expect (waitFor ([&] { return ! job.isActive(); }, 180000), "job finished in time");

            const auto s = job.snapshot();
            expect (s.phase == SeparationJob::Phase::finished, "phase finished; error: " + s.error);
            expectEquals (s.outputs.size(), 2);
            for (const auto& o : s.outputs)
                expect (o.file.existsAsFile(), o.file.getFullPathName());
            expect (s.device.isNotEmpty());
            expectWithinAbsoluteError (s.overall, 1.0f, 0.001f);
            expect (outDir.findChildFiles (juce::File::findFiles, false, "*.partial").isEmpty());
        }

        beginTest ("estimate reports both qualities in the background");
        {
            auto estimate = std::make_shared<SeparationJob::Estimate>();
            SeparationJob::requestEstimate (input, estimate, loc);
            expect (waitFor ([&] { return estimate->bestSec.load() > 0.0; }, 15000), "estimate arrived");
            expect (estimate->bestSec.load() > estimate->fastSec.load() && estimate->fastSec.load() > 0.0);
            expect (estimate->total (Quality::fast, 3) > estimate->total (Quality::fast, 1), "extra passes add time");
        }

        beginTest ("best quality runs the RoFormer and reports both stems");
        {
            outDir.deleteRecursively();
            SeparationJob job;
            expect (job.start (input, outDir, Quality::best, 1, loc));
            expect (waitFor ([&] { return ! job.isActive(); }, 300000), "best-quality job finished in time");
            const auto s = job.snapshot();
            expect (s.phase == SeparationJob::Phase::finished, "phase finished; error: " + s.error);
            expectEquals (s.outputs.size(), 2);
            expect (s.log.joinIntoString ("\n").contains ("RoFormer"), "the karaoke stage used the RoFormer");
            expect (s.etaSec >= 0.0 && s.etaSec < 5.0, "countdown reached ~0 at the end, got " + juce::String (s.etaSec));
        }

        beginTest ("cancel kills the engine and leaves no partial files");
        {
            outDir.deleteRecursively();
            const auto longInput = writeTestWav (dir.getChildFile ("long.wav"), 90.0, 44100.0);
            SeparationJob job;
            expect (job.start (longInput, outDir, Quality::fast, 1, loc));
            expect (waitFor ([&] { return job.snapshot().phase == SeparationJob::Phase::running; }, 60000));
            job.cancel();
            expect (waitFor ([&] { return ! job.isActive(); }, 10000), "worker exits promptly after cancel");
            expect (job.snapshot().phase == SeparationJob::Phase::cancelled);
            expect (outDir.findChildFiles (juce::File::findFiles, false, "*.partial").isEmpty());
        }

        beginTest ("a missing engine fails with a readable message");
        {
            SeparationJob job;
            EngineLocation missing;
            missing.problem = "VoxSplit's engine is missing";
            expect (! job.start (input, outDir, Quality::fast, 1, missing));
            expect (job.snapshot().phase == SeparationJob::Phase::failed);
            expect (job.snapshot().error.contains ("missing"));
        }

        beginTest ("engine errors surface as failed with the engine's message");
        {
            SeparationJob job;
            const auto notAudio = dir.getChildFile ("notes.wav");
            notAudio.replaceWithText ("not audio");
            expect (job.start (notAudio, outDir, Quality::fast, 1, loc));
            expect (waitFor ([&] { return ! job.isActive(); }, 30000));
            const auto s = job.snapshot();
            expect (s.phase == SeparationJob::Phase::failed);
            expect (s.error.isNotEmpty(), "error message from the engine's error event");
        }
    }
};

class StemPreviewTests final : public juce::UnitTest
{
public:
    StemPreviewTests() : juce::UnitTest ("StemPreview", "VoxSplit") {}

    void runTest() override
    {
        juce::TemporaryFile tempDir;
        tempDir.getFile().createDirectory();
        const auto stem = writeTestWav (tempDir.getFile().getChildFile ("lead_vocals.wav"), 1.0, 44100.0);

        beginTest ("renders nothing (pass-through) when idle");
        {
            StemPreview preview;
            preview.prepare (48000.0, 512);
            juce::AudioBuffer<float> buffer (2, 512);
            buffer.clear();
            expect (! preview.render (buffer));
        }

        beginTest ("plays a stem at a different host rate, then stops at the end");
        {
            StemPreview preview;
            preview.prepare (48000.0, 512);
            preview.play (stem);
            expect (preview.isPlaying());
            expect (preview.getFile() == stem);

            juce::AudioBuffer<float> buffer (2, 512);
            double energy = 0.0;
            int blocks = 0;
            // ~1 s of audio at 48 kHz is 94 blocks; render in real time-ish so read-ahead keeps up.
            while (preview.render (buffer) && blocks < 400)
            {
                energy += buffer.getRMSLevel (0, 0, 512);
                ++blocks;
                juce::Thread::sleep (2);
            }
            expect (energy / juce::jmax (1, blocks) > 0.05, "preview produced the stem's audio");
            expect (blocks >= 90 && blocks <= 110, "about one second of output at 48 kHz, got " + juce::String (blocks) + " blocks");
            expect (! preview.isPlaying(), "stops by itself at the end");
        }

        beginTest ("stop returns to pass-through");
        {
            StemPreview preview;
            preview.prepare (44100.0, 256);
            preview.play (stem);
            juce::AudioBuffer<float> buffer (2, 256);
            expect (preview.render (buffer));
            preview.stop();
            expect (! preview.render (buffer));
            expect (! preview.isPlaying());
        }
    }
};

static SeparationJobTests separationJobTests;
static StemPreviewTests stemPreviewTests;

int renderSnapshot (const juce::File& out);

int main (int argc, char* argv[])
{
    juce::ScopedJuceInitialiser_GUI init;
    if (argc == 3 && juce::String (argv[1]) == "--snapshot")
        return renderSnapshot (juce::File (juce::String (argv[2])));

    juce::UnitTestRunner runner;
    runner.setAssertOnFailure (false);
    runner.runTestsInCategory ("VoxSplit");

    int failures = 0;
    for (int i = 0; i < runner.getNumResults(); ++i)
        failures += runner.getResult (i)->failures;
    std::cout << (failures == 0 ? "ALL TESTS PASSED" : "TESTS FAILED: " + std::to_string (failures)) << std::endl;
    return failures == 0 ? 0 : 1;
}
