#pragma once

#include <juce_audio_formats/juce_audio_formats.h>
#include <juce_audio_processors/juce_audio_processors.h>

#include "SeparationJob.h"
#include "StemPreview.h"

/**
    VoxSplit as an insert effect. Audio passes through untouched unless a
    finished stem is being previewed, in which case the stem replaces the
    input on this insert. Separation runs in a separate engine process driven
    by SeparationJob, never on the audio thread. The processor owns the job so
    a run keeps going (and its result stays visible) when the host closes and
    reopens the editor.
*/
class VoxSplitProcessor final : public juce::AudioProcessor
{
public:
    VoxSplitProcessor();
    ~VoxSplitProcessor() override = default;

    void prepareToPlay (double sampleRate, int maxBlockSize) override { preview.prepare (sampleRate, maxBlockSize); }
    void releaseResources() override { preview.release(); }
    bool isBusesLayoutSupported (const BusesLayout&) const override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;

    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override { return true; }

    const juce::String getName() const override { return JucePlugin_Name; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override { return 0.0; }

    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}

    void getStateInformation (juce::MemoryBlock&) override;
    void setStateInformation (const void* data, int sizeInBytes) override;

    SeparationJob& getJob() noexcept { return job; }
    StemPreview& getPreview() noexcept { return preview; }

    juce::File getInputFile() const;
    /** Also starts estimating how long separating it will take. */
    void setInputFile (const juce::File&);

    SeparationJob::Quality getQuality() const noexcept { return quality.load(); }
    const std::shared_ptr<SeparationJob::Estimate>& getEstimate() const noexcept { return estimate; }
    void setQuality (SeparationJob::Quality q) noexcept { quality = q; }

    /** "3:10  ·  44.1 kHz  ·  FLAC", read from the file header. */
    juce::String describe (const juce::File&);

private:
    SeparationJob job;
    StemPreview preview;
    juce::AudioFormatManager formats;

    juce::CriticalSection inputLock;
    juce::File inputFile;
    std::atomic<SeparationJob::Quality> quality { SeparationJob::Quality::best };
    std::shared_ptr<SeparationJob::Estimate> estimate = std::make_shared<SeparationJob::Estimate>();

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (VoxSplitProcessor)
};
