// Qualification probe only: deliberately installs no hooks or listeners.
#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/managers/input/InputManager.hpp>
#include <hyprland/src/protocols/SessionLock.hpp>

#include "identity_policy.hpp"
#include <type_traits>

static_assert(std::is_same_v<decltype(&CInputManager::onKeyboardKey),
                            void (CInputManager::*)(const IKeyboard::SKeyEvent&, SP<IKeyboard>)>);
static_assert(std::is_same_v<decltype(&CInputManager::onKeyboardMod),
                            void (CInputManager::*)(SP<IKeyboard>)>);
static_assert(std::is_same_v<decltype(&CSessionLockProtocol::isLocked),
                            bool (CSessionLockProtocol::*)()>);
static_assert(std::is_same_v<decltype(IKeyboard::SKeyEvent::keycode), uint32_t>);

// These exports follow Hyprland's C++ plugin ABI, not a C-compatible ABI.
#ifdef __clang__
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wreturn-type-c-linkage"
#endif

APICALL EXPORT std::string PLUGIN_API_VERSION() {
    return HYPRLAND_API_VERSION;
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE) {
    // Reject before calling into the host: "unknown" cannot identify an ABI.
    sledge::hyprland::requireQualifiedArtifact(GIT_COMMIT_HASH);
    throw std::runtime_error("sledge qualification probe: qualification policy unexpectedly returned");
}

APICALL EXPORT void PLUGIN_EXIT() {}

#ifdef __clang__
#pragma clang diagnostic pop
#endif
