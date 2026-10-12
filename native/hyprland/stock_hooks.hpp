#pragma once

#include "hook_transaction.hpp"
#include "hook_bytes.hpp"
#include "identity_policy.hpp"
#include <hyprland/src/plugins/PluginAPI.hpp>
#include <array>
#include <stdexcept>
#include <string>

namespace sledge::hyprland {
// Stock 0.52.2 x86_64 ABI. Resolve the complete set before the first mutation.
// This is a lifecycle boundary, never called from an input callback. Production
// admission still rejects every artifact; the VM investigation is separate.
class StockHooks {
    HANDLE owner;
    HookTransaction<CFunctionHook,9> transaction;
    std::array<CFunctionHook*,9> hooks{};
    std::array<HookBytes,9> bytes{};
public:
    enum Index : unsigned { InputKey, InputMods, SeatKey, SeatMods, Focus, ImeKey, ImeMods, LockEntry, LockQuery };
    explicit StockHooks(HANDLE handle) : owner(handle) {}
    StockHooks(const StockHooks&) = delete;
    StockHooks& operator=(const StockHooks&) = delete;
    ~StockHooks() { reset(); }
    void installPassive(std::string_view executable, std::string_view serverHash,
                        std::string_view clientHash, const std::array<void*,9>& destinations) {
        if (executable != "/nix/store/n13h1pmiv6n05w0lkd7fbaywg1fc9b4p-hyprland-0.52.2/bin/.Hyprland-wrapped" ||
            serverHash.empty() || serverHash != clientHash)
            throw std::runtime_error("unknown stock passive ABI identity");
        installResolved(destinations);
    }
    void install(std::string_view artifact, const std::array<void*,9>& destinations) {
        requireQualifiedArtifact(artifact); // SDK/API hash equality is not qualification
        installResolved(destinations);
    }
private:
    void installResolved(const std::array<void*,9>& destinations) {
        if (transaction.size()) throw std::runtime_error("stock hooks already installed");
        const std::array<std::string,9> names{"onKeyboardKey", "onKeyboardMod", "sendKeyboardKey", "sendKeyboardMods",
            "setKeyboardFocus", "sendKey", "sendMods", "onLock", "isLocked"};
        const std::array<std::string,9> signatures{
            "CInputManager::onKeyboardKey(IKeyboard::SKeyEvent const&, Hyprutils::Memory::CSharedPointer<IKeyboard>)",
            "CInputManager::onKeyboardMod(Hyprutils::Memory::CSharedPointer<IKeyboard>)",
            "CSeatManager::sendKeyboardKey(unsigned int, unsigned int, wl_keyboard_key_state)",
            "CSeatManager::sendKeyboardMods(unsigned int, unsigned int, unsigned int, unsigned int)",
            "CSeatManager::setKeyboardFocus(Hyprutils::Memory::CSharedPointer<CWLSurfaceResource>)",
            "CInputMethodKeyboardGrabV2::sendKey(unsigned int, unsigned int, wl_keyboard_key_state)",
            "CInputMethodKeyboardGrabV2::sendMods(unsigned int, unsigned int, unsigned int, unsigned int)",
            "CSessionLockProtocol::onLock(CExtSessionLockManagerV1*, unsigned int)",
            "CSessionLockProtocol::isLocked()"};
        std::array<void*,9> sources{};
        for (unsigned i = 0; i < sources.size(); ++i) {
            if (!destinations[i]) throw std::runtime_error("null stock detour");
            for (const auto& match : HyprlandAPI::findFunctionsByName(owner,names[i])) {
                if (match.demangled != signatures[i]) continue;
                if (sources[i] || !match.address) throw std::runtime_error("ambiguous stock signature: " + signatures[i]);
                sources[i] = match.address;
            }
            if (!sources[i]) throw std::runtime_error("missing stock signature: " + signatures[i]);
            for (unsigned j = 0; j < i; ++j)
                if (sources[j] == sources[i]) throw std::runtime_error("aliased stock hook addresses");
        }
        for (unsigned i = 0; i < sources.size(); ++i) bytes[i].snapshot(sources[i]);
        try {
            for (unsigned i = 0; i < sources.size(); ++i) {
                hooks[i] = HyprlandAPI::createFunctionHook(owner,sources[i],destinations[i]);
                if (!transaction.install(hooks[i])) throw std::runtime_error("stock hook installation failed");
                if (bytes[i].restored()) throw std::runtime_error("stock hook source bytes did not change");
            }
        } catch (...) { reset(); throw; }
    }
public:
    void reset() noexcept {
        transaction.rollback();
        for (auto& snapshot : bytes) { snapshot.requireRestored(); snapshot.clear(); }
        hooks.fill(nullptr);
    }
    // Original pointers are valid only while installed; never cache beyond reset.
    void* original(Index index) const noexcept {
        return hooks[index] ? hooks[index]->m_original : nullptr;
    }
};
} // namespace sledge::hyprland
