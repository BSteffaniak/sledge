#include "DeliveredKeyboardState.hpp"
#include <iostream>
#include <stdexcept>

void check(bool value) { if (!value) throw std::runtime_error("delivered state regression"); }
int main() {
    auto* context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    check(context);
    xkb_rule_names names{}; names.layout = "us";
    auto* map = xkb_keymap_new_from_names(context,&names,XKB_KEYMAP_COMPILE_NO_FLAGS);
    check(map);
    {
        DeliveredKeyboardState a, b;
        check(a.setKeymap(map)); check(b.setKeymap(map));
        check(a.depressed() == 0);
        a.key(42,true); check(a.contains(42) && a.depressed() != 0);
        a.key(42,true); a.key(42,false); check(a.depressed() == 0); // duplicate down not two holds
        a.key(42,true); b.key(42,true); a.key(42,false);
        check(a.depressed() == 0 && b.depressed() != 0);
        check(b.setKeymap(map)); check(b.contains(42) && b.depressed() != 0);
        check(!b.setKeymap(nullptr)); check(b.depressed() != 0); // failure preserves snapshot
        b.clear(); check(!b.contains(42) && b.depressed() == 0);
        a.key(900,true); check(!a.contains(900));
        // A physical modifier not consumer-admitted never enters shadow state.
        check(a.depressed() == 0);
    }
    xkb_keymap_unref(map); xkb_context_unref(context);
    std::cout << "PASS real XKB delivered lifecycle: duplicate, per-device, keymap rebuild, failure, clear\n";
}
