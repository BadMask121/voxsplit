#pragma once

#include <juce_audio_devices/juce_audio_devices.h>
#include <juce_audio_formats/juce_audio_formats.h>

/**
    Plays a finished stem through the plugin's own output so it can be
    auditioned on the mixer insert VoxSplit sits on.

    Disk reads happen on a read-ahead thread; the audio thread only copies
    from that buffer (resampled to the host rate by AudioTransportSource).
    play()/stop() are called from the message thread, render() from the
    audio thread.
*/
class StemPreview final
{
public:
    StemPreview();
    ~StemPreview();

    void prepare (double sampleRate, int maxBlockSize);
    void release();

    void play (const juce::File&);
    void stop();

    juce::File getFile() const { return file; }
    bool isPlaying() const { return active.load() && transport.isPlaying(); }
    float getProgress() const;

    /** Audio thread. Returns true (and fills `buffer`) while a preview plays. */
    bool render (juce::AudioBuffer<float>& buffer);

private:
    juce::AudioFormatManager formats;
    juce::TimeSliceThread readAhead { "VoxSplit preview read-ahead" };
    juce::AudioTransportSource transport;
    std::unique_ptr<juce::AudioFormatReaderSource> source;
    juce::File file;
    std::atomic<bool> active { false };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (StemPreview)
};
