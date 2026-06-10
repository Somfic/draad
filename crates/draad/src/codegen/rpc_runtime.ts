/** Returned by `Rpc.listen`, call to unsubscribe. */
export type UnlistenFn = () => void;

/** HTTP verb a generated method maps to. */
export type HttpVerb = "GET" | "POST" | "PUT" | "PATCH" | "DELETE";

/**
 * Transport contract the generated API classes call into. Implement this
 * in your app (using fetch / axios / tauri / etc.) and pass it to
 * `new Api(rpc)`. Or use `defaultRpc(...)` below.
 */
export interface Rpc {
	/**
	 * Send an RPC request. The wire command is `{namespace}/{method}`.
	 * Verb defaults to POST (body-bearing); GET/DELETE callers must pass
	 * the verb explicitly so `args` is serialised as a query string.
	 */
	call<T>(command: string, args?: Record<string, unknown>, method?: HttpVerb): Promise<T>;

	/** Subscribe to a backend event. Returns an unsubscribe handle. */
	listen<T>(topic: string, handler: (payload: T) => void): UnlistenFn;
}

/** Thrown (or returned) by `Rpc.call` implementations on failure. */
export class RpcError extends Error {
	constructor(public readonly code: string, message: string) {
		super(message);
		this.name = "RpcError";
	}
}

export type DefaultRpcOptions = {
	/** Base URL for RPC calls. Requests go to `${baseUrl}/${command}`. */
	baseUrl?: string;
	/** WebSocket URL for events. Omit to make `listen()` throw. */
	wsUrl?: string;
	/** Extra headers added to every RPC call. */
	headers?: Record<string, string>;
	/** Disable WebSocket auto-reconnect. Default: true. */
	reconnect?: boolean;
	/** Cap on the backoff between reconnect attempts. Default: 30000. */
	maxReconnectDelayMs?: number;
};

/**
 * Default `Rpc` implementation: POST JSON for calls, a shared WebSocket
 * for events. Wire format:
 *
 *  - Calls: `POST {baseUrl}/{command}` with body `args`. 2xx responses
 *    are parsed as JSON and returned; non-2xx are thrown as `RpcError`.
 *  - Events: WS frames shaped `{ topic: string, payload: T }`.
 *
 * Auto-reconnects with exponential backoff + jitter (capped at
 * `maxReconnectDelayMs`) as long as there are active subscribers. Replace
 * with your own `Rpc` for auth, custom transports, or a different wire
 * format.
 */
export function defaultRpc(opts: DefaultRpcOptions = {}): Rpc {
	const baseUrl = opts.baseUrl ?? "/api";
	const maxDelayMs = opts.maxReconnectDelayMs ?? 30000;
	const shouldReconnect = opts.reconnect !== false;
	const subs = new Map<string, Set<(payload: unknown) => void>>();
	let ws: WebSocket | undefined;
	let reconnectDelayMs = 500;
	let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

	function openWs(url: string): WebSocket {
		const sock = new WebSocket(url);
		sock.addEventListener("open", () => { reconnectDelayMs = 500; });
		sock.addEventListener("message", (ev) => {
			try {
				const { topic, payload } = JSON.parse(String(ev.data));
				subs.get(topic)?.forEach((h) => h(payload));
			} catch {
				/* ignore malformed frames */
			}
		});
		sock.addEventListener("close", () => {
			ws = undefined;
			if (!shouldReconnect || subs.size === 0) return;
			const base = Math.min(reconnectDelayMs, maxDelayMs);
			const jitter = base * (0.8 + Math.random() * 0.4);
			reconnectDelayMs = Math.min(reconnectDelayMs * 2, maxDelayMs);
			reconnectTimer = setTimeout(() => {
				reconnectTimer = undefined;
				if (subs.size > 0 && !ws) ws = openWs(url);
			}, jitter);
		});
		return sock;
	}

	function ensureWs(): WebSocket {
		if (!opts.wsUrl) {
			throw new RpcError("NO_WS_URL", "defaultRpc: wsUrl not configured; cannot listen()");
		}
		if (ws && ws.readyState !== WebSocket.CLOSED) return ws;
		ws = openWs(opts.wsUrl);
		return ws;
	}

	return {
		async call<T>(command: string, args?: Record<string, unknown>, method: HttpVerb = "POST"): Promise<T> {
			const hasBody = method === "POST" || method === "PUT" || method === "PATCH";
			let url = `${baseUrl}/${command}`;
			const init: RequestInit = { method, headers: { ...opts.headers } };
			if (hasBody) {
				(init.headers as Record<string, string>)["Content-Type"] = "application/json";
				init.body = JSON.stringify(args ?? {});
			} else if (args) {
				const params = new URLSearchParams();
				for (const [k, v] of Object.entries(args)) {
					if (v === undefined || v === null) continue;
					if (Array.isArray(v)) {
						for (const item of v) params.append(k, String(item));
					} else {
						params.append(k, String(v));
					}
				}
				const qs = params.toString();
				if (qs) url += `?${qs}`;
			}
			const res = await fetch(url, init);
			if (!res.ok) {
				throw new RpcError(`HTTP_${res.status}`, await res.text());
			}
			return (await res.json()) as T;
		},
		listen<T>(topic: string, handler: (payload: T) => void): UnlistenFn {
			ensureWs();
			let set = subs.get(topic);
			if (!set) {
				set = new Set();
				subs.set(topic, set);
			}
			const wrapped = handler as (p: unknown) => void;
			set.add(wrapped);
			return () => {
				set!.delete(wrapped);
				if (set!.size === 0) subs.delete(topic);
				if (subs.size === 0 && reconnectTimer) {
					clearTimeout(reconnectTimer);
					reconnectTimer = undefined;
				}
			};
		},
	};
}
