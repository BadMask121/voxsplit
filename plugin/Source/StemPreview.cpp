#include "StemPreview.h"

StemPreview::StemPreview()
{
    formats.registerBasicFormats();
    readAhead.startThread (juce::Thread::Priority::normal);
}

StemPreview::~StemPreview()
{
    clear();
    readAhead.stopThread (2000);
}

void StemPreview::prepare (double rate, int maxBlockSize)
{
    const juce::ScopedLock sl (lock);
    sampleRate = rate;
    blockSize = juce::jmax (1, maxBlockSize);
    scratch.setSize (2, blockSize, false, false, true);
    for (auto* stem : stems)
        stem->transport.prepareToPlay (blockSize, sampleRate);
    prepared = true;
}

void StemPreview::release()
{
    const juce::ScopedLock sl (lock);
    for (auto* stem : stems)
        stem->transport.releaseResources();
    prepared = false;
}

void StemPreview::load (const juce::Array<juce::File>& files)
{
    {
        // Reopening the editor reloads the same stems: keep position and mutes.
        const juce::ScopedLock sl (lock);
        if (files.size() == stems.size()
            && std::equal (files.begin(), files.end(), stems.begin(), [] (const auto& f, const auto* s) { return f == s->file; }))
            return;
    }

    juce::OwnedArray<Stem> fresh;
    for (const auto& f : files)
    {
        std::unique_ptr<juce::AudioFormatReader> reader { formats.createReaderFor (f) };
        if (reader == nullptr)
            continue;
        auto* stem = fresh.add (new Stem());
        stem->file = f;
        const auto rate = reader->sampleRate;
        stem->source = std::make_unique<juce::AudioFormatReaderSource> (reader.release(), true);
        if (prepared)
            stem->transport.prepareToPlay (blockSize, sampleRate);
        // 2 s of read-ahead at 44.1 kHz; the transport resamples to the host rate.
        stem->transport.setSource (stem->source.get(), 88200, &readAhead, rate, 2);
    }

    juce::OwnedArray<Stem> old;
    {
        const juce::ScopedLock sl (lock);
        playing = false;
        stems.swapWith (fresh);
        old.swapWith (fresh);
    }
    for (auto* stem : old) // outside the lock: tearing down sources can block
        stem->transport.setSource (nullptr);
}

void StemPreview::play()
{
    const juce::ScopedLock sl (lock); // render() can't run mid-way: all start on the same block
    if (stems.isEmpty())
        return;
    // Finished last time: start over.
    if (getPositionSeconds() >= getLengthSeconds() - 0.01)
        for (auto* stem : stems)
            stem->transport.setPosition (0.0);
    for (auto* stem : stems)
        stem->transport.start();
    playing = true;
}

void StemPreview::pause()
{
    const juce::ScopedLock sl (lock);
    stopAll();
}

void StemPreview::stopAll()
{
    for (auto* stem : stems)
        stem->transport.stop();
    playing = false;
}

void StemPreview::seek (double fraction)
{
    const juce::ScopedLock sl (lock);
    const auto seconds = juce::jlimit (0.0, 1.0, fraction) * getLengthSeconds();
    for (auto* stem : stems)
        stem->transport.setPosition (seconds);
}

void StemPreview::setMuted (int index, bool muted)
{
    const juce::ScopedLock sl (lock);
    if (auto* stem = stems[index])
    {
        stem->muted = muted;
        // The transport ramps between gains over a block, so muting doesn't click.
        stem->transport.setGain (muted ? 0.0f : 1.0f);
    }
}

bool StemPreview::isMuted (int index) const
{
    const juce::ScopedLock sl (lock);
    const auto* stem = stems[index];
    return stem != nullptr && stem->muted.load();
}

int StemPreview::getNumStems() const
{
    const juce::ScopedLock sl (lock);
    return stems.size();
}

juce::File StemPreview::getFile (int index) const
{
    const juce::ScopedLock sl (lock);
    const auto* stem = stems[index];
    return stem != nullptr ? stem->file : juce::File();
}

double StemPreview::getLengthSeconds() const
{
    const juce::ScopedLock sl (lock);
    return stems.isEmpty() ? 0.0 : stems.getFirst()->transport.getLengthInSeconds();
}

double StemPreview::getPositionSeconds() const
{
    const juce::ScopedLock sl (lock);
    return stems.isEmpty() ? 0.0 : stems.getFirst()->transport.getCurrentPosition();
}

bool StemPreview::render (juce::AudioBuffer<float>& buffer)
{
    if (! playing.load())
        return false;

    const juce::ScopedTryLock sl (lock);
    if (! sl.isLocked() || stems.isEmpty() || scratch.getNumSamples() == 0)
        return false;

    bool any = false;
    for (auto* stem : stems)
        any = any || stem->transport.isPlaying();
    if (! any) // reached the end
    {
        playing = false;
        return false;
    }

    const auto numSamples = buffer.getNumSamples();
    const auto numChannels = buffer.getNumChannels();
    buffer.clear();
    // Sum each stem, a scratch-sized slice at a time (no allocation here).
    for (int offset = 0; offset < numSamples; offset += scratch.getNumSamples())
    {
        const auto n = juce::jmin (scratch.getNumSamples(), numSamples - offset);
        for (auto* stem : stems)
        {
            juce::AudioSourceChannelInfo info (&scratch, 0, n);
            stem->transport.getNextAudioBlock (info);
            for (int ch = 0; ch < numChannels; ++ch)
                buffer.addFrom (ch, offset, scratch, ch % scratch.getNumChannels(), 0, n);
        }
    }
    return true;
}
