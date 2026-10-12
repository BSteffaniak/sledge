// Isolated stock-build investigation only. No capture or remapping.
#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/protocols/SessionLock.hpp>
#include <hyprland/src/managers/input/InputManager.hpp>
#include <hyprland/src/managers/SeatManager.hpp>
#include <hyprland/src/protocols/InputMethodV2.hpp>
#include <unistd.h>
#include <array>
#include "hook_transaction.hpp"
#include "hook_bytes.hpp"
#include <stdexcept>
#include <cstdio>
#include <cstdarg>
#include <cstdlib>

namespace {
sledge::hyprland::HookTransaction<CFunctionHook, 9> installed;
std::array<sledge::hyprland::HookBytes,9> originalBytes{};
void restore() noexcept {
    installed.rollback();
    for (auto& bytes : originalBytes) { bytes.requireRestored(); bytes.clear(); }
}
CFunctionHook* queryHook = nullptr;
CFunctionHook* lockHook = nullptr;
unsigned calls = 0;
unsigned lockCalls = 0;
unsigned earlyLockCalls = 0;
unsigned suppressed = 0;
std::array<CFunctionHook*, 7> inputHooks{};
std::array<unsigned, 7> inputCalls{};
unsigned long long traceSequence = 0;
// Research fixture ONLY: ordered file traces are deliberately not production
// callbacks. No strings are built, but fopen/fprintf still block and allocate.
void trace(const char* format, ...) {
    FILE* file = fopen("/tmp/stock-hook-events", "a");
    if (!file) return;
    fprintf(file, "seq=%llu ", ++traceSequence);
    va_list args;
    va_start(args, format);
    vfprintf(file, format, args);
    va_end(args);
    fputc('\n', file);
    fclose(file);
}
void inputKey(CInputManager* self, const IKeyboard::SKeyEvent& event, SP<IKeyboard> keyboard) {
    ++inputCalls[0];
    // Fixed virtual-keyboard fixture only: wtype's fixture "a" event code is 1 (observed).
    // This is NOT a general mapping policy or physical-keyboard qualification.
    trace("input_key key=%u state=%u time=%u device=%p virtual=%d name=%s", event.keycode,
          static_cast<unsigned>(event.state), event.timeMs, static_cast<void*>(keyboard.get()),
          keyboard->isVirtual(), keyboard->m_hlName.c_str());
    if (keyboard->isVirtual() && event.keycode == 1) {
        ++suppressed;
        trace("input_suppressed key=%u state=%u device=%p", event.keycode,
              static_cast<unsigned>(event.state), static_cast<void*>(keyboard.get()));
        return;
    }
    reinterpret_cast<void (*)(CInputManager*, const IKeyboard::SKeyEvent&, SP<IKeyboard>)>(inputHooks[0]->m_original)(self, event, keyboard);
}
void inputMod(CInputManager* self, SP<IKeyboard> keyboard) {
    ++inputCalls[1];
    trace("input_mod device=%p depressed=%u latched=%u locked=%u group=%u", static_cast<void*>(keyboard.get()),
          keyboard->m_modifiersState.depressed, keyboard->m_modifiersState.latched,
          keyboard->m_modifiersState.locked, keyboard->m_modifiersState.group);
    reinterpret_cast<void (*)(CInputManager*, SP<IKeyboard>)>(inputHooks[1]->m_original)(self, keyboard);
}
void seatKey(CSeatManager* self, uint32_t time, uint32_t key, wl_keyboard_key_state state) {
    ++inputCalls[2];
    trace("seat_key key=%u state=%u time=%u seat=%p", key, static_cast<unsigned>(state), time, static_cast<void*>(self));
    reinterpret_cast<void (*)(CSeatManager*, uint32_t, uint32_t, wl_keyboard_key_state)>(inputHooks[2]->m_original)(self, time, key, state);
}
void seatMods(CSeatManager* self, uint32_t depressed, uint32_t latched, uint32_t locked, uint32_t group) {
    ++inputCalls[3];
    trace("seat_mod depressed=%u latched=%u locked=%u group=%u seat=%p", depressed, latched, locked, group, static_cast<void*>(self));
    reinterpret_cast<void (*)(CSeatManager*, uint32_t, uint32_t, uint32_t, uint32_t)>(inputHooks[3]->m_original)(self, depressed, latched, locked, group);
}
void focus(CSeatManager* self, SP<CWLSurfaceResource> surface) {
    ++inputCalls[4];
    trace("seat_focus surface=%p seat=%p", static_cast<void*>(surface.get()), static_cast<void*>(self));
    reinterpret_cast<void (*)(CSeatManager*, SP<CWLSurfaceResource>)>(inputHooks[4]->m_original)(self, surface);
}
void imeKey(CInputMethodKeyboardGrabV2* self, uint32_t time, uint32_t key, wl_keyboard_key_state state) {
    ++inputCalls[5];
    trace("ime_key key=%u state=%u time=%u grab=%p", key, static_cast<unsigned>(state), time, static_cast<void*>(self));
    reinterpret_cast<void (*)(CInputMethodKeyboardGrabV2*, uint32_t, uint32_t, wl_keyboard_key_state)>(inputHooks[5]->m_original)(self, time, key, state);
}
void imeMods(CInputMethodKeyboardGrabV2* self, uint32_t depressed, uint32_t latched, uint32_t locked, uint32_t group) {
    ++inputCalls[6];
    trace("ime_mod depressed=%u latched=%u locked=%u group=%u grab=%p", depressed, latched, locked, group, static_cast<void*>(self));
    reinterpret_cast<void (*)(CInputMethodKeyboardGrabV2*, uint32_t, uint32_t, uint32_t, uint32_t)>(inputHooks[6]->m_original)(self, depressed, latched, locked, group);
}
void report(const char* phase) {
    FILE* file = fopen("/tmp/stock-hook-evidence", "a");
    if (!file) throw std::runtime_error("cannot retain probe evidence");
    fprintf(file, "%s query=%u lock=%u before_manager_lock=%u\n", phase, calls, lockCalls, earlyLockCalls);
    fprintf(file, "suppressed_virtual_fixture=%u\n", suppressed);
    for (unsigned i = 0; i < inputCalls.size(); ++i) fprintf(file, "input_path_%u=%u\n", i, inputCalls[i]);
    fclose(file);
}
bool query(CSessionLockProtocol* self) {
    ++calls;
    return reinterpret_cast<bool (*)(CSessionLockProtocol*)>(queryHook->m_original)(self);
}
void lock(CSessionLockProtocol* self, CExtSessionLockManagerV1* manager, uint32_t id) {
    ++lockCalls;
    if (!self->isLocked()) ++earlyLockCalls;
    trace("lock_before locked=%d manager=%p id=%u", self->isLocked(), static_cast<void*>(manager), id);
    reinterpret_cast<void (*)(CSessionLockProtocol*, CExtSessionLockManagerV1*, uint32_t)>(lockHook->m_original)(self, manager, id);
    trace("lock_after locked=%d manager=%p id=%u", self->isLocked(), static_cast<void*>(manager), id);
}
SFunctionMatch exact(HANDLE handle, const std::string& name, const std::string& signature) {
    const auto matches = HyprlandAPI::findFunctionsByName(handle, name);
    std::optional<SFunctionMatch> result;
    for (const auto& match : matches) {
        if (match.demangled == signature) {
            if (result) throw std::runtime_error("ambiguous symbol: " + signature);
            result = match;
        }
    }
    if (!result || !result->address) throw std::runtime_error("missing symbol: " + signature);
    return *result;
}
}
APICALL EXPORT std::string PLUGIN_API_VERSION() { return HYPRLAND_API_VERSION; }
APICALL EXPORT void PLUGIN_EXIT() {
    // Restore hooks before plugin code is unmapped; host removes their objects.
    restore();
    report("UNLOAD_RESTORED_BYTES_VERIFIED");
    Debug::log(LOG, "SLEDGE_STOCK_HOOK_UNLOAD_RESTORED");
}
static PLUGIN_DESCRIPTION_INFO initialize(HANDLE handle) {
    std::array<char, 4096> executable{};
    const auto size = readlink("/proc/self/exe", executable.data(), executable.size());
    const std::string expected = "/nix/store/n13h1pmiv6n05w0lkd7fbaywg1fc9b4p-hyprland-0.52.2/bin/.Hyprland-wrapped";
    if (size < 0 || std::string(executable.data(), static_cast<size_t>(size)) != expected)
        throw std::runtime_error("not the exact isolated test runtime");
    if (std::string(__hyprland_api_get_hash()) != __hyprland_api_get_client_hash())
        throw std::runtime_error("SDK ABI hash mismatch");
    // A registered exact store artifact is required: unknown commit equality alone is insufficient.
    const auto q = exact(handle, "isLocked", "CSessionLockProtocol::isLocked()");
    const auto l = exact(handle, "onLock", "CSessionLockProtocol::onLock(CExtSessionLockManagerV1*, unsigned int)");
    originalBytes[0].snapshot(q.address);
    originalBytes[1].snapshot(l.address);
    queryHook = HyprlandAPI::createFunctionHook(handle, q.address, reinterpret_cast<void*>(&query));
    lockHook = HyprlandAPI::createFunctionHook(handle, l.address, reinterpret_cast<void*>(&lock));
    if (!installed.install(queryHook) || !installed.install(lockHook))
        throw std::runtime_error("stock hook installation failed");
    if (originalBytes[0].restored() || originalBytes[1].restored())
        throw std::runtime_error("stock hook source bytes did not change");
    const bool before = reinterpret_cast<bool (*)(CSessionLockProtocol*)>(queryHook->m_original)(PROTO::sessionLock.get());
    const bool after = reinterpret_cast<bool (*)(CSessionLockProtocol*)>(q.address)(PROTO::sessionLock.get());
    if (before != after || calls != 1) throw std::runtime_error("trampoline self-test failed");
    const std::array<std::string, 7> names{"onKeyboardKey", "onKeyboardMod", "sendKeyboardKey", "sendKeyboardMods", "setKeyboardFocus", "sendKey", "sendMods"};
    const std::array<std::string, 7> signatures{
        "CInputManager::onKeyboardKey(IKeyboard::SKeyEvent const&, Hyprutils::Memory::CSharedPointer<IKeyboard>)",
        "CInputManager::onKeyboardMod(Hyprutils::Memory::CSharedPointer<IKeyboard>)",
        "CSeatManager::sendKeyboardKey(unsigned int, unsigned int, wl_keyboard_key_state)",
        "CSeatManager::sendKeyboardMods(unsigned int, unsigned int, unsigned int, unsigned int)",
        "CSeatManager::setKeyboardFocus(Hyprutils::Memory::CSharedPointer<CWLSurfaceResource>)",
        "CInputMethodKeyboardGrabV2::sendKey(unsigned int, unsigned int, wl_keyboard_key_state)",
        "CInputMethodKeyboardGrabV2::sendMods(unsigned int, unsigned int, unsigned int, unsigned int)"};
    const std::array<void*, 7> destinations{reinterpret_cast<void*>(&inputKey), reinterpret_cast<void*>(&inputMod),
        reinterpret_cast<void*>(&seatKey), reinterpret_cast<void*>(&seatMods), reinterpret_cast<void*>(&focus),
        reinterpret_cast<void*>(&imeKey), reinterpret_cast<void*>(&imeMods)};
    for (unsigned i = 0; i < inputHooks.size(); ++i) {
        const auto match = exact(handle, names[i], signatures[i]);
        originalBytes[i+2].snapshot(match.address);
        inputHooks[i] = HyprlandAPI::createFunctionHook(handle, match.address, destinations[i]);
        if (!installed.install(inputHooks[i])) throw std::runtime_error("input path hook installation failed: " + signatures[i]);
        if (originalBytes[i+2].restored()) throw std::runtime_error("input hook source bytes did not change");
    }
    report("QUERY_TRAMPOLINE_PASS_LOCK_INSTALLED");
    // Explicit research-only rollback fixture, not a capture override. Restore
    // after all nine hooks and self-test have succeeded, verify every byte, then
    // leave the loaded plugin completely passive for host load/list/unload.
    if (const auto* rollback = std::getenv("SLEDGE_STOCK_ROLLBACK_ONLY"); rollback && std::string_view(rollback)=="1") {
        restore();
        report("ROLLBACK_ALL_NINE_BYTES_VERIFIED");
    }
    Debug::log(LOG, "SLEDGE_STOCK_QUERY_TRAMPOLINE_PASS_LOCK_INSTALLED_NOT_EXERCISED");
    return {"sledge-stock-hook-investigation", "No production capture; isolated stock trampoline test", "sledge", "0"};
}
APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    try { return initialize(handle); }
    catch (...) {
        restore(); // includes before/after byte checks on failed initialization
        throw;
    }
}
