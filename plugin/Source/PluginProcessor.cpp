#include "PluginProcessor.h"

#include "PluginEditor.h"

VoxSplitProcessor::VoxSplitProcessor()
    : juce::AudioProcessor (BusesProperties()
                                .withInput ("Input", juce::AudioChannelSet::stereo(), true)
                                .withOutput ("Output", juce::AudioChannelSet::stereo(), true))
{
    formats.registerBasicFormats();
}

bool VoxSplitProcessor::isBusesLayoutSupported (const BusesLayout& layout) const
{
    const auto out = layout.getMainOutputChannelSet();
    return (out == juce::AudioChannelSet::mono() || out == juce::AudioChannelSet::stereo())
        && out == layout.getMainInputChannelSet();
}

void VoxSplitProcessor::processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer&)
{
    juce::ScopedNoDenormals noDenormals;
    for (auto ch = getTotalNumInputChannels(); ch < getTotalNumOutputChannels(); ++ch)
        buffer.clear (ch, 0, buffer.getNumSamples());

    // Previewing a stem replaces the input; otherwise this is a pass-through.
    // Never touches the separation job or the file system.
    preview.render (buffer);
}

juce::AudioProcessorEditor* VoxSplitProcessor::createEditor()
{
    return new VoxSplitEditor (*this);
}

juce::File VoxSplitProcessor::getInputFile() const
{
    const juce::ScopedLock sl (inputLock);
    return inputFile;
}

void VoxSplitProcessor::setInputFile (const juce::File& f)
{
    {
        const juce::ScopedLock sl (inputLock);
        inputFile = f;
    }
    if (f.existsAsFile())
        SeparationJob::requestEstimate (f, estimate);
}

juce::String VoxSplitProcessor::describe (const juce::File& f)
{
    const auto dot = juce::String ("  ") + juce::String::fromUTF8 ("\xc2\xb7") + "  ";
    const auto format = f.getFileExtension().trimCharactersAtStart (".").toUpperCase();

    if (std::unique_ptr<juce::AudioFormatReader> reader { formats.createReaderFor (f) })
    {
        if (reader->sampleRate > 0)
        {
            const auto seconds = (int) std::round ((double) reader->lengthInSamples / reader->sampleRate);
            const auto duration = juce::String (seconds / 60) + ":" + juce::String (seconds % 60).paddedLeft ('0', 2);
            const auto rate = juce::String (reader->sampleRate / 1000.0, reader->sampleRate == 48000.0 ? 0 : 1) + " kHz";
            return duration + dot + rate + dot + format;
        }
    }
    return format;
}

void VoxSplitProcessor::getStateInformation (juce::MemoryBlock& dest)
{
    juce::ValueTree state ("VoxSplit");
    state.setProperty ("input", getInputFile().getFullPathName(), nullptr);
    state.setProperty ("quality", getQuality() == SeparationJob::Quality::fast ? "fast" : "best", nullptr);

    const auto snap = job.snapshot();
    if (snap.phase == SeparationJob::Phase::finished)
        for (const auto& out : snap.outputs)
        {
            juce::ValueTree o ("Output");
            o.setProperty ("stem", out.stem, nullptr);
            o.setProperty ("path", out.file.getFullPathName(), nullptr);
            state.appendChild (o, nullptr);
        }

    if (const auto xml = state.createXml())
        copyXmlToBinary (*xml, dest);
}

void VoxSplitProcessor::setStateInformation (const void* data, int sizeInBytes)
{
    const auto xml = getXmlFromBinary (data, sizeInBytes);
    if (xml == nullptr)
        return;

    const auto state = juce::ValueTree::fromXml (*xml);
    if (! state.hasType ("VoxSplit"))
        return;

    const juce::String path = state.getProperty ("input").toString();
    const auto input = juce::File::isAbsolutePath (path) ? juce::File (path) : juce::File();
    setInputFile (input);
    setQuality (state.getProperty ("quality").toString() == "fast" ? SeparationJob::Quality::fast
                                                                   : SeparationJob::Quality::best);

    juce::Array<SeparationJob::Output> outputs;
    for (const auto& o : state)
        if (o.hasType ("Output"))
            outputs.add ({ o.getProperty ("stem").toString(), juce::File (o.getProperty ("path").toString()) });

    job.restore (input, outputs);
}

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter()
{
    return new VoxSplitProcessor();
}
