#!/usr/bin/env python3
"""Source wiring regression: SDK replay must run after hooks, before Hello."""
from pathlib import Path
source = Path(__file__).with_name("stock_adapter.cpp").read_text()
init = source[source.index("APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT"):]
production = init[init.index("#else\n#if defined(SLEDGE_STOCK_CANDIDATE)"):]
hooks = production.index("adapter->research->hooks.install")
refresh = production.index("refreshStockOutput(")
hello = production.index("wire::hello(")
assert hooks < refresh < hello
assert "g_pInputManager->updateCapabilities()" in production[refresh:hello]
assert "reconcileStartupOutput()" in production[refresh:hello]
replay = source[source.index("void reconcileStartupOutput()"):source.index("void installRegistryRetry()")]
assert "resource->sendEnter(surface,&keys)" in replay
assert "getKeysFromAllKBs().empty()" in replay
assert "resource->m_owner.lock()!=owner" in replay
assert "resendEnterEvents" not in replay and "setKeyboardFocus" not in replay
assert "mods.latched,mods.locked,mods.group" in replay
keypath = source[source.index("void inputKey("):source.index("void seatKey(")]
assert keypath.index("g_pSeatManager->setKeyboard(keyboard)") < keypath.index("reconcileInitial(keyboard.get()") < keypath.index("research->router.begin(")
assert "adapter->replacement.preparedFor(keyboard->m_xkbKeymap)" in keypath
assert "research->selectConsumer(keyboard)" in keypath
assert "if (!selection.retainsAdmission()) beginLifecycle();" in source
assert "retireGenerationForDrain(adapter->worker->generation(),true)" in source
assert "if (previous!=adapter->research->registeredCount) beginLifecycle();" in source
assert "if (beforeModifiers && !output.reconcileModifiers(0))" in Path(__file__).with_name("routing.hpp").read_text()
focus = source[source.index("void focus("):source.index("void imeKey(")]
assert focus.index("invalidateFocusProof(") < focus.index("lifecycle(nullptr,7,0,2)")
assert "invalidateFocusBindings()" in focus
assert "adapter->replacement.recoveryBindingQualified(research) && research.physicalQuiescent" in source
enter = source[source.index("void enter(CWLKeyboardResource*"):source.index("void resourceMods(")]
assert "if (completion.observed()) adapter->replacement.entered(self,surface)" in enter
assert "adapter->replacement.entered(self,surface);" not in enter[:enter.index("const auto finish")]
assert "nativeOutputReconciled(false,true)" in source
assert "adapter->lifecycleDirty,adapter->startupOutputPending,retiring" in source
replacement = Path(__file__).with_name("stock_replacement.hpp").read_text()
assert "bool capabilitiesKnown = false" in replacement
assert "return reject(Reject::Binding)" in replacement
assert "return reject(Reject::Capabilities)" in replacement
assert "return reject(Reject::Deadline)" in replacement
print("PASS startup stock replay after production observers and before admission; strict preflight retained")
