#pragma once
#include <array>
#include <cstdint>
#include <cstring>
#include <exception>

namespace sledge::hyprland {
// Lifecycle-only snapshot, never callback I/O. Covers the stock x86_64 minimum
// jump overwrite (5 bytes plus instruction boundary padding). The snapshot is
// deliberately larger than HOOK_TRAMPOLINE_MAX_SIZE; it is not an ABI identity.
class HookBytes {
    const void* source = nullptr;
    std::array<uint8_t,64> before{};
public:
    void snapshot(const void* address) noexcept {
        source = address;
        std::memcpy(before.data(),source,before.size());
    }
    void clear() noexcept { source = nullptr; }
    bool restored() const noexcept {
        return !source || std::memcmp(before.data(),source,before.size()) == 0;
    }
    void requireRestored() const noexcept { if (!restored()) std::terminate(); }
};
} // namespace sledge::hyprland
