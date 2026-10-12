#pragma once

#include <array>
#include <cstddef>
#include <exception>

namespace sledge::hyprland {
// Installation happens on the compositor thread, before accepting input. Hook
// objects remain host-owned. Retain them until every detour has been restored.
// A failed restoration cannot safely return into a plugin about to be unmapped.
template<class Hook, std::size_t Capacity> class HookTransaction {
    std::array<Hook*, Capacity> hooks{};
    std::size_t count = 0;
public:
    HookTransaction() = default;
    HookTransaction(const HookTransaction&) = delete;
    HookTransaction& operator=(const HookTransaction&) = delete;
    ~HookTransaction() { rollback(); }
    bool install(Hook* hook) {
        if (!hook || count == Capacity) { rollback(); return false; }
        // Stock 0.52.2 hook() false returns precede source writes; unhook()
        // returns false for inactive objects. Never unhook a failed/conflicting
        // install: it is not ours. Roll back all earlier successful hooks.
        if (!hook->hook()) { rollback(); return false; }
        hooks[count++] = hook;
        return true;
    }
    void rollback() noexcept {
        while (count != 0) {
            auto*& hook = hooks[--count];
            if (!hook->unhook()) std::terminate();
            hook = nullptr;
        }
    }
    std::size_t size() const noexcept { return count; }
};
} // namespace sledge::hyprland
