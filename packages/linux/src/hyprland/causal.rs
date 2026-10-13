//! Bounded worker-side transaction timestamps, logged only after settlement.
use super::wire::Scope;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CausalTerminal {
    Acknowledged,
    Uncertain,
    Rejected,
}
#[derive(Clone, Copy, Debug)]
pub struct CausalTrace {
    pub scope: Scope,
    pub received_ns: Option<u64>,
    /// Local send invocation interval; receiver may run before local return.
    pub decision_send_start_ns: Option<u64>,
    pub decision_send_return_ns: Option<u64>,
    /// Successful local completion only, never peer receipt proof.
    pub decision_sent_ns: Option<u64>,
    pub outcome_received_ns: Option<u64>,
    pub terminal_ns: Option<u64>,
    pub terminal: CausalTerminal,
}
impl CausalTrace {
    pub(super) fn received(scope: Scope) -> Self {
        Self {
            scope,
            received_ns: super::transport::monotonic_ns().ok(),
            decision_send_start_ns: None,
            decision_send_return_ns: None,
            decision_sent_ns: None,
            outcome_received_ns: None,
            terminal_ns: None,
            terminal: CausalTerminal::Uncertain,
        }
    }
}
