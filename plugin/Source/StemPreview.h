#pragma once

#include <juce_audio_devices/juce_audio_devices.h>
#include <juce_audio_formats/juce_audio_formats.h>

/**
    Plays the finished stems together, in sync, through the plugin's own output
    so they can be auditioned on the mixer insert VoxSplit sits on. Each stem
    can be muted on its own (lead only, backing only, or both).

    Threading: load/play/pause/seek/setMuted run on the message thread and take
    `lock`; render() runs on the audio thread and only *tries* the lock, so it
    never waits (a block that loses the race passes the input through). Disk
    reads happen on a read-ahead thread; the transports resample to the host rate.
*/
class StemPreview final
{
public:
    StemPreview();
    ~StemPreview();

    void prepare (double sampleRate, int maxBlockSize);
    void release();

    /** Replaces the loaded stems (stopped, at the start, all audible). Loading
        the same files again keeps the current position and mutes. */
    void load (const juce::Array<juce::File>& files);
    void clear() { load ({}); }

    void play();
    void pause();
    void togglePlay() { isPlaying() ? pause() : play(); }
    void seek (double fraction);

    void setMuted (int index, bool muted);
    bool isMuted (int index) const;

    int getNumStems() const;
    juce::File getFile (int index) const;
    bool isPlaying() const { return playing.load(); }
    double getLengthSeconds() const;
    double getPositionSeconds() const;

    /** Audio thread. Returns true (and fills `buffer`) while the stems play. */
    bool render (juce::AudioBuffer<float>& buffer);

private:
    struct Stem
    {
        juce::File file;
        std::unique_ptr<juce::AudioFormatReaderSource> source;
        juce::AudioTransportSource transport;
        std::atomic<bool> muted { false };
    };

    void stopAll();

    juce::AudioFormatManager formats;
    juce::TimeSliceThread readAhead { "VoxSplit preview read-ahead" };
    juce::OwnedArray<Stem> stems;
    juce::AudioBuffer<float> scratch;
    double sampleRate = 44100.0;
    int blockSize = 512;
    bool prepared = false;
    std::atomic<bool> playing { false };
    juce::CriticalSection lock;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (StemPreview)
};
