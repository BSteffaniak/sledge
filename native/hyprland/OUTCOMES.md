# Transaction outcomes

WIRE.md is the single authoritative transaction/reset proposal. Earlier layouts
in this file were superseded; no parallel protocol or detour design remains.
The typed Router result is NOT final client delivery until the stock route reports
originalOutcome. Stock processing and client emission are separate instructions.
Native physical ownership persists across speculative matcher/transport resets.

Review regressions execute in routing_test.cpp: locker-held modifiers excluded
through unlock/focus/reconnect; stock-consumed repeat/up retains release-binding
and timer cleanup instructions; same-key replacement rejects before writes and
preserves original repeats/up; typed replacement/reject/partial outcomes; consumed
and forwarded ownership survives focus/reconnect. No upstream callbacks are
installed, so VM-native stock cleanup/IME/XKB tests remain required.
