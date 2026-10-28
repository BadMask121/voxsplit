// Headless tests for the plugin's non-UI logic: the separation job driving the
// real engine binary, and stem preview rendering. Requires a built engine and
// downloaded models (scripts/build-engine.sh, scripts/fetch-models.sh).

#include <juce_audio_devices/juce_audio_devices.h>
#include <juce_audio_formats/juce_audio_formats.h>

#include "../Source/PluginEditor.h"
#include "../Source/PluginProcessor.h"
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
            expect (job.start (input, outDir, SeparationJob::Settings { SeparationJob::Tool::leadBacking, Quality::fast, 1, 1 }, loc));
            expect (! job.start (input, outDir, SeparationJob::Settings { SeparationJob::Tool::leadBacking, Quality::fast, 1, 1 }, loc), "a second start while running must be refused");
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
            expect (estimate->total (Quality::fast, 3, 1) > estimate->total (Quality::fast, 1, 1), "extra passes add time");
            expect (estimate->total (Quality::fast, 1, 2) > estimate->total (Quality::fast, 1, 1), "extra vocal passes add time");
            expect (estimate->total (Quality::fast, 1, 0) < estimate->total (Quality::fast, 1, 1), "skipping vocals saves time");
            const SeparationJob::Settings remover { SeparationJob::Tool::vocalRemover, Quality::best, 1, 1 };
            expect (estimate->total (remover) > 0.0 && estimate->total (remover) < estimate->total (Quality::fast, 1, 1),
                    "the vocal remover is just the vocal stage");
        }

        beginTest ("best quality runs the RoFormer and reports both stems");
        {
            outDir.deleteRecursively();
            SeparationJob job;
            expect (job.start (input, outDir, SeparationJob::Settings { SeparationJob::Tool::leadBacking, Quality::best, 1, 1 }, loc));
            expect (waitFor ([&] { return ! job.isActive(); }, 300000), "best-quality job finished in time");
            const auto s = job.snapshot();
            expect (s.phase == SeparationJob::Phase::finished, "phase finished; error: " + s.error);
            expectEquals (s.outputs.size(), 2);
            expect (s.model.contains ("RoFormer"), "the karaoke stage used the RoFormer, got: " + s.model);
            for (const auto& line : s.log)
                expect (! line.containsIgnoreCase ("roformer") && ! line.containsIgnoreCase ("kim"), "no model names in the log: " + line);
            expect (s.etaSec >= 0.0 && s.etaSec < 5.0, "countdown reached ~0 at the end, got " + juce::String (s.etaSec));
        }

        beginTest ("vocal remover reports vocals and instrumental");
        {
            outDir.deleteRecursively();
            SeparationJob job;
            expect (job.start (input, outDir, SeparationJob::Settings { SeparationJob::Tool::vocalRemover, Quality::best, 1, 1 }, loc));
            expect (waitFor ([&] { return ! job.isActive(); }, 120000));
            const auto s = job.snapshot();
            expect (s.phase == SeparationJob::Phase::finished, "phase finished; error: " + s.error);
            juce::StringArray stems;
            for (const auto& o : s.outputs)
                stems.add (o.stem);
            expectEquals (stems.joinIntoString (","), juce::String ("vocals,instrumental"));
        }

        beginTest ("cancel kills the engine and leaves no partial files");
        {
            outDir.deleteRecursively();
            const auto longInput = writeTestWav (dir.getChildFile ("long.wav"), 90.0, 44100.0);
            SeparationJob job;
            expect (job.start (longInput, outDir, SeparationJob::Settings { SeparationJob::Tool::leadBacking, Quality::fast, 1, 1 }, loc));
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
            expect (! job.start (input, outDir, SeparationJob::Settings {}, missing));
            expect (job.snapshot().phase == SeparationJob::Phase::failed);
            expect (job.snapshot().error.contains ("missing"));
        }

        beginTest ("engine errors surface as failed with the engine's message");
        {
            SeparationJob job;
            const auto notAudio = dir.getChildFile ("notes.wav");
            notAudio.replaceWithText ("not audio");
            expect (job.start (notAudio, outDir, SeparationJob::Settings { SeparationJob::Tool::leadBacking, Quality::fast, 1, 1 }, loc));
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

        beginTest ("plays stems together at a different host rate, then stops at the end");
        {
            const auto second = writeTestWav (tempDir.getFile().getChildFile ("backing_vocals.wav"), 1.0, 44100.0);
            StemPreview preview;
            preview.prepare (48000.0, 512);
            preview.load ({ stem, second });
            expectEquals (preview.getNumStems(), 2);
            preview.play();
            expect (preview.isPlaying());

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
            expect (energy / juce::jmax (1, blocks) > 0.08, "both stems summed into the output");
            expect (blocks >= 90 && blocks <= 110, "about one second of output at 48 kHz, got " + juce::String (blocks) + " blocks");
            expect (! preview.isPlaying(), "stops by itself at the end");
        }

        beginTest ("muting one stem halves the level; pause returns to pass-through");
        {
            const auto second = writeTestWav (tempDir.getFile().getChildFile ("b.wav"), 2.0, 44100.0);
            const auto first = writeTestWav (tempDir.getFile().getChildFile ("a.wav"), 2.0, 44100.0);
            StemPreview preview;
            preview.prepare (44100.0, 256);
            preview.load ({ first, second });

            auto level = [&] {
                juce::AudioBuffer<float> b (2, 256);
                double sum = 0.0;
                for (int i = 0; i < 40; ++i)
                {
                    preview.render (b);
                    if (i >= 20) sum += b.getRMSLevel (0, 0, 256); // after the gain ramp
                    juce::Thread::sleep (2);
                }
                return sum / 20.0;
            };
            preview.play();
            const auto both = level();
            preview.setMuted (1, true);
            expect (preview.isMuted (1));
            const auto one = level();
            expectWithinAbsoluteError (one / both, 0.5, 0.05);

            preview.pause();
            juce::AudioBuffer<float> b (2, 256);
            expect (! preview.render (b), "paused: pass-through");
            preview.seek (0.5);
            expectWithinAbsoluteError (preview.getPositionSeconds(), 1.0, 0.05);

            preview.load ({ first, second }); // the editor reopening
            expect (preview.isMuted (1), "same stems keep their mutes");
            expectWithinAbsoluteError (preview.getPositionSeconds(), 1.0, 0.05);
            preview.load ({ second });
            expect (! preview.isMuted (0), "new stems start audible");
        }
    }
};

static SeparationJobTests separationJobTests;
static StemPreviewTests stemPreviewTests;

int renderSnapshot (const juce::File& out);

/** Runs a real separation of a short test file, then draws the plugin window
    as it opens on the finished run (stems, transport and all), off-screen. */
static int renderEditorSnapshot (const juce::File& out, SeparationJob::Tool tool, double seconds, const juce::String& name)
{
    const auto dir = juce::File::getSpecialLocation (juce::File::tempDirectory).getChildFile ("voxsplit-editor-snapshot");
    dir.createDirectory();
    const auto input = writeTestWav (dir.getChildFile (name + ".wav"), seconds, 44100.0);

    VoxSplitProcessor processor;
    processor.prepareToPlay (44100.0, 512); // as a host does before showing the editor
    processor.setTool (tool);
    processor.setInputFile (input);
    auto& job = processor.getJob();
    if (! job.start (input, SeparationJob::defaultOutputFolder (input), processor.currentSettings(), repoEngine())
        || ! waitFor ([&] { return ! job.isActive(); }, 180000)
        || job.snapshot().phase != SeparationJob::Phase::finished)
    {
        std::cerr << "separation failed: " << job.snapshot().error << std::endl;
        return 1;
    }

    std::unique_ptr<juce::AudioProcessorEditor> editor (processor.createEditor());
    juce::Image image (juce::Image::ARGB, editor->getWidth() * 2, editor->getHeight() * 2, true);
    {
        juce::Graphics g (image);
        g.addTransform (juce::AffineTransform::scale (2.0f));
        editor->paintEntireComponent (g, true);
    }
    editor.reset();
    SeparationJob::defaultOutputFolder (input).deleteRecursively();

    juce::PNGImageFormat png;
    out.deleteFile();
    juce::FileOutputStream stream (out);
    return png.writeImageToStream (image, stream) ? 0 : 1;
}

int main (int argc, char* argv[])
{
    juce::ScopedJuceInitialiser_GUI init;
    if (argc == 3 && juce::String (argv[1]) == "--snapshot")
        return renderSnapshot (juce::File (juce::String (argv[2])));
    // --snapshot-editor out.png [lead-backing|vocal-remover] [seconds] [song name]
    if (argc >= 3 && juce::String (argv[1]) == "--snapshot-editor")
        return renderEditorSnapshot (juce::File (juce::String (argv[2])),
                                     argc > 3 && juce::String (argv[3]) == "vocal-remover" ? SeparationJob::Tool::vocalRemover
                                                                                          : SeparationJob::Tool::leadBacking,
                                     argc > 4 ? juce::String (argv[4]).getDoubleValue() : 4.0,
                                     argc > 5 ? juce::String (argv[5]) : juce::String ("My Song"));

    juce::UnitTestRunner runner;
    runner.setAssertOnFailure (false);
    runner.runTestsInCategory ("VoxSplit");

    int failures = 0;
    for (int i = 0; i < runner.getNumResults(); ++i)
        failures += runner.getResult (i)->failures;
    std::cout << (failures == 0 ? "ALL TESTS PASSED" : "TESTS FAILED: " + std::to_string (failures)) << std::endl;
    return failures == 0 ? 0 : 1;
}
