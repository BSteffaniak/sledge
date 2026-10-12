#pragma once

#include "routing.hpp"
#include <array>
#include <limits>

namespace sledge::hyprland {
// Physical device registration is a lifecycle operation, not key-path discovery.
// The caller proves nonvirtual evdev provenance and builds the keymap translation
// before publishing the slot. No names, pointer reuse or daemon identities qualify
// a device. InputManager codes for virtual keyboards are NOT evdev identities.
class PhysicalDevices {
    struct Device {
        const void* identity = nullptr;
        uint64_t generation = 0;
        std::array<uint32_t, keyCount> physical{};
        std::array<bool, keyCount> held{};
    };
    std::array<Device, deviceCount> devices{};
    uint64_t next = 1;
public:
    static constexpr uint32_t unmapped = keyCount;
    uint64_t attach(const void* identity, bool verifiedEvdev, bool virtualDevice,
                    const std::array<uint32_t, keyCount>& translation, Router& router) {
        if (!identity || !verifiedEvdev || virtualDevice || next == std::numeric_limits<uint64_t>::max()) return 0;
        for (const auto& device : devices) if (device.identity == identity) return 0;
        // Require injectivity: otherwise two raw presses can erase one physical
        // owner, allowing a consumed hold to leak on release or focus enter.
        std::array<bool, keyCount> seen{};
        for (const auto key : translation) {
            if (key == unmapped) continue;
            if (key >= keyCount || seen[key]) return 0;
            seen[key] = true;
        }
        for (auto& device : devices) {
            if (device.identity) continue;
            const auto generation = next++;
            if (!router.add(generation)) return 0;
            device = {identity, generation, translation, {}};
            return generation;
        }
        return 0;
    }
    std::optional<Event> event(const void* identity, uint32_t raw, bool down) {
        for (auto& device : devices) {
            if (device.identity != identity || !identity || raw >= keyCount) continue;
            const auto key = device.physical[raw];
            if (key == unmapped) return {};
            const auto state = down ? (device.held[raw] ? State::Repeat : State::Down) : State::Up;
            // An unsolicited release must not become a modeled application up.
            if (!down && !device.held[raw]) return {};
            device.held[raw] = down;
            return Event{device.generation, key, state};
        }
        return {};
    }
    bool detach(const void* identity, Router& router, Output& output) {
        for (auto& device : devices) {
            if (!identity || device.identity != identity) continue;
            router.retire(device.generation, output);
            device = {};
            return true;
        }
        return false;
    }
};
} // namespace sledge::hyprland
