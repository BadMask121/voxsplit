import SwiftUI

/// Mirrors plugin/Source/Theme.h so the app and the plugin look and move the same.
enum Theme {
    static let background = Color(hex: 0x121214)
    static let surface = Color(hex: 0x1A1A1E)
    static let surfaceHi = Color(hex: 0x24242A)
    static let border = Color(hex: 0x2E2E35)
    static let text = Color(hex: 0xF2F2F4)
    static let textDim = Color(hex: 0x9B9BA4)
    static let lead = Color(hex: 0xFF8A5B)
    static let backing = Color(hex: 0x7C9CFF)
    static let instrumental = Color(hex: 0x4FD1C5)
    static let success = Color(hex: 0x4CD38A)
    static let danger = Color(hex: 0xFF6359)

    // Durations in seconds. UI motion stays under 300 ms; exits are faster than entrances.
    static let press = 0.10
    static let release = 0.16
    static let hover = 0.15
    static let hoverOut = 0.11
    static let enter = 0.22
    static let stagger = 0.06
    static let fade = 0.18

    /// cubic-bezier(0.23, 1, 0.32, 1): strong ease-out for anything entering or responding.
    static func easeOut(_ duration: Double) -> Animation {
        .timingCurve(0.23, 1, 0.32, 1, duration: duration)
    }
}

extension Color {
    nonisolated init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}
