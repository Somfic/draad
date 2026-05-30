// Example app code consuming the draad-generated client.
//
// `./schema/index.ts` is regenerated on every `cargo build -p hello` and
// gitignored. Everything in this file is hand-written.
//
// Run with `bun install && bun start`. Bun ships `fetch` and `WebSocket`
// as globals and runs TypeScript natively, no bundler / tsx needed.

import { Api, defaultRpc, RpcError } from "./schema/index.js";

const api = new Api(
    defaultRpc({
        baseUrl: "http://localhost:3000/api",
        wsUrl: "ws://localhost:3000/ws",
    }),
);

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

async function main() {
    // Subscribe BEFORE firing the increments so we see every event.
    const seen: number[] = [];
    const unsubscribe = api.counterEvents.onChanged((value) => {
        console.log(`  ← ws: counter is now ${value}`);
        seen.push(value);
    });

    // Stateless call.
    console.log(`greet.hello("world")        → ${await api.greet.hello("world")}`);
    console.log(`greet.add(2, 3)             → ${await api.greet.add(2, 3)}`);

    // State + events.
    console.log(`\ncounter.current()          → ${await api.counter.current()}`);
    for (let i = 0; i < 3; i++) {
        console.log(`counter.increment()        → ${await api.counter.increment()}`);
    }

    // Events arrive on the WS asynchronously; give them a tick to flush
    // before we tear down.
    await sleep(100);
    console.log(`\nreceived ${seen.length} event(s): [${seen.join(", ")}]`);

    unsubscribe();
    // The defaultRpc keeps the WebSocket open until the process exits;
    // for a one-shot script we just bail.
    process.exit(0);
}

main().catch((err) => {
    if (err instanceof RpcError) {
        console.error(`rpc failed [${err.code}]: ${err.message}`);
    } else {
        console.error(err);
    }
    process.exit(1);
});
