#!/usr/bin/env python3
"""Source wiring regression: SDK replay must run after hooks, before Hello."""
from pathlib import Path
source = Path(__file__).with_name("stock_adapter.cpp").read_text()
init = source[source.index("APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT"):]
production = init[init.index("#else\n#ifdef SLEDGE_STOCK_CANDIDATE"):]
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
replacement = Path(__file__).with_name("stock_replacement.hpp").read_text()
assert "bool capabilitiesKnown = false" in replacement
assert "return reject(Reject::Binding)" in replacement
assert "return reject(Reject::Capabilities)" in replacement
assert "return reject(Reject::Deadline)" in replacement
print("PASS startup stock replay after production observers and before admission; strict preflight retained")
