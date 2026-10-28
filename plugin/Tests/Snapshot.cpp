// `VoxSplitTests --snapshot out.png` renders the settings block off-screen for a
// visual check, without opening a window. Mirrors PluginEditor's layout.

#include <juce_gui_basics/juce_gui_basics.h>

#include "../Source/Theme.h"
#include "../Source/Widgets.h"

namespace
{
    struct Settings final : juce::Component
    {
        struct Row
        {
            const char* name;
            const char* role;
            vox::PassSlider slider;
            juce::Rectangle<int> label, value;
        };

        vox::SegmentedToggle quality { { "Best", "Fast" } };
        Row vocal { "Vocal passes", "0 = file is vocals", { 0, 5 }, {}, {} };
        Row refine { "Refinement passes", "", { 1, 5 }, {}, {} };
        juce::Rectangle<int> qualityLabel;

        Settings (bool best, int vocalPasses, int refinePasses)
        {
            addAndMakeVisible (quality);
            quality.setSelected (best ? 0 : 1, false);
            refine.role = "More passes, cleaner lead";
            vocal.slider.setValue (vocalPasses, false);
            refine.slider.setValue (refinePasses, false);
            for (auto* r : { &vocal, &refine })
                addAndMakeVisible (r->slider);
        }

        void resized() override
        {
            auto area = getLocalBounds().reduced (24, 10);
            auto q = area.removeFromTop (32);
            qualityLabel = q.removeFromLeft (150);
            quality.setBounds (q.removeFromLeft (124));
            for (auto* r : { &vocal, &refine })
            {
                area.removeFromTop (6);
                auto row = area.removeFromTop (34);
                r->label = row.removeFromLeft (150);
                r->value = row.removeFromRight (18);
                r->slider.setBounds (row.reduced (6, 0));
            }
        }

        void paint (juce::Graphics& g) override
        {
            g.fillAll (vox::colours::background);
            g.setFont (vox::font (12.5f));
            g.setColour (vox::colours::textDim);
            g.drawText ("Quality", qualityLabel, juce::Justification::centredLeft);
            for (auto* r : { &vocal, &refine })
            {
                const float alpha = 1.0f;
                auto label = r->label;
                g.setColour (vox::colours::text.withAlpha (alpha));
                g.setFont (vox::font (12.5f, true));
                g.drawText (r->name, label.removeFromTop (label.getHeight() / 2 + 1), juce::Justification::bottomLeft);
                g.setColour (vox::colours::textDim.withAlpha (alpha));
                g.setFont (vox::font (10.5f));
                g.drawText (juce::String::fromUTF8 (r->role), label, juce::Justification::topLeft);
                g.setColour (vox::colours::text.withAlpha (alpha));
                g.setFont (vox::font (13.0f, true));
                g.drawText (juce::String (r->slider.getValue()), r->value, juce::Justification::centredRight);
            }
        }
    };
}

int renderSnapshot (const juce::File& out)
{
    constexpr int w = 540, h = 132;
    juce::Image image (juce::Image::ARGB, w * 2, h * 2 * 2, true);
    juce::Graphics g (image);
    g.addTransform (juce::AffineTransform::scale (2.0f));
    int y = 0;
    for (auto best : { true, false })
    {
        Settings settings (best, best ? 1 : 2, best ? 1 : 3);
        settings.setBounds (0, 0, w, h);
        g.saveState();
        g.setOrigin (0, y);
        settings.paintEntireComponent (g, true);
        g.restoreState();
        y += h;
    }
    juce::PNGImageFormat png;
    out.deleteFile();
    juce::FileOutputStream stream (out);
    return png.writeImageToStream (image, stream) ? 0 : 1;
}
