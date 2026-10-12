// Host/guest integration harness only; does not load hooks or access input.
#include "stock_worker.hpp"
#include <chrono>
#include <cstdio>
#include <sys/random.h>

int main(int argc, char** argv) {
    if (argc!=4) return 2; // endpoint, exact API hash, artifact identity
    uint64_t nonce=0;
    if (getrandom(&nonce,sizeof(nonce),0)!=sizeof(nonce)) return 2;
    sledge::hyprland::WirePacket hello{};
    if (!sledge::hyprland::wire::hello(hello,nonce,argv[2],argv[3])) return 2;
    sledge::hyprland::DecisionMailbox mailbox;
    sledge::hyprland::StockWorker worker(mailbox,argv[1],hello);
    const auto limit=std::chrono::steady_clock::now()+std::chrono::seconds(1);
    while (std::chrono::steady_clock::now()<limit) {
        if (worker.handshakeAcknowledged()) {
            puts("PASS native passive Hello/Ack: zero scope, nonzero nonce, caps=0, events=0");
            return 0;
        }
        std::this_thread::yield();
    }
    puts("FAIL native passive Hello/Ack not observed");
    return 1;
}
