// `VoxSplitTests --snapshot out.png` renders widgets off-screen for a visual check,
// without opening a window.

#include <juce_gui_basics/juce_gui_basics.h>

#include "../Source/Theme.h"
#include "../Source/Widgets.h"

namespace
{
    struct SettingsRow final : juce::Component
    {
        vox::SegmentedToggle quality { { "Best", "Fast" } };
        vox::PassSlider passes { 1, 5 };
        juce::Rectangle<int> qualityLabel, passesLabel, value;

        SettingsRow (int selected, int passCount)
        {
            addAndMakeVisible (quality);
            addAndMakeVisible (passes);
            quality.setSelected (selected, false);
            passes.setValue (passCount, false);
        }

        void resized() override
        {
            auto r = getLocalBounds().reduced (24, 8);
            qualityLabel = r.removeFromLeft (54);
            quality.setBounds (r.removeFromLeft (124));
            r.removeFromLeft (28);
            passesLabel = r.removeFromLeft (118);
            value = r.removeFromRight (18);
            passes.setBounds (r.reduced (6, 0));
        }

        void paint (juce::Graphics& g) override
        {
            g.fillAll (vox::colours::background);
            g.setFont (vox::font (12.5f));
            g.setColour (vox::colours::textDim);
            g.drawText ("Quality", qualityLabel, juce::Justification::centredLeft);
            g.drawText ("Refinement passes", passesLabel, juce::Justification::centredLeft);
            g.setColour (vox::colours::text);
            g.setFont (vox::font (13.0f, true));
            g.drawText (juce::String (passes.getValue()), value, juce::Justification::centredRight);
        }
    };
}

int renderSnapshot (const juce::File& out)
{
    juce::Image image (juce::Image::ARGB, 540 * 2, 48 * 3 * 2, true);
    juce::Graphics g (image);
    g.addTransform (juce::AffineTransform::scale (2.0f));
    int y = 0;
    for (auto [selected, count] : { std::pair { 0, 1 }, std::pair { 1, 3 }, std::pair { 0, 5 } })
    {
        SettingsRow row (selected, count);
        row.setBounds (0, 0, 540, 48);
        g.saveState();
        g.setOrigin (0, y);
        row.paintEntireComponent (g, true);
        g.restoreState();
        y += 48;
    }
    juce::PNGImageFormat png;
    out.deleteFile();
    juce::FileOutputStream stream (out);
    return png.writeImageToStream (image, stream) ? 0 : 1;
}
