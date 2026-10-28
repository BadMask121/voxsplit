#include "StemPreview.h"

StemPreview::StemPreview()
{
    formats.registerBasicFormats();
    readAhead.startThread (juce::Thread::Priority::normal);
}

StemPreview::~StemPreview()
{
    stop();
    transport.setSource (nullptr);
    readAhead.stopThread (2000);
}

void StemPreview::prepare (double sampleRate, int maxBlockSize)
{
    transport.prepareToPlay (maxBlockSize, sampleRate);
}

void StemPreview::release()
{
    transport.releaseResources();
}

void StemPreview::play (const juce::File& f)
{
    stop();
    transport.setSource (nullptr);
    source.reset();
    file = juce::File();

    std::unique_ptr<juce::AudioFormatReader> reader { formats.createReaderFor (f) };
    if (reader == nullptr)
        return;

    const auto rate = reader->sampleRate;
    source = std::make_unique<juce::AudioFormatReaderSource> (reader.release(), true);
    // 2 s of read-ahead at 44.1 kHz; the transport resamples to the host rate.
    transport.setSource (source.get(), 88200, &readAhead, rate, 2);
    transport.setPosition (0.0);
    file = f;
    active = true;
    transport.start();
}

void StemPreview::stop()
{
    active = false;
    transport.stop();
}

float StemPreview::getProgress() const
{
    const auto length = transport.getLengthInSeconds();
    return length > 0.0 ? (float) juce::jlimit (0.0, 1.0, transport.getCurrentPosition() / length) : 0.0f;
}

bool StemPreview::render (juce::AudioBuffer<float>& buffer)
{
    if (! active.load())
        return false;

    if (! transport.isPlaying()) // reached the end
    {
        active = false;
        return false;
    }

    juce::AudioSourceChannelInfo info (&buffer, 0, buffer.getNumSamples());
    transport.getNextAudioBlock (info);
    return true;
}
